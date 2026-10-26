//! Desktop playback runtime and background media decoding worker.
//!
//! Spawns a dedicated playback coordinator thread that handles commands from the desktop GUI
//! (`PlayerCommand`), demuxes containers with `UniversalDemuxer`, decodes video frames into
//! the SPSC ring buffer for GPU viewport rendering, streams normalized audio to `AutoAudioSink`
//! over a decoupled asynchronous channel, and publishes atomic UI snapshots to `TripleBufferSnapshotExchange`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncplay_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncplay_codecs::{UniversalAudioDecoder, UniversalVideoDecoder, VideoFrameRGBA};
use syncplay_demux::{Demuxer, TrackType, UniversalDemuxer};
use syncplay_ui::{PlayerCommand, PlayerUiSnapshot, TripleBufferSnapshotExchange};

use crate::audio_sink::{AudioSink, AutoAudioSink};

/// Commands delivered to the asynchronous audio streaming thread.
enum AudioWorkerCommand {
    /// Normalized interleaved f32 PCM samples ready for soundcard delivery.
    Samples(Vec<f32>),
    /// Pauses audio playback output.
    Pause,
    /// Resumes audio playback output.
    Resume,
    /// Resets the audio sink, instantly terminating stale playback streams.
    Reset,
}

/// Runs the dedicated audio sink output loop on an isolated OS thread.
fn run_audio_sink_worker(rx: Receiver<AudioWorkerCommand>, running: Arc<AtomicBool>) {
    let mut sink = AutoAudioSink::new();

    while running.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(AudioWorkerCommand::Samples(samples)) => {
                let _ = sink.write_samples(&samples);
            }
            Ok(AudioWorkerCommand::Pause) => {
                let _ = sink.pause();
            }
            Ok(AudioWorkerCommand::Resume) => {
                let _ = sink.resume();
            }
            Ok(AudioWorkerCommand::Reset) => {
                sink = AutoAudioSink::new();
            }
            Err(_) => {}
        }
    }
}

/// Active media playback session managing demuxer and decoders.
struct PlaybackSession {
    demuxer: UniversalDemuxer,
    video_decoder: Option<UniversalVideoDecoder>,
    audio_decoder: Option<UniversalAudioDecoder>,
    video_track_id: Option<u64>,
    audio_track_id: Option<u64>,
    duration_ms: u64,
    eof_reached: bool,
}

impl PlaybackSession {
    /// Attempts to open a media file and initialize decoders for its primary video and audio streams.
    fn open(path: &PathBuf) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("File does not exist: {:?}", path));
        }
        let demuxer = UniversalDemuxer::open_file(path)
            .map_err(|e| format!("UniversalDemuxer open failure: {e}"))?;

        let info = demuxer
            .container_info()
            .cloned()
            .ok_or_else(|| "Failed to retrieve container metadata".to_string())?;

        let duration_ms = info.duration_ms.unwrap_or(0);

        let vt = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .cloned();
        let at = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Audio)
            .cloned();

        let video_track_id = vt.as_ref().map(|t| t.track_id);
        let audio_track_id = at.as_ref().map(|t| t.track_id);

        let video_decoder = if let Some(ref track) = vt {
            if let Some(params) = demuxer.stream_parameters(track.track_id as usize) {
                UniversalVideoDecoder::from_parameters(&params).ok()
            } else {
                UniversalVideoDecoder::from_codec_name(
                    &track.codec_id,
                    track.width,
                    track.height,
                    track.codec_private.as_deref(),
                )
                .ok()
            }
        } else {
            None
        };

        let audio_decoder = if let Some(ref track) = at {
            if let Some(params) = demuxer.stream_parameters(track.track_id as usize) {
                UniversalAudioDecoder::from_parameters(params, None).ok()
            } else {
                UniversalAudioDecoder::from_codec_name(
                    &track.codec_id,
                    None,
                    track.codec_private.as_deref(),
                )
                .ok()
            }
        } else {
            None
        };

        Ok(Self {
            demuxer,
            video_decoder,
            audio_decoder,
            video_track_id,
            audio_track_id,
            duration_ms,
            eof_reached: false,
        })
    }

    /// Performs random-access seek and flushes internal decoder buffers.
    fn seek(&mut self, target_ms: u64, video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>) {
        let _ = self.demuxer.seek_to(target_ms);
        if let Some(ref mut vdec) = self.video_decoder {
            vdec.flush();
        }
        if let Some(ref mut adec) = self.audio_decoder {
            let _ = adec.flush();
        }
        while video_ring.try_pop().is_some() {}
        self.eof_reached = false;
    }
}

/// Runs the background playback worker loop in a dedicated OS thread.
fn run_playback_worker(
    command_rx: Receiver<PlayerCommand>,
    audio_tx: Sender<AudioWorkerCommand>,
    exchange: Arc<TripleBufferSnapshotExchange>,
    video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
    clock: Arc<MasterClock>,
    running: Arc<AtomicBool>,
) {
    let mut session: Option<PlaybackSession> = None;
    let mut volume: f32 = 1.0;
    let mut is_muted: bool = false;
    let mut speed: f64 = 1.0;
    let mut overflow_frame: Option<VideoFrameRGBA> = None;
    let mut last_snapshot_instant = Instant::now();

    while running.load(Ordering::Relaxed) {
        // 1. Drain all pending UI commands
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                PlayerCommand::OpenFile(path) => {
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
                    match PlaybackSession::open(&path) {
                        Ok(new_session) => {
                            let dur_ms = new_session.duration_ms;
                            session = Some(new_session);
                            overflow_frame = None;
                            while video_ring.try_pop().is_some() {}

                            let now = Instant::now();
                            clock.seek_to(0.0, now);
                            clock.set_state(now, PlaybackState::Playing);

                            // Initial snapshot publication
                            let snap = PlayerUiSnapshot {
                                monotonic_timestamp: 0,
                                media_position_pts: 0,
                                media_duration_pts: dur_ms * 1_000_000,
                                playback_state: 1,
                                pll_speed_ratio_q16: (speed * 65536.0) as u32,
                                peer_count: 1,
                                peers_ready: 1,
                                room_sequence: 1,
                                ..Default::default()
                            };
                            exchange.publish(snap);
                        }
                        Err(err) => {
                            eprintln!("[ERROR] PlaybackSession failed to open {:?}: {}", path, err);
                        }
                    }
                }
                PlayerCommand::TogglePause => {
                    let now = Instant::now();
                    if clock.is_playing() {
                        clock.set_state(now, PlaybackState::Paused);
                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                    } else {
                        clock.set_state(now, PlaybackState::Playing);
                        let _ = audio_tx.send(AudioWorkerCommand::Resume);
                    }
                }
                PlayerCommand::Stop => {
                    let now = Instant::now();
                    clock.seek_to(0.0, now);
                    clock.set_state(now, PlaybackState::Paused);
                    overflow_frame = None;
                    while video_ring.try_pop().is_some() {}
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
                    if let Some(ref mut sess) = session {
                        sess.seek(0, &video_ring);
                    }
                }
                PlayerCommand::Seek(target_ms) => {
                    overflow_frame = None;
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
                    if let Some(ref mut sess) = session {
                        sess.seek(target_ms, &video_ring);
                        let now = Instant::now();
                        clock.seek_to(target_ms as f64, now);
                    }
                }
                PlayerCommand::SetSpeed(spd) => {
                    speed = spd.clamp(0.2, 4.0);
                    clock.reanchor(Instant::now(), speed);
                }
                PlayerCommand::SetVolume(vol) => {
                    volume = vol.clamp(0.0, 1.0);
                }
                PlayerCommand::ToggleMute => {
                    is_muted = !is_muted;
                }
                PlayerCommand::SendChatMessage(_) | PlayerCommand::ExecuteSlashCommand(_) => {
                    // Watch-party chat commands routed through syncplay-chat
                }
                PlayerCommand::HostParty { .. }
                | PlayerCommand::ConnectParty { .. }
                | PlayerCommand::DisconnectParty => {
                    // Handled in watch-party networking integration
                }
            }
        }

        // 2. Process playback decoding and audio output
        let mut did_work = false;
        if let Some(ref mut sess) = session {
            if clock.is_playing() && !sess.eof_reached {
                // First push any pending overflow frame if ring has space
                if let Some(frame) = overflow_frame.take() {
                    if let Err(err) = video_ring.try_push(frame) {
                        overflow_frame = Some(err.item);
                    } else {
                        did_work = true;
                    }
                }

                // If overflow is clear and video ring has capacity, demux & decode packets
                if overflow_frame.is_none() {
                    let mut packets_read = 0;
                    while packets_read < 16 {
                        match sess.demuxer.read_packet() {
                            Ok(Some(packet)) => {
                                packets_read += 1;
                                did_work = true;

                                // Video packet decoding
                                if Some(packet.track_id) == sess.video_track_id {
                                    if let Some(ref mut vdec) = sess.video_decoder {
                                        let _ = vdec.send_packet(
                                            &packet.data,
                                            packet.pts_ms,
                                            packet.dts_ms,
                                            packet.is_keyframe,
                                        );
                                        while let Ok(Some(rgba)) = vdec.receive_frame_rgba() {
                                            if let Err(err) = video_ring.try_push(rgba) {
                                                overflow_frame = Some(err.item);
                                                break;
                                            }
                                        }
                                    }
                                }
                                // Audio packet decoding & asynchronous output
                                else if Some(packet.track_id) == sess.audio_track_id {
                                    if let Some(ref mut adec) = sess.audio_decoder {
                                        if let Ok(pcm_frames) =
                                            adec.decode_packet(&packet.data, packet.pts_ms)
                                        {
                                            let effective_vol =
                                                if is_muted { 0.0 } else { volume };
                                            for pcm in pcm_frames {
                                                if effective_vol < 1e-4 {
                                                    let silence =
                                                        vec![0.0f32; pcm.samples.len()];
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::Samples(silence),
                                                    );
                                                } else if (effective_vol - 1.0).abs() < 1e-3 {
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::Samples(pcm.samples),
                                                    );
                                                } else {
                                                    let scaled: Vec<f32> = pcm
                                                        .samples
                                                        .iter()
                                                        .map(|s| s * effective_vol)
                                                        .collect();
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::Samples(scaled),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                // Rate-limit demuxer for audio-only streams
                                if sess.video_track_id.is_none() {
                                    let now_pts = clock.current_pts(Instant::now());
                                    if (packet.pts_ms as f64) > now_pts + 500.0 {
                                        break;
                                    }
                                }

                                if overflow_frame.is_some() {
                                    break;
                                }
                            }
                            Ok(None) => {
                                sess.eof_reached = true;
                                break;
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 3. Publish atomic telemetry snapshot to UI (~40 Hz)
        let now = Instant::now();
        if now.duration_since(last_snapshot_instant) >= Duration::from_millis(25) {
            last_snapshot_instant = now;
            let current_pts_ms = clock.current_pts(now);
            let dur_ms = session.as_ref().map(|s| s.duration_ms).unwrap_or(0);
            let is_playing = clock.is_playing();

            let snap = PlayerUiSnapshot {
                monotonic_timestamp: now.elapsed().as_nanos() as u64,
                media_position_pts: (current_pts_ms.max(0.0) * 1_000_000.0) as u64,
                media_duration_pts: dur_ms * 1_000_000,
                playback_state: if is_playing { 1 } else { 0 },
                network_rtt_nanos: 0,
                clock_offset_nanos: 0,
                phase_error_nanos: 0,
                pll_speed_ratio_q16: (speed * 65536.0) as u32,
                peer_count: 1,
                peers_ready: 1,
                room_sequence: 1,
            };
            exchange.publish(snap);
        }

        // 4. Rate-limit thread execution
        if !did_work {
            thread::sleep(Duration::from_millis(10));
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Launches the full desktop graphical player with integrated background decoding and presentation runtime.
pub fn launch_desktop_player_app(
    initial_file: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let running = Arc::new(AtomicBool::new(true));

    // Spawn dedicated audio sink worker thread
    let audio_running = Arc::clone(&running);
    let audio_handle = thread::Builder::new()
        .name("syncplay-audio-worker".to_string())
        .spawn(move || {
            run_audio_sink_worker(audio_rx, audio_running);
        })
        .map_err(|e| format!("Failed to spawn audio worker thread: {e}"))?;

    // Spawn dedicated background playback coordinator thread
    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let worker_handle = thread::Builder::new()
        .name("syncplay-playback-worker".to_string())
        .spawn(move || {
            run_playback_worker(
                command_rx,
                audio_tx,
                thread_exchange,
                thread_video_ring,
                thread_clock,
                thread_running,
            );
        })
        .map_err(|e| format!("Failed to spawn playback worker thread: {e}"))?;

    // Run native graphical player on main thread (blocks until window is closed)
    let gui_res = syncplay_ui::run_desktop_player(
        initial_file,
        exchange,
        video_ring,
        clock,
        command_tx,
    );

    // Stop background workers on exit
    running.store(false, Ordering::Relaxed);
    let _ = worker_handle.join();
    let _ = audio_handle.join();

    gui_res.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}
