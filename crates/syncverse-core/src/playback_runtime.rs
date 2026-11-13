//! Desktop playback runtime and background media decoding worker.
//!
//! Spawns a dedicated playback coordinator thread that handles commands from the desktop GUI
//! (`PlayerCommand`), demuxes containers with `UniversalDemuxer`, decodes video frames into
//! the SPSC ring buffer for GPU viewport rendering, streams normalized audio to `AutoAudioSink`
//! over a decoupled asynchronous channel, and publishes atomic UI snapshots to `TripleBufferSnapshotExchange`.

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use syncverse_chat::{
    format_help, format_sync_status_report, format_timecode, ChatRecord, ParsedSlashCommand,
    RoomAnnouncement, SlashCommandParser, SyncDiagnostics,
};
use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::audio::LinearResampler;
use syncverse_codecs::frame::RgbaFramePool;
use syncverse_codecs::{
    apply_gain, UniversalAudioDecoder, UniversalVideoDecoder, VideoFrameRGBA,
};
use syncverse_common::subtitles::{SubtitleSource, SubtitleTrack as CommonSubtitleTrack};
use syncverse_demux::{DemuxPacket, Demuxer, TrackInfo, TrackType, UniversalDemuxer};
use syncverse_sync::{
    spawn_upnp_mapping, ClientNode, HostServer, MediaFingerprint, PeerControlAction,
    PeerStatusPayload, RoomCryptoSession,
};
use syncverse_ui::{
    evaluate_file_match, FileMatchStatus, PlayerCommand, PlayerUiSnapshot, RoomMember,
    TripleBufferSnapshotExchange, UiEvent, SNAPSHOT_FLAG_FORCE_PLAY, SNAPSHOT_FLAG_GATE_BLOCKED,
    SNAPSHOT_FLAG_HAS_MISMATCH, SNAPSHOT_FLAG_LOCAL_READY, SNAPSHOT_FLAG_LOOP_ENABLED,
    SNAPSHOT_FLAG_MUTED,
};

use crate::audio_sink::{create_default_audio_sink, AudioSink};
use crate::error::CoreError;
use crate::subtitle_runtime::{
    build_embedded_subtitle_decoders, choose_initial_subtitle_track,
    composite_active_bitmap_subtitle, find_exact_stem_sidecar_languages,
    process_embedded_subtitle_packet, to_ui_subtitle_tracks, EmbeddedSubtitleDecoderEntry,
};
use crate::sync_runtime::{
    broadcast_chat_announcement, broadcast_local_peer_status, get_sender_name,
    poll_host_upnp_status, send_client_peer_control, WatchPartyRole,
};

/// Commands delivered to the asynchronous audio streaming thread.
#[derive(Debug, Clone)]
pub enum AudioWorkerCommand {
    /// Normalized interleaved f32 PCM samples ready for soundcard delivery.
    Samples(Vec<f32>),
    /// Timestamped normalized interleaved f32 PCM samples with presentation timestamp and seek generation.
    TimestampedSamples {
        /// Media presentation timestamp of the first sample in milliseconds.
        pts_ms: u64,
        /// Normalized interleaved f32 audio samples.
        samples: Vec<f32>,
        /// Monotonically increasing seek generation epoch to instantly reject stale frames.
        generation: u64,
    },
    /// Pauses audio playback output.
    Pause,
    /// Resumes audio playback output.
    Resume,
    /// Resets the audio sink, instantly terminating stale playback streams.
    Reset,
    /// Update 10-band audio equalizer settings.
    SetEqualizer(syncverse_codecs::audio::AudioEqualizerConfig),
    /// Update audio dynamic compressor (night mode) settings.
    SetCompressor(Option<syncverse_codecs::audio::AudioDynamicsCompressor>),
    /// Update stereo balance panning [-1.0, 1.0].
    SetStereoPan(f32),
    /// Update output volume gain `[0.0, 2.0]`.
    SetVolume(f32),
    /// Update mute state non-destructively.
    SetMuted(bool),
    /// Update playback speed multiplier `[0.2, 4.0]` for real-time resampling.
    SetSpeed(f64),
    /// Update audio presentation delay offset in milliseconds `[-5000, 5000]`.
    SetAudioDelay(i64),
    /// Cleanly terminates the audio worker thread.
    Shutdown,
}

/// Dispatches a single [`AudioWorkerCommand`] inside [`run_audio_sink_worker`].
///
/// Returns `false` when [`AudioWorkerCommand::Shutdown`] is received.
#[allow(clippy::too_many_arguments)]
fn handle_audio_worker_command(
    cmd: AudioWorkerCommand,
    sink: &mut Box<dyn AudioSink>,
    local_generation: &mut u64,
    pending_queue: &mut VecDeque<(u64, Vec<f32>, u64)>,
    needs_fade_in: &mut bool,
    equalizer: &mut syncverse_codecs::audio::AudioEqualizer10Band,
    compressor: &mut Option<syncverse_codecs::audio::AudioDynamicsCompressor>,
    stereo_pan: &mut f32,
    volume: &mut f32,
    muted: &mut bool,
    speed: &mut f64,
    audio_delay_ms: &mut i64,
    resampler: &LinearResampler,
    clock: &MasterClock,
) -> bool {
    match cmd {
        AudioWorkerCommand::Shutdown => return false,
        AudioWorkerCommand::Reset => {
            *local_generation = local_generation.wrapping_add(1);
            equalizer.reset();
            if let Some(ref mut c) = compressor {
                c.reset();
            }
            resampler.reset();
            // Smooth micro-fade-out of remaining front audio to eliminate DC offset clicks/pops
            if let Some((_, ref samples, _)) = pending_queue.front() {
                let fade_len = samples.len().min(256);
                if fade_len > 0 {
                    let mut fade_buf = samples[..fade_len].to_vec();
                    for (i, s) in fade_buf.iter_mut().enumerate() {
                        *s *= 1.0 - (i as f32 / fade_len as f32);
                    }
                    let _ = sink.write_samples(&fade_buf);
                }
            }
            let _ = sink.flush();
            pending_queue.clear();
            *needs_fade_in = true;
        }
        AudioWorkerCommand::Pause => {
            let _ = sink.pause();
        }
        AudioWorkerCommand::Resume => {
            let _ = sink.resume();
        }
        AudioWorkerCommand::SetEqualizer(cfg) => {
            equalizer.set_config(cfg);
        }
        AudioWorkerCommand::SetCompressor(comp) => {
            *compressor = comp;
        }
        AudioWorkerCommand::SetStereoPan(pan) => {
            *stereo_pan = pan;
        }
        AudioWorkerCommand::SetVolume(vol) => {
            *volume = vol.clamp(0.0, 2.0);
            let effective = if *muted { 0.0 } else { *volume };
            sink.set_volume(effective);
        }
        AudioWorkerCommand::SetMuted(m) => {
            *muted = m;
            sink.set_muted(*muted);
            let effective = if *muted { 0.0 } else { *volume };
            sink.set_volume(effective);
        }
        AudioWorkerCommand::SetSpeed(spd) => {
            let clamped = spd.clamp(0.2, 4.0);
            let was_bypass = (*speed - 1.0).abs() < 1e-4;
            let now_active = (clamped - 1.0).abs() >= 1e-4;
            if was_bypass && now_active {
                resampler.reset();
            }
            *speed = clamped;
            let _ = resampler.set_speed_ratio(clamped);
        }
        AudioWorkerCommand::SetAudioDelay(delay) => {
            *audio_delay_ms = delay.clamp(-5000, 5000);
        }
        AudioWorkerCommand::TimestampedSamples {
            pts_ms,
            samples,
            generation,
        } => {
            if generation >= *local_generation && !samples.is_empty() {
                pending_queue.push_back((pts_ms, samples, generation));
            }
        }
        AudioWorkerCommand::Samples(samples) => {
            if !samples.is_empty() {
                let now_pts = clock.current_pts(Instant::now()).max(0.0) as u64;
                pending_queue.push_back((now_pts, samples, *local_generation));
            }
        }
    }
    true
}

/// Runs the dedicated audio sink output loop on an isolated OS thread with MasterClock pacing and closed-loop sync.
fn run_audio_sink_worker(
    rx: Receiver<AudioWorkerCommand>,
    clock: Arc<MasterClock>,
    running: Arc<AtomicBool>,
) {
    let mut sink = create_default_audio_sink(false);
    let mut local_generation: u64 = 0;
    let mut pending_queue: VecDeque<(u64, Vec<f32>, u64)> = VecDeque::with_capacity(128);
    let mut needs_fade_in = false;
    let mut equalizer = syncverse_codecs::audio::AudioEqualizer10Band::default();
    let mut compressor: Option<syncverse_codecs::audio::AudioDynamicsCompressor> = None;
    let mut stereo_pan: f32 = 0.0;
    let mut volume: f32 = 1.0;
    let mut muted: bool = false;
    let mut speed: f64 = 1.0;
    let mut audio_delay_ms: i64 = 0;
    let resampler = LinearResampler::new(48_000, 48_000, 2)
        .expect("48kHz stereo LinearResampler initialization must succeed");
    let mut resampled_buf: Vec<f32> = Vec::with_capacity(4096);

    while running.load(Ordering::Relaxed) {
        // 1. Drain incoming commands non-blockingly to catch Resets, control changes, and fresh samples
        while let Ok(cmd) = rx.try_recv() {
            if !handle_audio_worker_command(
                cmd,
                &mut sink,
                &mut local_generation,
                &mut pending_queue,
                &mut needs_fade_in,
                &mut equalizer,
                &mut compressor,
                &mut stereo_pan,
                &mut volume,
                &mut muted,
                &mut speed,
                &mut audio_delay_ms,
                &resampler,
                &clock,
            ) {
                return;
            }
        }

        // Report remaining DAC hardware buffer latency excluding queued ring samples
        let dac_latency_ms = sink.latency_ms().saturating_sub(sink.queued_ms());
        clock.set_audio_latency_ms(dac_latency_ms);

        // 2. If clock is not playing, sleep briefly and loop
        if !clock.is_playing() {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        // 3. If pending queue is empty, wait briefly for new commands
        if pending_queue.is_empty() {
            if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(5)) {
                if !handle_audio_worker_command(
                    cmd,
                    &mut sink,
                    &mut local_generation,
                    &mut pending_queue,
                    &mut needs_fade_in,
                    &mut equalizer,
                    &mut compressor,
                    &mut stereo_pan,
                    &mut volume,
                    &mut muted,
                    &mut speed,
                    &mut audio_delay_ms,
                    &resampler,
                    &clock,
                ) {
                    return;
                }
            }
            continue;
        }

        // 4. Process candidate audio frame from front of pending queue
        let (pts_ms, generation) = match pending_queue.front() {
            Some(&(pts, _, gen)) => (pts, gen),
            None => continue,
        };

        if generation < local_generation {
            pending_queue.pop_front();
            continue;
        }

        let now = Instant::now();
        let master_pts = clock.current_pts(now);
        let effective_pts_ms = (pts_ms as i64 + audio_delay_ms).max(0) as f64;
        let delta = effective_pts_ms - master_pts;

        if delta < -500.0 {
            // Severely late frame (> 500ms behind master clock): drop to resynchronize
            pending_queue.pop_front();
            continue;
        } else if delta > 80.0 || sink.queued_ms() >= 80 {
            // Soundcard ring already holds >= 80ms or frame is ahead of target lead window
            let sleep_ms = if delta > 80.0 {
                (delta - 50.0).clamp(1.0, 10.0) as u64
            } else {
                3
            };
            thread::sleep(Duration::from_millis(sleep_ms));
            continue;
        } else {
            // Frame is within presentation window (-500ms <= delta <= 80ms) and sink queued < 80ms
            let (_, mut samples, _) = pending_queue.pop_front().unwrap();
            if needs_fade_in && !samples.is_empty() {
                let fade_len = samples.len().min(256);
                for (i, s) in samples[..fade_len].iter_mut().enumerate() {
                    *s *= i as f32 / fade_len as f32;
                }
                needs_fade_in = false;
            }

            // Apply stereo panning, dynamic compression, and 10-band equalizer before resampling
            if stereo_pan.abs() > 0.01 {
                syncverse_codecs::audio::AudioStereoPan::pan_interleaved(&mut samples, stereo_pan);
            }
            if let Some(ref mut c) = compressor {
                c.process(&mut samples);
            }
            if equalizer.is_active() {
                equalizer.process_interleaved(&mut samples);
            }

            // Continuous micro-speed / playback rate resampling (bypassed at 1.0x)
            if (speed - 1.0).abs() >= 1e-4
                && resampler.resample_into(&samples, &mut resampled_buf).is_ok()
            {
                std::mem::swap(&mut samples, &mut resampled_buf);
            }

            if samples.is_empty() {
                continue;
            }

            // Apply software gain only if the backend does not scale gain internally in its callback
            if !sink.handles_gain_internally() {
                let effective_vol = if muted { 0.0 } else { volume };
                if (effective_vol - 1.0).abs() > 1e-4 {
                    apply_gain(&mut samples, effective_vol);
                }
            }

            if sink.write_samples(&samples).is_err() {
                sink = create_default_audio_sink(false);
                let effective_vol = if muted { 0.0 } else { volume };
                sink.set_volume(effective_vol);
                sink.set_muted(muted);
                let _ = sink.write_samples(&samples);
            }
        }
    }
}

/// Formats track info into a human-readable display string.
fn format_track_info(track: &TrackInfo) -> String {
    let mut parts = Vec::new();
    if let Some(ref title) = track.title {
        if !title.is_empty() {
            parts.push(title.clone());
        }
    }
    if let Some(ref lang) = track.language {
        if !lang.is_empty() {
            parts.push(lang.to_uppercase());
        }
    }
    if parts.is_empty() {
        parts.push(format!("Track {}", track.track_id));
    }
    if !track.codec_id.is_empty() {
        parts.push(track.codec_id.clone());
    }
    if let Some(ch) = track.channels {
        match ch {
            1 => parts.push("Mono".to_string()),
            2 => parts.push("Stereo".to_string()),
            n => parts.push(format!("{n}ch")),
        }
    }
    parts.join(" - ")
}

/// Active media playback session managing dual demuxers, decoders, pooled RGBA frames, and subtitles.
struct PlaybackSession {
    demuxer: UniversalDemuxer,
    audio_demuxer: Option<UniversalDemuxer>,
    video_decoder: Option<UniversalVideoDecoder>,
    audio_decoder: Option<UniversalAudioDecoder>,
    video_track_id: Option<u64>,
    audio_track_id: Option<u64>,
    available_audio_tracks: Vec<TrackInfo>,
    available_subtitle_tracks: Vec<CommonSubtitleTrack>,
    selected_subtitle_track_index: Option<usize>,
    subtitle_track_id: Option<u64>,
    subtitle_decoders: HashMap<u64, EmbeddedSubtitleDecoderEntry>,
    duration_ms: u64,
    eof_reached: bool,
    audio_eof_reached: bool,
    pending_video_packets: VecDeque<DemuxPacket>,
    unsent_video_packet: Option<DemuxPacket>,
    video_eof_sent: bool,
    last_demuxed_video_dts: Option<u64>,
    last_demuxed_audio_pts_ms: Option<u64>,
    rgba_pool: RgbaFramePool,
    video_fps: Option<f64>,
    video_metadata: syncverse_ui::gui::dialogs::VideoStreamMetadata,
    chapters: Vec<syncverse_demux::MediaChapter>,
}

impl PlaybackSession {
    /// Attempts to open a media file and initialize demuxers, decoders, subtitle parsers, and pools.
    fn open(path: &PathBuf) -> Result<Self, CoreError> {
        if !path.exists() {
            return Err(CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File does not exist: {:?}", path),
            )));
        }
        let mut demuxer = UniversalDemuxer::open_file(path)?;

        let info = demuxer
            .container_info()
            .cloned()
            .ok_or_else(|| CoreError::Runtime("Failed to retrieve container metadata".to_string()))?;

        let duration_ms = info.duration_ms.unwrap_or(0);

        let available_audio_tracks: Vec<TrackInfo> = info
            .tracks
            .iter()
            .filter(|t| t.track_type == TrackType::Audio)
            .cloned()
            .collect();

        let embedded_subtitle_tracks: Vec<TrackInfo> = info
            .tracks
            .iter()
            .filter(|t| t.track_type == TrackType::Subtitle)
            .cloned()
            .collect();

        let mut available_subtitle_tracks = Vec::new();
        let mut subtitle_embedded_flags = Vec::new();
        let mut stream_to_track_index = HashMap::new();
        for t in embedded_subtitle_tracks {
            let track_idx = available_subtitle_tracks.len();
            let title = t.title.clone().unwrap_or_else(|| {
                if let Some(ref lang) = t.language {
                    format!("Embedded Subtitle [{}]", lang.to_uppercase())
                } else {
                    format!("Embedded Subtitle (Track {})", t.track_id)
                }
            });
            stream_to_track_index.insert(t.track_id, track_idx);
            subtitle_embedded_flags.push((t.is_forced, t.is_default));
            available_subtitle_tracks.push(CommonSubtitleTrack::new(
                t.track_id as usize,
                SubtitleSource::Embedded,
                t.codec_id.clone(),
                t.language.clone(),
                title,
                Vec::new(),
            ));
        }

        let vt = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .cloned();
        let at = available_audio_tracks.first().cloned();

        let video_track_id = vt.as_ref().map(|t| t.track_id);
        let audio_track_id = at.as_ref().map(|t| t.track_id);

        let video_frame_rate = vt
            .as_ref()
            .and_then(|v| demuxer.stream_frame_rate(v.track_id as usize));

        let external_tracks =
            syncverse_demux::subtitles::scan_external_subtitles_with_fps(path, video_frame_rate);
        let has_exact_stem_external_subtitle = !find_exact_stem_sidecar_languages(path).is_empty();
        available_subtitle_tracks.extend(external_tracks);

        let subtitle_decoders = build_embedded_subtitle_decoders(
            &demuxer,
            &info.tracks,
            &stream_to_track_index,
        );

        let selected_subtitle_track_index = choose_initial_subtitle_track(
            &available_subtitle_tracks,
            &subtitle_embedded_flags,
            has_exact_stem_external_subtitle,
            Some("en"),
        );
        let subtitle_track_id = selected_subtitle_track_index.and_then(|idx| {
            let t = &available_subtitle_tracks[idx];
            if t.source == SubtitleSource::Embedded {
                Some(t.id as u64)
            } else {
                None
            }
        });

        // Open independent audio demuxer when both video and audio streams exist for <1ms track switch
        let mut audio_demuxer = if video_track_id.is_some() && audio_track_id.is_some() {
            UniversalDemuxer::open_file(path).ok()
        } else {
            None
        };

        if let Some(ref mut ad) = audio_demuxer {
            ad.set_active_streams(None, audio_track_id, false);
            demuxer.set_active_streams(video_track_id, None, true);
        } else {
            demuxer.set_active_streams(video_track_id, audio_track_id, true);
        }

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

        let audio_demuxer_ref = audio_demuxer.as_ref().unwrap_or(&demuxer);
        let audio_decoder = if let Some(ref track) = at {
            let tb = audio_demuxer_ref.stream_time_base(track.track_id as usize);
            if let Some(params) = audio_demuxer_ref.stream_parameters(track.track_id as usize) {
                UniversalAudioDecoder::from_parameters(params, tb).ok()
            } else {
                UniversalAudioDecoder::from_codec_name(
                    &track.codec_id,
                    tb,
                    track.codec_private.as_deref(),
                )
                .ok()
            }
        } else {
            None
        };

        let video_metadata = syncverse_ui::gui::dialogs::VideoStreamMetadata {
            width: vt.as_ref().and_then(|v| v.width),
            height: vt.as_ref().and_then(|v| v.height),
            codec: vt.as_ref().map(|v| v.codec_id.clone()),
            frame_rate: video_frame_rate,
            bit_depth: Some(8),
            pixel_format: Some("yuv420p".to_string()),
            timescale: Some(info.timescale),
        };
        let chapters = demuxer.chapters().to_vec();

        Ok(Self {
            demuxer,
            audio_demuxer,
            video_decoder,
            audio_decoder,
            video_track_id,
            audio_track_id,
            available_audio_tracks,
            available_subtitle_tracks,
            selected_subtitle_track_index,
            subtitle_track_id,
            subtitle_decoders,
            duration_ms,
            eof_reached: false,
            audio_eof_reached: false,
            pending_video_packets: VecDeque::with_capacity(64),
            unsent_video_packet: None,
            video_eof_sent: false,
            last_demuxed_video_dts: None,
            last_demuxed_audio_pts_ms: None,
            rgba_pool: RgbaFramePool::new(48),
            video_fps: video_frame_rate,
            video_metadata,
            chapters,
        })
    }

    /// Switches the active subtitle track to the specified index (`None` = Off/Disabled).
    /// Returns the human-readable description of the selected track or "Off".
    fn select_subtitle_track(&mut self, track_idx: Option<usize>) -> (Option<usize>, String) {
        match track_idx {
            Some(idx) if idx < self.available_subtitle_tracks.len() => {
                self.selected_subtitle_track_index = Some(idx);
                let track = &self.available_subtitle_tracks[idx];
                self.subtitle_track_id = if track.source == SubtitleSource::Embedded {
                    Some(track.id as u64)
                } else {
                    None
                };
                (Some(idx), track.display_label())
            }
            _ => {
                self.selected_subtitle_track_index = None;
                self.subtitle_track_id = None;
                (None, "Off".to_string())
            }
        }
    }

    /// Cycles sequentially to the next available subtitle track (including Off).
    /// Sequence: None -> Some(0) -> Some(1) -> ... -> Some(N-1) -> None.
    fn cycle_subtitle_track(&mut self) -> (Option<usize>, String) {
        if self.available_subtitle_tracks.is_empty() {
            return (None, "Off".to_string());
        }
        let next_idx = match self.selected_subtitle_track_index {
            None => Some(0),
            Some(idx) if idx + 1 < self.available_subtitle_tracks.len() => Some(idx + 1),
            Some(_) => None,
        };
        self.select_subtitle_track(next_idx)
    }

    /// Switches the active audio track to the specified `track_id`.
    ///
    /// Constructs a new `UniversalAudioDecoder` for the selected stream and updates
    /// stream discard masks on the audio demuxer (if dedicated) or primary demuxer.
    fn select_audio_track(&mut self, track_id: u64) -> Result<String, CoreError> {
        let track = self
            .available_audio_tracks
            .iter()
            .find(|t| t.track_id == track_id)
            .cloned()
            .ok_or_else(|| CoreError::Runtime(format!("Audio track ID {} not found", track_id)))?;

        let tb = {
            let source_demuxer = self.audio_demuxer.as_ref().unwrap_or(&self.demuxer);
            source_demuxer.stream_time_base(track.track_id as usize)
        };
        let new_decoder = {
            let source_demuxer = self.audio_demuxer.as_ref().unwrap_or(&self.demuxer);
            if let Some(params) = source_demuxer.stream_parameters(track.track_id as usize) {
                UniversalAudioDecoder::from_parameters(params, tb)?
            } else {
                UniversalAudioDecoder::from_codec_name(
                    &track.codec_id,
                    tb,
                    track.codec_private.as_deref(),
                )?
            }
        };

        self.audio_decoder = Some(new_decoder);
        self.audio_track_id = Some(track_id);

        if let Some(ref mut ad) = self.audio_demuxer {
            ad.set_active_streams(None, Some(track_id), false);
        } else {
            self.demuxer
                .set_active_streams(self.video_track_id, Some(track_id), true);
        }

        let desc = format_track_info(&track);
        Ok(desc)
    }

    /// Cycles sequentially to the next available audio track, wrapping around to 0.
    ///
    /// Returns the newly selected track ID and description on success, or None if no audio tracks exist.
    fn cycle_audio_track(&mut self) -> Option<(u64, String)> {
        if self.available_audio_tracks.is_empty() {
            return None;
        }

        let current_idx = self
            .available_audio_tracks
            .iter()
            .position(|t| Some(t.track_id) == self.audio_track_id)
            .unwrap_or(0);

        let next_idx = (current_idx + 1) % self.available_audio_tracks.len();
        let next_track_id = self.available_audio_tracks[next_idx].track_id;

        match self.select_audio_track(next_track_id) {
            Ok(desc) => Some((next_track_id, desc)),
            Err(e) => {
                eprintln!("[WARN] Failed to switch to audio track {next_track_id}: {e}");
                None
            }
        }
    }

    /// Emits decoded PCM frames with boundary trimming at `target_ms` and unity software gain.
    fn trim_and_send_audio_frames(
        pcm_frames: Vec<syncverse_codecs::AudioFramePCM>,
        target_ms: u64,
        audio_delay_ms: i64,
        audio_tx: &Sender<AudioWorkerCommand>,
        audio_generation: u64,
    ) -> bool {
        let mut sent_any = false;
        for mut pcm in pcm_frames {
            let effective_pts = (pcm.pts_ms as i64 + audio_delay_ms).max(0) as u64;
            if effective_pts < target_ms {
                let diff_ms = target_ms - effective_pts;
                let num_frames = (diff_ms * pcm.sample_rate as u64 / 1000) as usize;
                let float_count = num_frames * pcm.channels as usize;
                if float_count < pcm.samples.len() {
                    pcm.samples.drain(0..float_count);
                    pcm.pts_ms = pcm.pts_ms.saturating_add(diff_ms);
                } else {
                    continue;
                }
            }
            if !pcm.samples.is_empty() {
                let _ = audio_tx.send(AudioWorkerCommand::TimestampedSamples {
                    pts_ms: pcm.pts_ms,
                    samples: pcm.samples,
                    generation: audio_generation,
                });
                sent_any = true;
            }
        }
        sent_any
    }

    /// Performs synchronous random-access seek and keyframe preroll forward to `target_ms`.
    ///
    /// Never pops from `video_ring` (worker owns the producer side only). Decodes up to 600
    /// reference video packets into the `RgbaFramePool`, handles near-EOF draining safely, decodes
    /// interleaved embedded subtitles, and prerolls up to 64 audio packets with unity gain.
    ///
    /// Returns `(actual_landed_pts, first_video_frame)` so the caller updates `MasterClock`
    /// once and stamps `first_video_frame` with the final post-seek `seek_generation()`.
    #[allow(clippy::too_many_arguments)]
    fn seek_preroll(
        &mut self,
        target_ms: u64,
        audio_tx: &Sender<AudioWorkerCommand>,
        audio_generation: u64,
        audio_delay_ms: i64,
        subtitle_delay_ms: i64,
        ui_event_tx: Option<&Sender<UiEvent>>,
    ) -> (u64, Option<VideoFrameRGBA>) {
        self.pending_video_packets.clear();
        self.unsent_video_packet = None;
        self.video_eof_sent = false;
        let _ = self.demuxer.seek_to(target_ms);
        if let Some(ref mut ad) = self.audio_demuxer {
            let _ = ad.seek_to(target_ms);
        }
        self.last_demuxed_video_dts = Some(target_ms);
        self.last_demuxed_audio_pts_ms = Some(target_ms);

        if let Some(ref mut vdec) = self.video_decoder {
            vdec.flush();
        }
        if let Some(ref mut adec) = self.audio_decoder {
            adec.flush_buffers();
        }
        for entry in self.subtitle_decoders.values_mut() {
            entry.decoder.flush();
        }
        self.eof_reached = false;
        self.audio_eof_reached = false;

        let mut first_video_frame: Option<VideoFrameRGBA> = None;
        let mut last_drained_frame: Option<VideoFrameRGBA> = None;
        let mut first_audio_sent = false;

        const MAX_VIDEO_PREROLL_PACKETS: usize = 600;
        const MAX_TOTAL_READ_PACKETS: usize = 5000;
        let mut video_packets_sent = 0usize;
        let mut packets_read = 0usize;

        while packets_read < MAX_TOTAL_READ_PACKETS {
            match self.demuxer.read_packet() {
                Ok(Some(packet)) => {
                    packets_read += 1;

                    if Some(packet.track_id) == self.video_track_id {
                        let pkt_dts = packet.dts_ms.unwrap_or(packet.pts_ms);
                        self.last_demuxed_video_dts = Some(pkt_dts);
                        if first_video_frame.is_none() {
                            if video_packets_sent >= MAX_VIDEO_PREROLL_PACKETS {
                                self.pending_video_packets.push_back(packet);
                                break;
                            }
                            if let Some(ref mut vdec) = self.video_decoder {
                                let mut sent = false;
                                for _ in 0..16 {
                                    match vdec.try_send_packet_with_dts(
                                        &packet.data,
                                        packet.pts_ms,
                                        packet.dts_ms,
                                        packet.is_keyframe,
                                    ) {
                                        Ok(true) => {
                                            sent = true;
                                            break;
                                        }
                                        Ok(false) => {
                                            if let Ok(Some(rgba)) = vdec.receive_preroll_or_rgba_pooled(
                                                target_ms,
                                                &self.rgba_pool,
                                                0,
                                            ) {
                                                first_video_frame = Some(rgba);
                                                break;
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                                if sent {
                                    video_packets_sent += 1;
                                    if let Ok(Some(rgba)) = vdec.receive_preroll_or_rgba_pooled(
                                        target_ms,
                                        &self.rgba_pool,
                                        0,
                                    ) {
                                        first_video_frame = Some(rgba);
                                    }
                                } else if first_video_frame.is_none() {
                                    self.unsent_video_packet = Some(packet);
                                    break;
                                } else {
                                    self.pending_video_packets.push_back(packet);
                                }
                            }
                        } else {
                            self.pending_video_packets.push_back(packet);
                        }
                    } else if self.audio_demuxer.is_none()
                        && Some(packet.track_id) == self.audio_track_id
                    {
                        let dur = packet.duration_ms.unwrap_or(25);
                        let delayed_pts = (packet.pts_ms as i64 + audio_delay_ms).max(0) as u64;
                        if delayed_pts + dur >= target_ms {
                            self.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                            if let Some(ref mut adec) = self.audio_decoder {
                                if let Ok(pcm_frames) =
                                    adec.decode_packet_us(&packet.data, packet.pts_us)
                                {
                                    if Self::trim_and_send_audio_frames(
                                        pcm_frames,
                                        target_ms,
                                        audio_delay_ms,
                                        audio_tx,
                                        audio_generation,
                                    ) {
                                        first_audio_sent = true;
                                    }
                                }
                            }
                        }
                    } else if let Some(entry) = self.subtitle_decoders.get_mut(&packet.track_id) {
                        process_embedded_subtitle_packet(
                            entry,
                            &packet.data,
                            packet.pts_ms,
                            packet.duration_ms.unwrap_or(3000),
                            &mut self.available_subtitle_tracks,
                            self.selected_subtitle_track_index,
                            ui_event_tx,
                        );
                    }

                    let v_done = self.video_track_id.is_none() || first_video_frame.is_some();
                    let a_done = self.audio_demuxer.is_some()
                        || self.audio_track_id.is_none()
                        || first_audio_sent;
                    if v_done && a_done {
                        break;
                    }
                }
                Ok(None) => {
                    self.eof_reached = true;
                    break;
                }
                Err(_) => break,
            }
        }

        // Near-EOF or extreme-GOP drain fallback so a valid frame is surfaced instead of blanking
        if first_video_frame.is_none() {
            if let Some(ref mut vdec) = self.video_decoder {
                if self.eof_reached && !self.video_eof_sent {
                    let _ = vdec.send_eof();
                    self.video_eof_sent = true;
                }
                while let Ok(Some(rgba)) = vdec.receive_frame_rgba_pooled(&self.rgba_pool, 0) {
                    last_drained_frame = Some(rgba);
                }
                if last_drained_frame.is_some() {
                    first_video_frame = last_drained_frame;
                } else if self.eof_reached && target_ms > 0 {
                    // Demuxer landed right past the final packet; step back slightly and recover last frame
                    let fallback_ms = target_ms.saturating_sub(500);
                    let _ = self.demuxer.seek_to(fallback_ms);
                    vdec.flush();
                    self.eof_reached = false;
                    self.video_eof_sent = false;
                    let mut fallback_pkts = 0usize;
                    while fallback_pkts < 64 {
                        match self.demuxer.read_packet() {
                            Ok(Some(packet)) => {
                                if Some(packet.track_id) == self.video_track_id {
                                    fallback_pkts += 1;
                                    let _ = vdec.try_send_packet_with_dts(
                                        &packet.data,
                                        packet.pts_ms,
                                        packet.dts_ms,
                                        packet.is_keyframe,
                                    );
                                    while let Ok(Some(rgba)) =
                                        vdec.receive_frame_rgba_pooled(&self.rgba_pool, 0)
                                    {
                                        last_drained_frame = Some(rgba);
                                    }
                                }
                            }
                            Ok(None) => {
                                self.eof_reached = true;
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    if self.eof_reached && !self.video_eof_sent {
                        let _ = vdec.send_eof();
                        self.video_eof_sent = true;
                        while let Ok(Some(rgba)) =
                            vdec.receive_frame_rgba_pooled(&self.rgba_pool, 0)
                        {
                            last_drained_frame = Some(rgba);
                        }
                    }
                    first_video_frame = last_drained_frame;
                }
            }
        }

        let actual_landed_pts = if let Some(ref f) = first_video_frame {
            f.pts_ms
        } else {
            target_ms
        };

        // Composite active bitmap subtitle onto the landed video frame before returning
        if let Some(ref mut frame) = first_video_frame {
            let pts_ms = frame.pts_ms;
            let active_sub = self
                .selected_subtitle_track_index
                .and_then(|idx| self.available_subtitle_tracks.get(idx));
            composite_active_bitmap_subtitle(frame, active_sub, pts_ms, subtitle_delay_ms);
        }

        // Preroll independent audio demuxer (bounded <= 64 audio packets) aligned to landed PTS
        if let (Some(ref mut ad), Some(audio_id), Some(ref mut adec)) = (
            self.audio_demuxer.as_mut(),
            self.audio_track_id,
            self.audio_decoder.as_mut(),
        ) {
            let mut audio_packets = 0usize;
            while audio_packets < 64 {
                match ad.read_packet() {
                    Ok(Some(packet)) => {
                        if packet.track_id == audio_id {
                            audio_packets += 1;
                            self.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                            let dur = packet.duration_ms.unwrap_or(25);
                            let delayed_pts = (packet.pts_ms as i64 + audio_delay_ms).max(0) as u64;
                            if delayed_pts + dur >= actual_landed_pts {
                                if let Ok(pcm_frames) =
                                    adec.decode_packet_us(&packet.data, packet.pts_us)
                                {
                                    if Self::trim_and_send_audio_frames(
                                        pcm_frames,
                                        actual_landed_pts,
                                        audio_delay_ms,
                                        audio_tx,
                                        audio_generation,
                                    ) {
                                        first_audio_sent = true;
                                        if delayed_pts > actual_landed_pts.saturating_add(80) {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        self.audio_eof_reached = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            let _ = first_audio_sent;
        }

        (actual_landed_pts, first_video_frame)
    }

    /// Performs non-disruptive audio track catch-up preroll without flushing video or resetting clock.
    ///
    /// When the dedicated `audio_demuxer` is present, seeks ONLY `audio_demuxer` in <1ms without
    /// touching the primary demuxer, video decoder, or `pending_video_packets`. Falls back to
    /// single-demuxer catch-up when `audio_demuxer` is absent.
    fn switch_audio_track_preroll(
        &mut self,
        current_pts: u64,
        audio_tx: &Sender<AudioWorkerCommand>,
        audio_generation: u64,
        audio_delay_ms: i64,
    ) {
        if let (Some(ref mut ad), Some(audio_id), Some(ref mut adec)) = (
            self.audio_demuxer.as_mut(),
            self.audio_track_id,
            self.audio_decoder.as_mut(),
        ) {
            let _ = ad.seek_to(current_pts);
            self.audio_eof_reached = false;
            self.last_demuxed_audio_pts_ms = Some(current_pts);

            let mut packets_read = 0usize;
            while packets_read < 64 {
                match ad.read_packet() {
                    Ok(Some(packet)) => {
                        if packet.track_id != audio_id {
                            continue;
                        }
                        packets_read += 1;
                        self.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                        let dur = packet.duration_ms.unwrap_or(25);
                        let delayed_pts = (packet.pts_ms as i64 + audio_delay_ms).max(0) as u64;
                        if delayed_pts + dur >= current_pts {
                            if let Ok(pcm_frames) =
                                adec.decode_packet_us(&packet.data, packet.pts_us)
                            {
                                Self::trim_and_send_audio_frames(
                                    pcm_frames,
                                    current_pts,
                                    audio_delay_ms,
                                    audio_tx,
                                    audio_generation,
                                );
                            }
                            if delayed_pts > current_pts.saturating_add(120) {
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        self.audio_eof_reached = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            return;
        }

        let catchup_dts = self.last_demuxed_video_dts;
        let _ = self.demuxer.seek_to(current_pts);
        self.eof_reached = false;

        let max_packets = 5000;
        let mut packets_read = 0;

        while packets_read < max_packets {
            match self.demuxer.read_packet() {
                Ok(Some(packet)) => {
                    packets_read += 1;

                    if Some(packet.track_id) == self.audio_track_id {
                        let dur = packet.duration_ms.unwrap_or(25);
                        let delayed_pts = (packet.pts_ms as i64 + audio_delay_ms).max(0) as u64;
                        if delayed_pts + dur >= current_pts {
                            self.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                            if let Some(ref mut adec) = self.audio_decoder {
                                if let Ok(pcm_frames) =
                                    adec.decode_packet_us(&packet.data, packet.pts_us)
                                {
                                    Self::trim_and_send_audio_frames(
                                        pcm_frames,
                                        current_pts,
                                        audio_delay_ms,
                                        audio_tx,
                                        audio_generation,
                                    );
                                }
                            }
                        }
                    } else if Some(packet.track_id) == self.video_track_id {
                        let pkt_dts = packet.dts_ms.unwrap_or(packet.pts_ms);
                        if let Some(target_dts) = catchup_dts {
                            if pkt_dts > target_dts {
                                self.last_demuxed_video_dts = Some(pkt_dts);
                                self.pending_video_packets.push_back(packet);
                                break;
                            }
                        } else if packet.pts_ms >= current_pts {
                            self.last_demuxed_video_dts = Some(pkt_dts);
                            self.pending_video_packets.push_back(packet);
                            break;
                        }
                    }

                    if self.video_track_id.is_none() && packet.pts_ms >= current_pts + 500 {
                        break;
                    }
                }
                Ok(None) => {
                    self.eof_reached = true;
                    break;
                }
                Err(_) => break,
            }
        }
    }
}

/// Stamps `frame` with `generation` and attempts lock-free push into `video_ring`,
/// saving into `overflow_frame` if the ring is full. Never calls `try_pop()`.
#[inline]
fn push_video_frame_stamped(
    mut frame: VideoFrameRGBA,
    generation: u64,
    video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>,
    overflow_frame: &mut Option<VideoFrameRGBA>,
) -> bool {
    frame.set_generation(generation);
    if let Err(err) = video_ring.try_push(frame) {
        *overflow_frame = Some(err.item);
        false
    } else {
        true
    }
}

/// Attempts non-blocking packet submission (`try_send_packet_with_dts`) with `EAGAIN` drain/retry,
/// compositing active bitmap subtitles onto decoded RGBA frames before pushing to `video_ring`.
#[allow(clippy::too_many_arguments)]
fn feed_video_packet_with_backpressure(
    packet: DemuxPacket,
    vdec: &mut UniversalVideoDecoder,
    rgba_pool: &RgbaFramePool,
    unsent_video_packet: &mut Option<DemuxPacket>,
    active_subtitle_track: Option<&CommonSubtitleTrack>,
    subtitle_delay_ms: i64,
    generation: u64,
    video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>,
    overflow_frame: &mut Option<VideoFrameRGBA>,
) {
    let drain_ready = |vdec: &mut UniversalVideoDecoder,
                       overflow_frame: &mut Option<VideoFrameRGBA>| {
        while overflow_frame.is_none() {
            match vdec.receive_frame_rgba_pooled(rgba_pool, generation) {
                Ok(Some(mut rgba)) => {
                    let pts_ms = rgba.pts_ms;
                    composite_active_bitmap_subtitle(
                        &mut rgba,
                        active_subtitle_track,
                        pts_ms,
                        subtitle_delay_ms,
                    );
                    if !push_video_frame_stamped(rgba, generation, video_ring, overflow_frame) {
                        break;
                    }
                }
                _ => break,
            }
        }
    };

    match vdec.try_send_packet_with_dts(
        &packet.data,
        packet.pts_ms,
        packet.dts_ms,
        packet.is_keyframe,
    ) {
        Ok(true) => {
            drain_ready(vdec, overflow_frame);
        }
        Ok(false) => {
            // Decoder input queue full (EAGAIN): drain decoded output frames and retry once
            drain_ready(vdec, overflow_frame);
            if overflow_frame.is_some() {
                *unsent_video_packet = Some(packet);
                return;
            }
            match vdec.try_send_packet_with_dts(
                &packet.data,
                packet.pts_ms,
                packet.dts_ms,
                packet.is_keyframe,
            ) {
                Ok(true) => {
                    drain_ready(vdec, overflow_frame);
                }
                Ok(false) => {
                    *unsent_video_packet = Some(packet);
                }
                Err(_) => {}
            }
        }
        Err(_) => {}
    }
}

/// Synchronizes session seek preroll, `MasterClock` epoch update, stamped first frame push,
/// and audio worker reset/pause state without ever calling `video_ring.try_pop()`.
#[allow(clippy::too_many_arguments)]
fn perform_session_seek(
    sess: &mut PlaybackSession,
    target_ms: u64,
    was_playing: bool,
    clock: &MasterClock,
    video_ring: &SpscRingBuffer<VideoFrameRGBA, 32>,
    overflow_frame: &mut Option<VideoFrameRGBA>,
    audio_tx: &Sender<AudioWorkerCommand>,
    current_audio_generation: &mut u64,
    audio_delay_ms: i64,
    subtitle_delay_ms: i64,
    event_tx: &Sender<UiEvent>,
) -> u64 {
    *overflow_frame = None;
    let now = Instant::now();
    clock.set_state(now, PlaybackState::Seeking);

    *current_audio_generation = current_audio_generation.wrapping_add(1);
    let _ = audio_tx.send(AudioWorkerCommand::Reset);

    let (landed_pts, first_frame) = sess.seek_preroll(
        target_ms,
        audio_tx,
        *current_audio_generation,
        audio_delay_ms,
        subtitle_delay_ms,
        Some(event_tx),
    );

    let resume_now = Instant::now();
    clock.seek_to(landed_pts as f64, resume_now);
    let current_gen = clock.seek_generation();
    if let Some(frame) = first_frame {
        let _ = push_video_frame_stamped(frame, current_gen, video_ring, overflow_frame);
    }

    if was_playing {
        clock.set_state(resume_now, PlaybackState::Playing);
        let _ = audio_tx.send(AudioWorkerCommand::Resume);
    } else {
        clock.set_state(resume_now, PlaybackState::Paused);
        let _ = audio_tx.send(AudioWorkerCommand::Pause);
    }

    landed_pts
}

/// Runs the background playback worker loop in a dedicated OS thread.
pub fn run_playback_worker(
    command_rx: Receiver<PlayerCommand>,
    audio_tx: Sender<AudioWorkerCommand>,
    event_tx: Sender<UiEvent>,
    exchange: Arc<TripleBufferSnapshotExchange>,
    video_ring: Arc<SpscRingBuffer<VideoFrameRGBA, 32>>,
    clock: Arc<MasterClock>,
    running: Arc<AtomicBool>,
) {
    let mut session: Option<PlaybackSession> = None;
    let mut current_file_name: Option<String> = None;
    let mut current_file_size: u64 = 0;
    let mut current_media_hash: [u8; 32] = [0u8; 32];
    let mut local_ready: bool = false;
    let mut force_play: bool = false;
    let mut gate_blocked: bool = false;
    let mut volume: f32 = 1.0;
    let mut is_muted: bool = false;
    let mut is_loop_enabled: bool = false;
    let mut audio_delay_ms: i64 = 0;
    let mut subtitle_delay_ms: i64 = 0;
    let mut speed: f64 = 1.0;
    let mut overflow_frame: Option<VideoFrameRGBA> = None;
    let mut last_snapshot_instant = Instant::now();
    let mut party_role = WatchPartyRole::None;
    let mut current_audio_generation: u64 = 0;

    while running.load(Ordering::Relaxed) {
        // 1. Drain all pending UI commands, coalescing consecutive Seek commands
        let mut command_batch = Vec::new();
        while let Ok(cmd) = command_rx.try_recv() {
            command_batch.push(cmd);
        }
        let last_seek_index = command_batch
            .iter()
            .rposition(|cmd| matches!(cmd, PlayerCommand::Seek(_)));

        for (cmd_idx, cmd) in command_batch.into_iter().enumerate() {
            if matches!(cmd, PlayerCommand::Seek(_)) && Some(cmd_idx) != last_seek_index {
                continue;
            }
            match cmd {
                PlayerCommand::OpenFile(path) => {
                    current_audio_generation = current_audio_generation.wrapping_add(1);
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
                    let _ = audio_tx.send(AudioWorkerCommand::SetVolume(volume));
                    let _ = audio_tx.send(AudioWorkerCommand::SetMuted(is_muted));
                    let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                    let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));
                    match PlaybackSession::open(&path) {
                        Ok(new_session) => {
                            let dur_ms = new_session.duration_ms;
                            let filename = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("media")
                                .to_string();
                            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            current_file_name = Some(filename.clone());
                            current_file_size = file_size;
                            current_media_hash = MediaFingerprint::compute(&path, dur_ms)
                                .map(|fp| fp.hash)
                                .unwrap_or([0u8; 32]);
                            local_ready = true;
                            gate_blocked = false;

                            // Broadcast discovered audio and subtitle tracks to UI presentation thread
                            let audio_tracks_info: Vec<String> = new_session
                                .available_audio_tracks
                                .iter()
                                .map(format_track_info)
                                .collect();
                            let subtitle_tracks_info: Vec<String> = new_session
                                .available_subtitle_tracks
                                .iter()
                                .map(|t| t.display_label())
                                .collect();

                            let ui_subtitle_tracks =
                                to_ui_subtitle_tracks(&new_session.available_subtitle_tracks);

                            let audio_items: Vec<syncverse_ui::gui::dialogs::AudioTrackItem> = new_session
                                .available_audio_tracks
                                .iter()
                                .map(|t| syncverse_ui::gui::dialogs::AudioTrackItem {
                                    id: t.track_id,
                                    title: t.title.clone().unwrap_or_else(|| format!("Track {}", t.track_id)),
                                    language: t.language.clone(),
                                    codec: t.codec_id.clone(),
                                    channels: t.channels,
                                    sample_rate: t.sample_rate,
                                })
                                .collect();

                            let _ = event_tx.send(UiEvent::TracksDiscovered {
                                audio_tracks: audio_tracks_info,
                                subtitle_tracks: subtitle_tracks_info,
                            });
                            let _ = event_tx.send(UiEvent::AudioDataDiscovered(audio_items));
                            let _ = event_tx.send(UiEvent::VideoMetadataDiscovered(new_session.video_metadata.clone()));
                            let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_subtitle_tracks));
                            if let Some(sub_idx) = new_session.selected_subtitle_track_index {
                                let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
                                    track_index: Some(sub_idx),
                                    name: new_session.available_subtitle_tracks[sub_idx].display_label(),
                                });
                            }
                            let _ = event_tx.send(UiEvent::ChaptersDiscovered(new_session.chapters.clone()));
                            if let Some(at) = new_session.available_audio_tracks.first() {
                                let _ = event_tx.send(UiEvent::AudioTrackSelected {
                                    track_id: at.track_id,
                                    name: format_track_info(at),
                                });
                            }

                            session = Some(new_session);
                            overflow_frame = None;
                            current_audio_generation = current_audio_generation.wrapping_add(1);
                            let _ = audio_tx.send(AudioWorkerCommand::Reset);

                            let now = Instant::now();
                            clock.seek_to(0.0, now);
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);

                            if let WatchPartyRole::Host { server, .. } = &mut party_role {
                                server.set_media_state(0, speed, false);
                                let _ = server.broadcast_room_state();
                            }

                            // Broadcast peer status with loaded media and readiness
                            broadcast_local_peer_status(
                                true,
                                local_ready,
                                dur_ms,
                                file_size,
                                &filename,
                                current_media_hash,
                                &mut party_role,
                            );

                            // Broadcast room activity chat announcement: RoomAnnouncement::media_loaded
                            let sender = get_sender_name(&party_role);
                            let announcement =
                                RoomAnnouncement::media_loaded_ms(&sender, &filename, dur_ms);
                            broadcast_chat_announcement(
                                &announcement,
                                now,
                                &clock,
                                &mut party_role,
                                &event_tx,
                            );

                            // Initial snapshot publication
                            let snap = PlayerUiSnapshot {
                                monotonic_timestamp: 0,
                                media_position_pts: 0,
                                media_duration_pts: dur_ms * 1_000_000,
                                playback_state: 1 | SNAPSHOT_FLAG_LOCAL_READY,
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
                            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("media");
                            session = None;
                            current_file_name = None;
                            current_file_size = 0;
                            current_media_hash = [0u8; 32];
                            clock.set_state(Instant::now(), PlaybackState::Paused);
                            clock.seek_to(0.0, Instant::now());
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                            let snap = PlayerUiSnapshot {
                                monotonic_timestamp: 0,
                                media_position_pts: 0,
                                media_duration_pts: 0,
                                playback_state: 0,
                                ..Default::default()
                            };
                            exchange.publish(snap);
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Error opening {}: {}", fname, err),
                                timestamp: Instant::now(),
                            });
                        }
                    }
                }
                PlayerCommand::SetReady(ready) => {
                    local_ready = ready;
                    let now = Instant::now();
                    let dur_ms = session.as_ref().map(|s| s.duration_ms).unwrap_or(0);
                    broadcast_local_peer_status(
                        session.is_some(),
                        local_ready,
                        dur_ms,
                        current_file_size,
                        current_file_name.as_deref().unwrap_or(""),
                        current_media_hash,
                        &mut party_role,
                    );

                    let sender = get_sender_name(&party_role);
                    let msg = RoomAnnouncement::readiness(&sender, ready);
                    broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                }
                PlayerCommand::SetForcePlay(val) => {
                    force_play = val;
                    if force_play {
                        gate_blocked = false;
                    }
                }
                PlayerCommand::ForcePlay => {
                    force_play = true;
                    gate_blocked = false;
                    let now = Instant::now();
                    let current_pts = clock.current_pts(now).max(0.0) as u64;

                    let sender = get_sender_name(&party_role);
                    match &mut party_role {
                        WatchPartyRole::Host {
                            server,
                            pending_commit_unpause_ms,
                            barrier_committed,
                            ..
                        } => {
                            if server.clients().is_empty() {
                                clock.set_state(now, PlaybackState::Playing);
                                let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                server.set_media_state(current_pts, speed, false);
                                let _ = server.broadcast_room_state();
                            } else {
                                clock.set_state(now, PlaybackState::Paused);
                                let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                server.set_media_state(current_pts, speed, true);
                                *pending_commit_unpause_ms =
                                    server.prepare_barrier_unpause(50).ok();
                                *barrier_committed = false;
                                let _ = server.broadcast_room_state();
                            }
                        }
                        WatchPartyRole::Client { .. } => {
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);
                            send_client_peer_control(
                                &mut party_role,
                                PeerControlAction::Play,
                                current_pts,
                                speed,
                            );
                        }
                        WatchPartyRole::None => {
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);
                        }
                    }

                    let msg = RoomAnnouncement::resumed(&sender);
                    broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                }
                PlayerCommand::TogglePause => {
                    let now = Instant::now();
                    let will_play = !clock.is_playing();
                    let sender = get_sender_name(&party_role);

                    if will_play {
                        let current_pts = clock.current_pts(now);
                        let dur = session.as_ref().map(|s| s.duration_ms).unwrap_or(0);
                        if dur > 0 && current_pts >= dur as f64 {
                            if let Some(ref mut sess) = session {
                                perform_session_seek(
                                    sess,
                                    0,
                                    false,
                                    &clock,
                                    &video_ring,
                                    &mut overflow_frame,
                                    &audio_tx,
                                    &mut current_audio_generation,
                                    audio_delay_ms,
                                    subtitle_delay_ms,
                                    &event_tx,
                                );
                            } else {
                                overflow_frame = None;
                                current_audio_generation = current_audio_generation.wrapping_add(1);
                                let _ = audio_tx.send(AudioWorkerCommand::Reset);
                                clock.seek_to(0.0, now);
                            }
                        }

                        // Unpause trigger: evaluate unpause gate across room peers
                        let mut unready_peers = Vec::new();
                        let in_watch_party = !matches!(party_role, WatchPartyRole::None);

                        if in_watch_party {
                            // Check local readiness
                            if session.is_none() || !local_ready {
                                unready_peers.push(sender.clone());
                            }

                            // Check remote peers
                            match &party_role {
                                WatchPartyRole::Host {
                                    server,
                                    client_names,
                                    ..
                                } => {
                                    for (addr, client) in server.clients() {
                                        if client.last_seen.elapsed() < Duration::from_secs(15)
                                            && (!client.has_media || !client.is_user_ready)
                                        {
                                            let name = if !client.username.is_empty() {
                                                client.username.clone()
                                            } else {
                                                client_names
                                                    .get(addr)
                                                    .cloned()
                                                    .unwrap_or_else(|| {
                                                        format!("Peer-{}", client.client_id)
                                                    })
                                            };
                                            unready_peers.push(name);
                                        }
                                    }
                                }
                                WatchPartyRole::Client {
                                    client,
                                    host_username,
                                    ..
                                } => {
                                    if client.peer_statuses().is_empty() {
                                        unready_peers.push(host_username.clone());
                                    } else {
                                        for status in client.peer_statuses().values() {
                                            if !status.has_media || !status.is_ready {
                                                unready_peers.push(status.username.clone());
                                            }
                                        }
                                    }
                                }
                                WatchPartyRole::None => {}
                            }
                        }

                        if !unready_peers.is_empty() && !force_play {
                            // Unpause blocked: hold playback paused and alert room
                            clock.set_state(now, PlaybackState::Paused);
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                            gate_blocked = true;

                            let msg = RoomAnnouncement::gate_waiting(&unready_peers);
                            let _ = event_tx.send(UiEvent::UnpauseBlocked {
                                unready_peers: unready_peers.clone(),
                                message: msg.clone(),
                            });

                            broadcast_chat_announcement(
                                &msg,
                                now,
                                &clock,
                                &mut party_role,
                                &event_tx,
                            );
                        } else {
                            // Unpause allowed or force_play override active
                            gate_blocked = false;
                            let current_pts_u64 = clock.current_pts(now).max(0.0) as u64;

                            match &mut party_role {
                                WatchPartyRole::Host {
                                    server,
                                    pending_commit_unpause_ms,
                                    barrier_committed,
                                    ..
                                } => {
                                    if server.clients().is_empty() {
                                        clock.set_state(now, PlaybackState::Playing);
                                        let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                        server.set_media_state(current_pts_u64, speed, false);
                                        let _ = server.broadcast_room_state();
                                    } else {
                                        clock.set_state(now, PlaybackState::Paused);
                                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                        server.set_media_state(current_pts_u64, speed, true);
                                        *pending_commit_unpause_ms =
                                            server.prepare_barrier_unpause(50).ok();
                                        *barrier_committed = false;
                                        let _ = server.broadcast_room_state();
                                    }
                                }
                                WatchPartyRole::Client { .. } => {
                                    // Delegate room unpause to host so two-phase barrier synchronizes start
                                    send_client_peer_control(
                                        &mut party_role,
                                        PeerControlAction::Play,
                                        current_pts_u64,
                                        speed,
                                    );
                                }
                                WatchPartyRole::None => {
                                    clock.set_state(now, PlaybackState::Playing);
                                    let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                }
                            }

                            let msg = RoomAnnouncement::resumed(&sender);
                            broadcast_chat_announcement(
                                &msg,
                                now,
                                &clock,
                                &mut party_role,
                                &event_tx,
                            );
                        }
                    } else {
                        // Pause trigger
                        gate_blocked = false;
                        clock.set_state(now, PlaybackState::Paused);
                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                        let current_pts = clock.current_pts(now).max(0.0) as u64;

                        match &mut party_role {
                            WatchPartyRole::Host {
                                server,
                                pending_commit_unpause_ms,
                                barrier_committed,
                                ..
                            } => {
                                server.reset_barrier();
                                *pending_commit_unpause_ms = None;
                                *barrier_committed = false;
                                server.set_media_state(current_pts, speed, true);
                                let _ = server.broadcast_room_state();
                            }
                            WatchPartyRole::Client { .. } => {
                                send_client_peer_control(
                                    &mut party_role,
                                    PeerControlAction::Pause,
                                    current_pts,
                                    speed,
                                );
                            }
                            WatchPartyRole::None => {}
                        }

                        let msg = RoomAnnouncement::paused(&sender);
                        broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                    }
                }
                PlayerCommand::Stop => {
                    let now = Instant::now();
                    gate_blocked = false;
                    if let Some(ref mut sess) = session {
                        perform_session_seek(
                            sess,
                            0,
                            false,
                            &clock,
                            &video_ring,
                            &mut overflow_frame,
                            &audio_tx,
                            &mut current_audio_generation,
                            audio_delay_ms,
                            subtitle_delay_ms,
                            &event_tx,
                        );
                    } else {
                        clock.seek_to(0.0, now);
                        clock.set_state(now, PlaybackState::Paused);
                        overflow_frame = None;
                        current_audio_generation = current_audio_generation.wrapping_add(1);
                        let _ = audio_tx.send(AudioWorkerCommand::Reset);
                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                    }
                    if let WatchPartyRole::Host {
                        server,
                        pending_commit_unpause_ms,
                        barrier_committed,
                        ..
                    } = &mut party_role
                    {
                        server.reset_barrier();
                        *pending_commit_unpause_ms = None;
                        *barrier_committed = false;
                        server.set_media_state(0, speed, true);
                        let _ = server.broadcast_room_state();
                    }
                }
                PlayerCommand::Seek(target_ms) => {
                    let was_playing = clock.is_playing();
                    let landed_pts = if let Some(ref mut sess) = session {
                        perform_session_seek(
                            sess,
                            target_ms,
                            was_playing,
                            &clock,
                            &video_ring,
                            &mut overflow_frame,
                            &audio_tx,
                            &mut current_audio_generation,
                            audio_delay_ms,
                            subtitle_delay_ms,
                            &event_tx,
                        )
                    } else {
                        let now = Instant::now();
                        overflow_frame = None;
                        current_audio_generation = current_audio_generation.wrapping_add(1);
                        let _ = audio_tx.send(AudioWorkerCommand::Reset);
                        clock.seek_to(target_ms as f64, now);
                        if was_playing {
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);
                        } else {
                            clock.set_state(now, PlaybackState::Paused);
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                        }
                        target_ms
                    };
                    let resume_now = Instant::now();

                    match &mut party_role {
                        WatchPartyRole::Host { server, .. } => {
                            server.set_media_state(landed_pts, speed, !was_playing);
                            let _ = server.broadcast_room_state();
                        }
                        WatchPartyRole::Client { .. } => {
                            send_client_peer_control(
                                &mut party_role,
                                PeerControlAction::Seek,
                                landed_pts,
                                speed,
                            );
                        }
                        WatchPartyRole::None => {}
                    }
                    let sender = get_sender_name(&party_role);
                    let msg = RoomAnnouncement::seeked_ms(&sender, landed_pts);
                    broadcast_chat_announcement(&msg, resume_now, &clock, &mut party_role, &event_tx);
                }
                PlayerCommand::SetSpeed(spd) => {
                    speed = spd.clamp(0.2, 4.0);
                    let now = Instant::now();
                    clock.reanchor(now, speed);
                    let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                    let current_pts = clock.current_pts(now).max(0.0) as u64;
                    match &mut party_role {
                        WatchPartyRole::Host { server, .. } => {
                            server.set_media_state(current_pts, speed, !clock.is_playing());
                            let _ = server.broadcast_room_state();
                        }
                        WatchPartyRole::Client { .. } => {
                            send_client_peer_control(
                                &mut party_role,
                                PeerControlAction::SetSpeed,
                                current_pts,
                                speed,
                            );
                        }
                        WatchPartyRole::None => {}
                    }
                }
                PlayerCommand::SetVolume(vol) => {
                    volume = vol.clamp(0.0, 2.0);
                    if volume > 0.0 && is_muted {
                        is_muted = false;
                        let _ = audio_tx.send(AudioWorkerCommand::SetMuted(false));
                    }
                    let _ = audio_tx.send(AudioWorkerCommand::SetVolume(volume));
                    let boost_suffix = if volume > 1.0 { " [Boost]" } else { "" };
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Volume: {}%{}", (volume * 100.0).round() as u32, boost_suffix),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::ToggleMute => {
                    is_muted = !is_muted;
                    let _ = audio_tx.send(AudioWorkerCommand::SetMuted(is_muted));
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: if is_muted {
                            "Muted".to_string()
                        } else {
                            "Unmuted".to_string()
                        },
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::SelectAudioTrack(track_id) => {
                    if let Some(ref mut sess) = session {
                        match sess.select_audio_track(track_id) {
                            Ok(name) => {
                                let now = Instant::now();
                                let current_pts = clock.current_pts(now).max(0.0) as u64;

                                current_audio_generation = current_audio_generation.wrapping_add(1);
                                let _ = audio_tx.send(AudioWorkerCommand::Reset);
                                let _ = audio_tx.send(AudioWorkerCommand::SetVolume(volume));
                                let _ = audio_tx.send(AudioWorkerCommand::SetMuted(is_muted));
                                let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                                let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));

                                sess.switch_audio_track_preroll(
                                    current_pts,
                                    &audio_tx,
                                    current_audio_generation,
                                    audio_delay_ms,
                                );

                                if clock.is_playing() {
                                    let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                } else {
                                    let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                }

                                let _ = event_tx.send(UiEvent::AudioTrackSelected {
                                    track_id,
                                    name: name.clone(),
                                });
                                let _ = event_tx.send(UiEvent::HudNotification {
                                    text: format!("Audio: {name}"),
                                    timestamp: Instant::now(),
                                });
                            }
                            Err(e) => {
                                eprintln!("[WARN] SelectAudioTrack({track_id}) failed: {e}");
                            }
                        }
                    }
                }
                PlayerCommand::CycleAudioTrack => {
                    if let Some(ref mut sess) = session {
                        if let Some((next_id, name)) = sess.cycle_audio_track() {
                            let now = Instant::now();
                            let current_pts = clock.current_pts(now).max(0.0) as u64;

                            current_audio_generation = current_audio_generation.wrapping_add(1);
                            let _ = audio_tx.send(AudioWorkerCommand::Reset);
                            let _ = audio_tx.send(AudioWorkerCommand::SetVolume(volume));
                            let _ = audio_tx.send(AudioWorkerCommand::SetMuted(is_muted));
                            let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                            let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));

                            sess.switch_audio_track_preroll(
                                current_pts,
                                &audio_tx,
                                current_audio_generation,
                                audio_delay_ms,
                            );

                            if clock.is_playing() {
                                let _ = audio_tx.send(AudioWorkerCommand::Resume);
                            } else {
                                let _ = audio_tx.send(AudioWorkerCommand::Pause);
                            }

                            let _ = event_tx.send(UiEvent::AudioTrackSelected {
                                track_id: next_id,
                                name: name.clone(),
                            });
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Audio: {name}"),
                                timestamp: Instant::now(),
                            });
                        }
                    }
                }
                PlayerCommand::SetAudioDelay(delay_ms) => {
                    audio_delay_ms = delay_ms.clamp(-5000, 5000);
                    let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));
                    let _ = event_tx.send(UiEvent::AudioDelayChanged(audio_delay_ms));
                }
                PlayerCommand::AdjustAudioDelay(delta_ms) => {
                    audio_delay_ms = (audio_delay_ms + delta_ms).clamp(-5000, 5000);
                    let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));
                    let _ = event_tx.send(UiEvent::AudioDelayChanged(audio_delay_ms));
                }
                PlayerCommand::SelectSubtitleTrack(track_index) => {
                    if let Some(ref mut sess) = session {
                        let (idx, name) = sess.select_subtitle_track(track_index);
                        let ui_tracks = to_ui_subtitle_tracks(&sess.available_subtitle_tracks);
                        let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_tracks));
                        let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
                            track_index: idx,
                            name: name.clone(),
                        });
                        let notif = if idx.is_some() {
                            format!("Subtitles: {name}")
                        } else {
                            "Subtitles: Off".to_string()
                        };
                        let _ = event_tx.send(UiEvent::HudNotification {
                            text: notif,
                            timestamp: Instant::now(),
                        });
                    }
                }
                PlayerCommand::CycleSubtitleTrack => {
                    if let Some(ref mut sess) = session {
                        let (next_idx, name) = sess.cycle_subtitle_track();
                        let ui_tracks = to_ui_subtitle_tracks(&sess.available_subtitle_tracks);
                        let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_tracks));
                        let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
                            track_index: next_idx,
                            name: name.clone(),
                        });
                        let notif = match next_idx {
                            Some(idx) => format!(
                                "Subtitles: [{}/{}] {}",
                                idx + 1,
                                sess.available_subtitle_tracks.len(),
                                name
                            ),
                            None => "Subtitles: Off".to_string(),
                        };
                        let _ = event_tx.send(UiEvent::HudNotification {
                            text: notif,
                            timestamp: Instant::now(),
                        });
                    }
                }
                PlayerCommand::LoadExternalSubtitle(path) => {
                    if let Some(ref mut sess) = session {
                        let next_track_id = 1000 + sess.available_subtitle_tracks.len();
                        let filename = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("External Subtitle")
                            .to_string();
                        let ext = path
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("srt")
                            .to_string();
                        let track = syncverse_demux::subtitles::parse_subtitle_file_with_fps(
                            &path,
                            next_track_id,
                            Some(&filename),
                            sess.video_fps,
                        )
                        .unwrap_or_else(|_| {
                            CommonSubtitleTrack::new(
                                next_track_id,
                                SubtitleSource::External,
                                ext,
                                None,
                                filename.clone(),
                                Vec::new(),
                            )
                        });
                        let track_idx = sess.available_subtitle_tracks.len();
                        sess.available_subtitle_tracks.push(track);
                        let (selected_idx, name) = sess.select_subtitle_track(Some(track_idx));
                        let ui_tracks = to_ui_subtitle_tracks(&sess.available_subtitle_tracks);
                        let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_tracks));
                        let _ = event_tx.send(UiEvent::SubtitleTrackSelected {
                            track_index: selected_idx,
                            name: name.clone(),
                        });
                        let _ = event_tx.send(UiEvent::HudNotification {
                            text: format!("Subtitles: {name}"),
                            timestamp: Instant::now(),
                        });
                    }
                }
                PlayerCommand::SetSubtitleDelay(delay_ms) => {
                    subtitle_delay_ms = delay_ms.clamp(-5000, 5000);
                    let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
                }
                PlayerCommand::AdjustSubtitleDelay(delta_ms) => {
                    subtitle_delay_ms = (subtitle_delay_ms + delta_ms).clamp(-5000, 5000);
                    let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
                }
                PlayerCommand::CycleAspectRatio
                | PlayerCommand::NextChapter
                | PlayerCommand::PreviousChapter
                | PlayerCommand::TakeScreenshot => {
                    // Handled by UI presentation layer / video viewport / MPRIS
                }
                PlayerCommand::SendChatMessage(text) => {
                    let now = Instant::now();
                    let wall_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let media_ms = clock.current_pts(now).max(0.0) as u64;

                    let sender_name = match &party_role {
                        WatchPartyRole::Host { local_username, .. } => local_username.clone(),
                        WatchPartyRole::Client { local_username, .. } => local_username.clone(),
                        WatchPartyRole::None => std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
                    };

                    let rec = ChatRecord::new_text(wall_ms, media_ms, [0u8; 16], sender_name, &text);
                    let wire_bytes = rec.to_network_bytes();

                    match &mut party_role {
                        WatchPartyRole::Host { server, .. } => {
                            let _ = server.broadcast_chat_message(&wire_bytes);
                        }
                        WatchPartyRole::Client { client, .. } => {
                            let _ = client.send_chat_message_with_pts(&wire_bytes, media_ms);
                        }
                        WatchPartyRole::None => {}
                    }

                    let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                }
                PlayerCommand::ExecuteSlashCommand(text) => {
                    let now = Instant::now();
                    let wall_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let media_ms = clock.current_pts(now).max(0.0) as u64;

                    match SlashCommandParser::parse(&text, Some(media_ms)) {
                        Ok(ParsedSlashCommand::Seek { target_pts_ms }) => {
                            let duration = session.as_ref().map(|s| s.duration_ms).unwrap_or(u64::MAX);
                            let bounded_target = target_pts_ms.min(duration);
                            let was_playing = clock.is_playing();
                            let landed_pts = if let Some(ref mut sess) = session {
                                perform_session_seek(
                                    sess,
                                    bounded_target,
                                    was_playing,
                                    &clock,
                                    &video_ring,
                                    &mut overflow_frame,
                                    &audio_tx,
                                    &mut current_audio_generation,
                                    audio_delay_ms,
                                    subtitle_delay_ms,
                                    &event_tx,
                                )
                            } else {
                                overflow_frame = None;
                                current_audio_generation = current_audio_generation.wrapping_add(1);
                                let _ = audio_tx.send(AudioWorkerCommand::Reset);
                                clock.seek_to(bounded_target as f64, now);
                                if was_playing {
                                    clock.set_state(now, PlaybackState::Playing);
                                    let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                } else {
                                    clock.set_state(now, PlaybackState::Paused);
                                    let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                }
                                bounded_target
                            };

                            let resume_now = Instant::now();
                            match &mut party_role {
                                WatchPartyRole::Host { server, .. } => {
                                    server.set_media_state(landed_pts, speed, !was_playing);
                                    let _ = server.broadcast_room_state();
                                }
                                WatchPartyRole::Client { .. } => {
                                    send_client_peer_control(
                                        &mut party_role,
                                        PeerControlAction::Seek,
                                        landed_pts,
                                        speed,
                                    );
                                }
                                WatchPartyRole::None => {}
                            }
                            let sender = get_sender_name(&party_role);
                            let msg = RoomAnnouncement::seeked_ms(&sender, landed_pts);
                            broadcast_chat_announcement(&msg, resume_now, &clock, &mut party_role, &event_tx);
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Seek: {}", format_timecode(landed_pts)),
                                timestamp: resume_now,
                            });
                        }
                        Ok(ParsedSlashCommand::Loop { enabled }) => {
                            is_loop_enabled = enabled.unwrap_or(!is_loop_enabled);
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Repeat: {}", if is_loop_enabled { "On" } else { "Off" }),
                                timestamp: now,
                            });
                        }
                        Ok(ParsedSlashCommand::Mute { enabled }) => {
                            let new_mute = enabled.unwrap_or(!is_muted);
                            if new_mute != is_muted {
                                is_muted = new_mute;
                                let _ = audio_tx.send(AudioWorkerCommand::SetMuted(is_muted));
                                let _ = event_tx.send(UiEvent::HudNotification {
                                    text: if is_muted {
                                        "Muted".to_string()
                                    } else {
                                        format!("Volume: {}%", (volume * 100.0).round() as u32)
                                    },
                                    timestamp: now,
                                });
                            }
                        }
                        Ok(ParsedSlashCommand::Speed { speed_permille }) => {
                            let new_speed = (speed_permille as f64 / 1000.0).clamp(0.2, 4.0);
                            speed = new_speed;
                            clock.reanchor(now, speed);
                            let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                            let current_pts = clock.current_pts(now).max(0.0) as u64;
                            match &mut party_role {
                                WatchPartyRole::Host { server, .. } => {
                                    server.set_media_state(current_pts, speed, !clock.is_playing());
                                    let _ = server.broadcast_room_state();
                                }
                                WatchPartyRole::Client { .. } => {
                                    send_client_peer_control(
                                        &mut party_role,
                                        PeerControlAction::SetSpeed,
                                        current_pts,
                                        speed,
                                    );
                                }
                                WatchPartyRole::None => {}
                            }
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Speed: {:.2}x", speed),
                                timestamp: now,
                            });
                        }
                        Ok(ParsedSlashCommand::AudioDelay { delay_ms }) => {
                            audio_delay_ms = delay_ms.clamp(-5000, 5000);
                            let _ = audio_tx.send(AudioWorkerCommand::SetAudioDelay(audio_delay_ms));
                            let _ = event_tx.send(UiEvent::AudioDelayChanged(audio_delay_ms));
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Audio Delay: {:+} ms", audio_delay_ms),
                                timestamp: now,
                            });
                        }
                        Ok(ParsedSlashCommand::SubtitleDelay { delay_ms }) => {
                            subtitle_delay_ms = delay_ms.clamp(-5000, 5000);
                            let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
                            let _ = event_tx.send(UiEvent::HudNotification {
                                text: format!("Subtitle Delay: {:+} ms", subtitle_delay_ms),
                                timestamp: now,
                            });
                        }
                        Ok(ParsedSlashCommand::Help { command }) => {
                            let help_text = format_help(command.as_deref());
                            let rec = RoomAnnouncement::create_record(wall_ms, media_ms, &help_text);
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                        Ok(ParsedSlashCommand::SyncStatus) => {
                            let (rtt, offset, peers) = match &party_role {
                                WatchPartyRole::Host { server, .. } => {
                                    (server.max_rtt_ms() as f64, 0.0, server.clients().len() + 1)
                                }
                                WatchPartyRole::Client { client, .. } => (
                                    client.median_rtt_nanos() / 1_000_000.0,
                                    client.filtered_offset_nanos().unwrap_or(0.0) / 1_000_000.0,
                                    client.peer_statuses().len() + 1,
                                ),
                                WatchPartyRole::None => (0.0, 0.0, 1),
                            };
                            let diag = SyncDiagnostics {
                                rtt_ms: rtt,
                                clock_offset_ms: offset,
                                speed_multiplier: speed,
                                peer_count: peers,
                                current_pts_ms: media_ms,
                                is_paused: !clock.is_playing(),
                                pll_zone: if offset.abs() <= 15.0 {
                                    "Deadband".to_string()
                                } else if offset.abs() <= 1500.0 {
                                    "MicroSpeed".to_string()
                                } else {
                                    "HardSeek".to_string()
                                },
                            };
                            let report = format_sync_status_report(&diag);
                            let rec = RoomAnnouncement::create_record(wall_ms, media_ms, &report);
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                        Ok(ParsedSlashCommand::Clear) => {
                            // UI clears local view on Clear command
                        }
                        Ok(ParsedSlashCommand::Who) => {
                            let mut members_summary = Vec::new();
                            match &party_role {
                                WatchPartyRole::Host { local_username, server, .. } => {
                                    members_summary.push(syncverse_chat::RoomMemberSummary {
                                        username: local_username.clone(),
                                        is_local: true,
                                        is_host: true,
                                        rtt_ms: 0.0,
                                        is_ready: local_ready,
                                        file_name: current_file_name.clone(),
                                    });
                                    for client_sess in server.clients().values() {
                                        members_summary.push(syncverse_chat::RoomMemberSummary {
                                            username: if !client_sess.username.is_empty() {
                                                client_sess.username.clone()
                                            } else {
                                                format!("Peer-{}", client_sess.client_id)
                                            },
                                            is_local: false,
                                            is_host: false,
                                            rtt_ms: client_sess.rtt_ms as f64,
                                            is_ready: client_sess.is_user_ready,
                                            file_name: if client_sess.has_media {
                                                Some(client_sess.filename.clone())
                                            } else {
                                                None
                                            },
                                        });
                                    }
                                }
                                WatchPartyRole::Client { local_username, client, .. } => {
                                    members_summary.push(syncverse_chat::RoomMemberSummary {
                                        username: local_username.clone(),
                                        is_local: true,
                                        is_host: false,
                                        rtt_ms: client.median_rtt_nanos() / 1_000_000.0,
                                        is_ready: local_ready,
                                        file_name: current_file_name.clone(),
                                    });
                                    for status in client.peer_statuses().values() {
                                        members_summary.push(syncverse_chat::RoomMemberSummary {
                                            username: status.username.clone(),
                                            is_local: false,
                                            is_host: status.client_id == 0,
                                            rtt_ms: 0.0,
                                            is_ready: status.is_ready,
                                            file_name: if status.has_media {
                                                Some(status.filename.clone())
                                            } else {
                                                None
                                            },
                                        });
                                    }
                                }
                                WatchPartyRole::None => {
                                    members_summary.push(syncverse_chat::RoomMemberSummary {
                                        username: std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
                                        is_local: true,
                                        is_host: true,
                                        rtt_ms: 0.0,
                                        is_ready: local_ready,
                                        file_name: current_file_name.clone(),
                                    });
                                }
                            }
                            let report = syncverse_chat::format_who_report(&members_summary);
                            let rec = RoomAnnouncement::create_record(wall_ms, media_ms, &report);
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                        Ok(ParsedSlashCommand::Bookmark { .. })
                        | Ok(ParsedSlashCommand::Clip { .. })
                        | Ok(ParsedSlashCommand::Poll { .. }) => {
                            let sender_name = match &party_role {
                                WatchPartyRole::Host { local_username, .. } => local_username.clone(),
                                WatchPartyRole::Client { local_username, .. } => local_username.clone(),
                                WatchPartyRole::None => std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
                            };
                            let rec = ChatRecord::new_text(wall_ms, media_ms, [0u8; 16], sender_name, &text);
                            let wire_bytes = rec.to_network_bytes();
                            match &mut party_role {
                                WatchPartyRole::Host { server, .. } => {
                                    let _ = server.broadcast_chat_message(&wire_bytes);
                                }
                                WatchPartyRole::Client { client, .. } => {
                                    let _ = client.send_chat_message_with_pts(&wire_bytes, media_ms);
                                }
                                WatchPartyRole::None => {}
                            }
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                        Err(err) => {
                            let err_msg = format!("❌ Command Error: {}", err);
                            let rec = RoomAnnouncement::create_record(wall_ms, media_ms, &err_msg);
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                    }
                }
                PlayerCommand::HostParty {
                    port,
                    room,
                    username,
                    password,
                    enable_upnp,
                } => {
                    #[cfg(target_os = "linux")]
                    let bind_res = HostServer::bind(format!("[::]:{}", port))
                        .or_else(|_| HostServer::bind(format!("0.0.0.0:{}", port)));
                    #[cfg(not(target_os = "linux"))]
                    let bind_res = HostServer::bind(&format!("0.0.0.0:{}", port));

                    match bind_res {
                        Ok(mut server) => {
                            let now = Instant::now();
                            let current_pts = clock.current_pts(now).max(0.0) as u64;
                            let is_paused = !clock.is_playing();

                            let crypto = password
                                .as_deref()
                                .and_then(|pwd| RoomCryptoSession::from_password(&room, pwd).ok())
                                .map(Arc::new);
                            server.set_crypto(crypto.clone());
                            server.set_media_state(current_pts, speed, is_paused);

                            let upnp_rx = if enable_upnp {
                                Some(spawn_upnp_mapping(port))
                            } else {
                                let _ = event_tx.send(UiEvent::UpnpStatusUpdated {
                                    is_mapped: false,
                                    external_ip: None,
                                    port,
                                    message: "UPnP disabled by user".to_string(),
                                });
                                None
                            };

                            let dur_ms = session.as_ref().map(|s| s.duration_ms);
                            server.set_host_peer_status(PeerStatusPayload::with_hash(
                                0,
                                session.is_some(),
                                local_ready,
                                dur_ms.unwrap_or(0),
                                current_file_size,
                                username.clone(),
                                current_file_name.clone().unwrap_or_default(),
                                current_media_hash,
                            ));

                            let _ = event_tx.send(UiEvent::RoomMembersUpdated(vec![RoomMember::with_media(
                                username.clone(),
                                true,
                                true,
                                0.0,
                                current_file_name.clone(),
                                dur_ms,
                                if session.is_some() { Some(current_file_size) } else { None },
                                local_ready,
                            )]));

                            let wall_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);

                            let enc_notice = if crypto.is_some() { " [E2E Encrypted 🔒]" } else { "" };
                            let rec = ChatRecord::new_text(
                                wall_ms,
                                current_pts,
                                [0u8; 16],
                                "System".to_string(),
                                &format!("★ Hosting watch party in room '{}' on port {}{}", room, port, enc_notice),
                            );
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));

                            party_role = WatchPartyRole::Host {
                                server,
                                _room: room,
                                local_username: username,
                                client_names: HashMap::new(),
                                last_state_broadcast: now,
                                last_member_update: now,
                                last_peer_count: 0,
                                last_probe_sent: now,
                                upnp_rx,
                                _upnp_guard: None,
                                upnp_port: port,
                                _crypto: crypto,
                                pending_commit_unpause_ms: None,
                                barrier_committed: false,
                            };

                            broadcast_local_peer_status(
                                session.is_some(),
                                local_ready,
                                dur_ms.unwrap_or(0),
                                current_file_size,
                                current_file_name.as_deref().unwrap_or(""),
                                current_media_hash,
                                &mut party_role,
                            );
                        }
                        Err(e) => {
                            eprintln!("[ERROR] Failed to bind HostServer on port {}: {}", port, e);
                            let wall_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            let rec = ChatRecord::new_text(
                                wall_ms,
                                0,
                                [0u8; 16],
                                "System".to_string(),
                                &format!("❌ Failed to host room on port {}: {}", port, e),
                            );
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                        }
                    }
                }
                PlayerCommand::ConnectParty {
                    addr,
                    room,
                    username,
                    password,
                } => {
                    let target_addr: Option<SocketAddr> = addr
                        .parse::<SocketAddr>()
                        .ok()
                        .or_else(|| addr.to_socket_addrs().ok().and_then(|mut iter| iter.next()));

                    if let Some(target) = target_addr {
                        let client_id = ((SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as u32)
                            .unwrap_or(1))
                            & 0x7FFFFFFF)
                            .max(1);

                        let crypto = password
                            .as_deref()
                            .and_then(|pwd| RoomCryptoSession::from_password(&room, pwd).ok())
                            .map(Arc::new);

                        match ClientNode::bind_for_target(target, client_id) {
                            Ok(mut client) => {
                                client.set_crypto(crypto.clone());

                                let now = Instant::now();
                                let wall_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                let current_pts = clock.current_pts(now).max(0.0) as u64;
                                let dur_ms = session.as_ref().map(|s| s.duration_ms);

                                // Send immediate join handshake: peer status, initial NTP probe, sync snapshot request, and join chat
                                let _ = client.send_peer_status_with_hash(
                                    session.is_some(),
                                    local_ready,
                                    dur_ms.unwrap_or(0),
                                    current_file_size,
                                    &username,
                                    current_file_name.as_deref().unwrap_or(""),
                                    current_media_hash,
                                );
                                let _ = client.send_ntp_probe();
                                let _ = client.request_sync_snapshot();

                                let join_rec = ChatRecord::new_text(
                                    wall_ms,
                                    current_pts,
                                    [0u8; 16],
                                    username.clone(),
                                    &format!("👋 Joined room '{}'", room),
                                );
                                let _ = client.send_chat_message_with_pts(&join_rec.to_network_bytes(), current_pts);

                                let _ = event_tx.send(UiEvent::RoomMembersUpdated(vec![RoomMember::with_media(
                                    username.clone(),
                                    true,
                                    false,
                                    0.0,
                                    current_file_name.clone(),
                                    dur_ms,
                                    if session.is_some() { Some(current_file_size) } else { None },
                                    local_ready,
                                )]));

                                let enc_notice = if crypto.is_some() { " [E2E Encrypted 🔒]" } else { "" };
                                let rec = ChatRecord::new_text(
                                    wall_ms,
                                    current_pts,
                                    [0u8; 16],
                                    "System".to_string(),
                                    &format!("🔗 Connected to {} (Room '{}'{})", addr, room, enc_notice),
                                );
                                let _ = event_tx.send(UiEvent::NewChatRecord(rec));

                                party_role = WatchPartyRole::Client {
                                    client,
                                    _room: room,
                                    local_username: username,
                                    host_username: "Host".to_string(),
                                    last_probe_sent: now,
                                    last_member_update: now,
                                    last_peer_count: 0,
                                    _crypto: crypto,
                                    initial_sync_locked: false,
                                    last_pll_update: now,
                                };
                            }
                            Err(e) => {
                                eprintln!("[ERROR] Failed to bind ClientNode to {}: {}", addr, e);
                                let wall_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                let rec = ChatRecord::new_text(
                                    wall_ms,
                                    0,
                                    [0u8; 16],
                                    "System".to_string(),
                                    &format!("❌ Failed to connect to {}: {}", addr, e),
                                );
                                let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                            }
                        }
                    } else {
                        eprintln!("[ERROR] Invalid address format: {}", addr);
                        let wall_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let rec = ChatRecord::new_text(
                            wall_ms,
                            0,
                            [0u8; 16],
                            "System".to_string(),
                            &format!("❌ Could not resolve address: {}", addr),
                        );
                        let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                    }
                }
                PlayerCommand::SendReaction(emoji) => {
                    let now = Instant::now();
                    let current_pts = clock.current_pts(now).max(0.0) as u64;
                    match &mut party_role {
                        WatchPartyRole::Host { server, local_username, .. } => {
                            let _ = server.broadcast_reaction(&emoji, current_pts, local_username);
                            let _ = event_tx.send(UiEvent::FloatingReaction {
                                emoji,
                                username: local_username.clone(),
                            });
                        }
                        WatchPartyRole::Client { client, local_username, .. } => {
                            let _ = client.send_reaction(&emoji, current_pts, local_username);
                            let _ = event_tx.send(UiEvent::FloatingReaction {
                                emoji,
                                username: local_username.clone(),
                            });
                        }
                        WatchPartyRole::None => {
                            let _ = event_tx.send(UiEvent::FloatingReaction {
                                emoji,
                                username: "You".to_string(),
                            });
                        }
                    }
                }
                PlayerCommand::DisconnectParty => {
                    party_role = WatchPartyRole::None;
                    gate_blocked = false;
                    let local_name = std::env::var("USER").unwrap_or_else(|_| "You".to_string());
                    let dur_ms = session.as_ref().map(|s| s.duration_ms);
                    let _ = event_tx.send(UiEvent::RoomMembersUpdated(vec![RoomMember::with_media(
                        local_name,
                        true,
                        true,
                        0.0,
                        current_file_name.clone(),
                        dur_ms,
                        if session.is_some() { Some(current_file_size) } else { None },
                        local_ready,
                    )]));
                    let wall_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let rec = ChatRecord::new_text(
                        wall_ms,
                        clock.current_pts(Instant::now()).max(0.0) as u64,
                        [0u8; 16],
                        "System".to_string(),
                        "Disconnected from watch party.",
                    );
                    let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                }
                PlayerCommand::StepFrame { forward } => {
                    let now = Instant::now();
                    let current_pts = clock.current_pts(now).max(0.0) as u64;
                    let step_ms = 42; // ~24fps single frame
                    let target = if forward {
                        current_pts.saturating_add(step_ms)
                    } else {
                        current_pts.saturating_sub(step_ms)
                    };
                    let was_playing = clock.is_playing();
                    if let Some(ref mut sess) = session {
                        perform_session_seek(
                            sess,
                            target,
                            was_playing,
                            &clock,
                            &video_ring,
                            &mut overflow_frame,
                            &audio_tx,
                            &mut current_audio_generation,
                            audio_delay_ms,
                            subtitle_delay_ms,
                            &event_tx,
                        );
                    } else {
                        clock.seek_to(target as f64, now);
                    }
                }
                PlayerCommand::ToggleLoop => {
                    is_loop_enabled = !is_loop_enabled;
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Repeat: {}", if is_loop_enabled { "On" } else { "Off" }),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::SetLoop(enabled) => {
                    is_loop_enabled = enabled;
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Repeat: {}", if is_loop_enabled { "On" } else { "Off" }),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::SetAudioEqualizer(cfg) => {
                    let _ = audio_tx.send(AudioWorkerCommand::SetEqualizer(cfg));
                }
                PlayerCommand::SetAudioCompressor {
                    enabled,
                    threshold_db,
                    ratio,
                    makeup_gain_db,
                } => {
                    let comp = if enabled {
                        Some(syncverse_codecs::audio::AudioDynamicsCompressor::new(
                            threshold_db,
                            ratio,
                            10.0,
                            100.0,
                            makeup_gain_db,
                            48000,
                        ))
                    } else {
                        None
                    };
                    let _ = audio_tx.send(AudioWorkerCommand::SetCompressor(comp));
                }
                PlayerCommand::SetAudioStereoPan(pan) => {
                    let _ = audio_tx.send(AudioWorkerCommand::SetStereoPan(pan));
                }
            }
        }

        // 2. Process playback decoding and audio output
        let mut did_work = false;
        if let Some(ref mut sess) = session {
            if clock.is_playing() {
                let current_gen = clock.seek_generation();

                // First push any pending overflow frame if ring has space
                if let Some(frame) = overflow_frame.take() {
                    if push_video_frame_stamped(frame, current_gen, &video_ring, &mut overflow_frame) {
                        did_work = true;
                    }
                }

                // Retry previously EAGAIN-rejected video packet before pulling new ones
                if overflow_frame.is_none() {
                    if let Some(packet) = sess.unsent_video_packet.take() {
                        if let Some(ref mut vdec) = sess.video_decoder {
                            let active_sub = sess
                                .selected_subtitle_track_index
                                .and_then(|idx| sess.available_subtitle_tracks.get(idx));
                            feed_video_packet_with_backpressure(
                                packet,
                                vdec,
                                &sess.rgba_pool,
                                &mut sess.unsent_video_packet,
                                active_sub,
                                subtitle_delay_ms,
                                current_gen,
                                &video_ring,
                                &mut overflow_frame,
                            );
                        }
                        did_work = true;
                    }
                }

                // Decode pending video packets while decoder accepts and ring has room
                // Cap per-iteration video decodes so UI commands (Seek/Pause/Audio switch) stay sub-frame.
                const MAX_VIDEO_DECODE_PER_ITER: usize = 3;
                let mut video_decoded_this_iter = 0usize;
                while overflow_frame.is_none()
                    && sess.unsent_video_packet.is_none()
                    && !sess.pending_video_packets.is_empty()
                    && video_decoded_this_iter < MAX_VIDEO_DECODE_PER_ITER
                {
                    if let Some(packet) = sess.pending_video_packets.pop_front() {
                        if let Some(ref mut vdec) = sess.video_decoder {
                            let active_sub = sess
                                .selected_subtitle_track_index
                                .and_then(|idx| sess.available_subtitle_tracks.get(idx));
                            feed_video_packet_with_backpressure(
                                packet,
                                vdec,
                                &sess.rgba_pool,
                                &mut sess.unsent_video_packet,
                                active_sub,
                                subtitle_delay_ms,
                                current_gen,
                                &video_ring,
                                &mut overflow_frame,
                            );
                        }
                        video_decoded_this_iter += 1;
                        did_work = true;
                    }
                }

                // Demux new packets from primary container (video + all subtitles, plus audio if single demuxer)
                if !sess.eof_reached
                    && sess.pending_video_packets.len() < 64
                    && sess.unsent_video_packet.is_none()
                    && video_decoded_this_iter < MAX_VIDEO_DECODE_PER_ITER
                {
                    let mut packets_read = 0;
                    while packets_read < 32 && video_decoded_this_iter < MAX_VIDEO_DECODE_PER_ITER {
                        match sess.demuxer.read_packet() {
                            Ok(Some(packet)) => {
                                packets_read += 1;
                                did_work = true;
                                let pkt_pts_ms = packet.pts_ms;

                                if Some(packet.track_id) == sess.video_track_id {
                                    let pkt_dts = packet.dts_ms.unwrap_or(packet.pts_ms);
                                    sess.last_demuxed_video_dts = Some(pkt_dts);
                                    if sess.pending_video_packets.is_empty()
                                        && overflow_frame.is_none()
                                        && sess.unsent_video_packet.is_none()
                                    {
                                        if let Some(ref mut vdec) = sess.video_decoder {
                                            let active_sub = sess
                                                .selected_subtitle_track_index
                                                .and_then(|idx| sess.available_subtitle_tracks.get(idx));
                                            feed_video_packet_with_backpressure(
                                                packet,
                                                vdec,
                                                &sess.rgba_pool,
                                                &mut sess.unsent_video_packet,
                                                active_sub,
                                                subtitle_delay_ms,
                                                current_gen,
                                                &video_ring,
                                                &mut overflow_frame,
                                            );
                                            video_decoded_this_iter += 1;
                                        }
                                    } else {
                                        sess.pending_video_packets.push_back(packet);
                                    }
                                } else if sess.audio_demuxer.is_none()
                                    && Some(packet.track_id) == sess.audio_track_id
                                {
                                    sess.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                                    if let Some(ref mut adec) = sess.audio_decoder {
                                        if let Ok(pcm_frames) =
                                            adec.decode_packet_us(&packet.data, packet.pts_us)
                                        {
                                            for pcm in pcm_frames {
                                                if !pcm.samples.is_empty() {
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::TimestampedSamples {
                                                            pts_ms: pcm.pts_ms,
                                                            samples: pcm.samples,
                                                            generation: current_audio_generation,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
                                } else if let Some(entry) =
                                    sess.subtitle_decoders.get_mut(&packet.track_id)
                                {
                                    process_embedded_subtitle_packet(
                                        entry,
                                        &packet.data,
                                        packet.pts_ms,
                                        packet.duration_ms.unwrap_or(3000),
                                        &mut sess.available_subtitle_tracks,
                                        sess.selected_subtitle_track_index,
                                        Some(&event_tx),
                                    );
                                }

                                if sess.video_track_id.is_none() {
                                    let now_pts = clock.current_pts(Instant::now());
                                    if (pkt_pts_ms as f64) > now_pts + 500.0 {
                                        break;
                                    }
                                }

                                if sess.pending_video_packets.len() >= 64
                                    || sess.unsent_video_packet.is_some()
                                {
                                    break;
                                }
                            }
                            Ok(None) => {
                                sess.eof_reached = true;
                                if let Some(ref mut vdec) = sess.video_decoder {
                                    if !sess.video_eof_sent {
                                        let _ = vdec.send_eof();
                                        sess.video_eof_sent = true;
                                    }
                                    while overflow_frame.is_none() {
                                        match vdec.receive_frame_rgba_pooled(
                                            &sess.rgba_pool,
                                            current_gen,
                                        ) {
                                            Ok(Some(mut rgba)) => {
                                                let pts_ms = rgba.pts_ms;
                                                let active_sub = sess
                                                    .selected_subtitle_track_index
                                                    .and_then(|idx| {
                                                        sess.available_subtitle_tracks.get(idx)
                                                    });
                                                composite_active_bitmap_subtitle(
                                                    &mut rgba,
                                                    active_sub,
                                                    pts_ms,
                                                    subtitle_delay_ms,
                                                );
                                                if !push_video_frame_stamped(
                                                    rgba,
                                                    current_gen,
                                                    &video_ring,
                                                    &mut overflow_frame,
                                                ) {
                                                    break;
                                                }
                                            }
                                            _ => break,
                                        }
                                    }
                                }
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }

                // Feed independent audio demuxer (bounded read-ahead <= 500ms ahead of master clock)
                if let (Some(ref mut ad), Some(audio_id), Some(ref mut adec)) = (
                    sess.audio_demuxer.as_mut(),
                    sess.audio_track_id,
                    sess.audio_decoder.as_mut(),
                ) {
                    if !sess.audio_eof_reached {
                        let target_audio_pts =
                            clock.current_pts(Instant::now()).max(0.0) as u64 + 500;
                        let mut audio_pkts = 0usize;
                        while audio_pkts < 32
                            && sess.last_demuxed_audio_pts_ms.unwrap_or(0) < target_audio_pts
                        {
                            match ad.read_packet() {
                                Ok(Some(packet)) => {
                                    if packet.track_id != audio_id {
                                        continue;
                                    }
                                    audio_pkts += 1;
                                    did_work = true;
                                    sess.last_demuxed_audio_pts_ms = Some(packet.pts_ms);
                                    if let Ok(pcm_frames) =
                                        adec.decode_packet_us(&packet.data, packet.pts_us)
                                    {
                                        for pcm in pcm_frames {
                                            if !pcm.samples.is_empty() {
                                                let _ = audio_tx.send(
                                                    AudioWorkerCommand::TimestampedSamples {
                                                        pts_ms: pcm.pts_ms,
                                                        samples: pcm.samples,
                                                        generation: current_audio_generation,
                                                    },
                                                );
                                            }
                                        }
                                    }
                                }
                                Ok(None) => {
                                    sess.audio_eof_reached = true;
                                    break;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        }

        // 3. Process watch-party networking and synchronize clocks
        if poll_host_upnp_status(&mut party_role, &clock, &event_tx) {
            did_work = true;
        }
        match &mut party_role {
            WatchPartyRole::Host {
                server,
                _room: _,
                local_username,
                client_names,
                last_state_broadcast,
                last_member_update,
                last_peer_count,
                last_probe_sent,
                upnp_rx: _,
                _upnp_guard: _,
                upnp_port: _,
                _crypto: _,
                pending_commit_unpause_ms,
                barrier_committed,
            } => {
                // Poll incoming UDP datagrams
                while let Ok(Some((_, sender_addr))) = server.poll_and_process() {
                    did_work = true;
                    client_names
                        .entry(sender_addr)
                        .or_insert_with(|| format!("Peer-{}", sender_addr.port()));
                }

                // Immediately push cached host status + peer statuses + room state to new joiners
                if let Ok(welcomed) = server.flush_welcome_to_new_peers() {
                    if welcomed > 0 {
                        did_work = true;
                    }
                }

                // Drain bidirectional PeerControl commands sent by clients
                let peer_controls = server.poll_peer_controls();
                for (_addr, ctrl) in peer_controls {
                    did_work = true;
                    let ctrl_now = Instant::now();
                    match ctrl.action {
                        PeerControlAction::Seek => {
                            let was_playing = clock.is_playing();
                            let landed_pts = if let Some(ref mut sess) = session {
                                perform_session_seek(
                                    sess,
                                    ctrl.media_pts_ms,
                                    was_playing,
                                    &clock,
                                    &video_ring,
                                    &mut overflow_frame,
                                    &audio_tx,
                                    &mut current_audio_generation,
                                    audio_delay_ms,
                                    subtitle_delay_ms,
                                    &event_tx,
                                )
                            } else {
                                clock.seek_to(ctrl.media_pts_ms as f64, ctrl_now);
                                ctrl.media_pts_ms
                            };
                            server.set_media_state(landed_pts, speed, !was_playing);
                            let _ = server.broadcast_room_state();
                        }
                        PeerControlAction::Pause => {
                            gate_blocked = false;
                            clock.set_state(ctrl_now, PlaybackState::Paused);
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                            server.reset_barrier();
                            *pending_commit_unpause_ms = None;
                            *barrier_committed = false;
                            let cur_pts = clock.current_pts(ctrl_now).max(0.0) as u64;
                            server.set_media_state(cur_pts, speed, true);
                            let _ = server.broadcast_room_state();
                        }
                        PeerControlAction::Play => {
                            let unready_exists = !force_play
                                && (!local_ready
                                    || session.is_none()
                                    || !server.are_all_users_ready());
                            if unready_exists {
                                clock.set_state(ctrl_now, PlaybackState::Paused);
                                let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                gate_blocked = true;
                                let unready: Vec<String> = server
                                    .get_unready_clients()
                                    .into_iter()
                                    .map(|(cid, uname)| {
                                        if uname.is_empty() {
                                            format!("Peer-{}", cid)
                                        } else {
                                            uname
                                        }
                                    })
                                    .collect();
                                let msg = RoomAnnouncement::gate_waiting(&unready);
                                let _ = event_tx.send(UiEvent::UnpauseBlocked {
                                    unready_peers: unready,
                                    message: msg,
                                });
                            } else {
                                gate_blocked = false;
                                let cur_pts = clock.current_pts(ctrl_now).max(0.0) as u64;
                                if server.clients().is_empty() {
                                    clock.set_state(ctrl_now, PlaybackState::Playing);
                                    let _ = audio_tx.send(AudioWorkerCommand::Resume);
                                    server.set_media_state(cur_pts, speed, false);
                                    let _ = server.broadcast_room_state();
                                } else if pending_commit_unpause_ms.is_none() {
                                    clock.set_state(ctrl_now, PlaybackState::Paused);
                                    let _ = audio_tx.send(AudioWorkerCommand::Pause);
                                    server.set_media_state(cur_pts, speed, true);
                                    *pending_commit_unpause_ms =
                                        server.prepare_barrier_unpause(50).ok();
                                    *barrier_committed = false;
                                    let _ = server.broadcast_room_state();
                                }
                            }
                        }
                        PeerControlAction::SetSpeed => {
                            speed = ctrl.rate_f64().clamp(0.2, 4.0);
                            clock.reanchor(ctrl_now, speed);
                            let _ = audio_tx.send(AudioWorkerCommand::SetSpeed(speed));
                            let cur_pts = clock.current_pts(ctrl_now).max(0.0) as u64;
                            server.set_media_state(cur_pts, speed, !clock.is_playing());
                            let _ = server.broadcast_room_state();
                        }
                        PeerControlAction::RequestSyncSnapshot => {}
                    }
                }

                // Progress two-phase barrier unpause commit -> scheduled synchronized resumption
                if let Some(target_ms) = *pending_commit_unpause_ms {
                    if !*barrier_committed
                        && (server.are_all_clients_ready()
                            || server.is_barrier_prepare_timed_out(Duration::from_millis(800)))
                    {
                        let _ = server.commit_barrier_unpause(target_ms);
                        *barrier_committed = true;
                        did_work = true;
                    }
                    if *barrier_committed && server.current_server_wall_ms() >= target_ms {
                        *pending_commit_unpause_ms = None;
                        *barrier_committed = false;
                        let _ = server.check_scheduled_unpause();
                        let unpause_now = Instant::now();
                        clock.set_state(unpause_now, PlaybackState::Playing);
                        let _ = audio_tx.send(AudioWorkerCommand::Resume);
                        let current_pts = clock.current_pts(unpause_now).max(0.0) as u64;
                        server.set_media_state(current_pts, speed, false);
                        let _ = server.broadcast_room_state();
                        did_work = true;
                    }
                }

                // Process pending chat messages
                let chat_msgs = server.poll_chat_messages();
                for msg in chat_msgs {
                    let rec = ChatRecord::from_chat_message(&msg);
                    for addr in server.clients().keys() {
                        if !client_names.contains_key(addr)
                            || client_names.get(addr).map(|s| s.starts_with("Peer-")).unwrap_or(false)
                        {
                            client_names.insert(*addr, rec.sender_name.clone());
                        }
                    }
                    let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                }

                // Process pending reactions
                for reaction in server.poll_reactions() {
                    let _ = event_tx.send(UiEvent::FloatingReaction {
                        emoji: reaction.emoji,
                        username: reaction.username,
                    });
                }

                // Prune stale / disconnected peers
                server.prune_stale_clients(Duration::from_secs(12));

                let now = Instant::now();
                // Send NTP probes to clients (~1 Hz) to measure RTT
                if now.duration_since(*last_probe_sent) >= Duration::from_millis(1000) {
                    *last_probe_sent = now;
                    let targets: Vec<SocketAddr> = server.clients().keys().copied().collect();
                    for target in targets {
                        let _ = server.send_ntp_probe_to(target);
                    }
                }

                // Periodic room state broadcast (~4 Hz) when not mid-barrier commit
                if pending_commit_unpause_ms.is_none()
                    && now.duration_since(*last_state_broadcast) >= Duration::from_millis(250)
                {
                    *last_state_broadcast = now;
                    let current_pts = clock.current_pts(now).max(0.0) as u64;
                    let is_paused = !clock.is_playing();
                    server.set_media_state(current_pts, speed, is_paused);
                    let _ = server.broadcast_room_state();
                }

                // Member list update (immediate on peer count change, otherwise ~2 Hz)
                let current_peer_count = server.clients().len();
                if current_peer_count != *last_peer_count
                    || now.duration_since(*last_member_update) >= Duration::from_millis(500)
                {
                    *last_peer_count = current_peer_count;
                    *last_member_update = now;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let local_hash = if current_media_hash != [0u8; 32] {
                        Some(current_media_hash)
                    } else {
                        None
                    };
                    let mut members = vec![RoomMember::with_media_and_hash(
                        local_username.clone(),
                        true,
                        true,
                        0.0,
                        current_file_name.clone(),
                        local_dur,
                        if session.is_some() {
                            Some(current_file_size)
                        } else {
                            None
                        },
                        local_ready,
                        local_hash,
                    )];

                    for (addr, client_session) in server.clients() {
                        if client_session.last_seen.elapsed() < Duration::from_secs(15) {
                            let name = if !client_session.username.is_empty() {
                                client_session.username.clone()
                            } else {
                                client_names
                                    .get(addr)
                                    .cloned()
                                    .unwrap_or_else(|| format!("Peer-{}", client_session.client_id))
                            };
                            let peer_hash = if client_session.media_hash != [0u8; 32] {
                                Some(client_session.media_hash)
                            } else {
                                None
                            };
                            members.push(RoomMember::with_media_and_hash(
                                name,
                                false,
                                false,
                                client_session.rtt_ms as f64,
                                if client_session.has_media && !client_session.filename.is_empty() {
                                    Some(client_session.filename.clone())
                                } else {
                                    None
                                },
                                if client_session.has_media {
                                    Some(client_session.duration_ms)
                                } else {
                                    None
                                },
                                if client_session.has_media {
                                    Some(client_session.file_size_bytes)
                                } else {
                                    None
                                },
                                client_session.is_user_ready,
                                peer_hash,
                            ));
                        }
                    }
                    let _ = event_tx.send(UiEvent::RoomMembersUpdated(members));
                }
            }
            WatchPartyRole::Client {
                client,
                _room: _,
                local_username,
                host_username,
                last_probe_sent,
                last_member_update,
                last_peer_count,
                _crypto: _,
                initial_sync_locked,
                last_pll_update,
            } => {
                // Poll incoming UDP datagrams
                while let Ok(Some(_)) = client.poll_and_process() {
                    did_work = true;
                }

                // Process pending chat messages
                let chat_msgs = client.poll_chat_messages();
                for msg in chat_msgs {
                    let rec = ChatRecord::from_chat_message(&msg);
                    if *host_username == "Host" && rec.sender_name != *local_username {
                        *host_username = rec.sender_name.clone();
                    }
                    let _ = event_tx.send(UiEvent::NewChatRecord(rec));
                }

                // Process pending reactions
                for reaction in client.poll_reactions() {
                    let _ = event_tx.send(UiEvent::FloatingReaction {
                        emoji: reaction.emoji,
                        username: reaction.username,
                    });
                }

                let now = Instant::now();
                // Dynamic NTP probe cadence: 75ms startup burst -> 1000ms steady state
                if now.duration_since(*last_probe_sent) >= client.recommended_ntp_probe_interval() {
                    *last_probe_sent = now;
                    let _ = client.send_ntp_probe();
                }

                // Check two-phase barrier scheduled unpause instant
                if client.check_scheduled_unpause() {
                    clock.set_state(now, PlaybackState::Playing);
                    let _ = audio_tx.send(AudioWorkerCommand::Resume);
                    did_work = true;
                }

                // Synchronize playback timeline from host's RoomState
                if let Some(room_state) = client.latest_room_state().cloned() {
                    if client.scheduled_unpause_local_ms().is_some() {
                        // Armed in barrier prepare: stay paused and snap within <= 25ms before unpause
                        if clock.is_playing() {
                            clock.set_state(now, PlaybackState::Paused);
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                        }
                        let local_pts_ms = clock.current_pts(now).max(0.0) as u64;
                        let drift_ms =
                            (room_state.tau_ref_media_ms as i64 - local_pts_ms as i64).unsigned_abs();
                        if drift_ms > 25 {
                            if let Some(ref mut sess) = session {
                                perform_session_seek(
                                    sess,
                                    room_state.tau_ref_media_ms,
                                    false,
                                    &clock,
                                    &video_ring,
                                    &mut overflow_frame,
                                    &audio_tx,
                                    &mut current_audio_generation,
                                    audio_delay_ms,
                                    subtitle_delay_ms,
                                    &event_tx,
                                );
                            } else {
                                clock.seek_to(room_state.tau_ref_media_ms as f64, now);
                            }
                        }
                    } else {
                        // Sync pause/play state without echoing back
                        if room_state.is_paused && clock.is_playing() {
                            clock.set_state(now, PlaybackState::Paused);
                            let _ = audio_tx.send(AudioWorkerCommand::Pause);
                        } else if !room_state.is_paused && !clock.is_playing() {
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);
                        }

                        let theta_hat = client.filtered_offset_nanos().unwrap_or(0.0);
                        let now_nanos = client.current_local_nanos();
                        let target_pts_ms =
                            room_state.compute_target_media_pts(now_nanos, theta_hat);
                        let local_pts_ms = clock.current_pts(now).max(0.0) as u64;
                        client.set_local_pts_ms(local_pts_ms);

                        let drift_ms =
                            (target_pts_ms as i64 - local_pts_ms as i64).unsigned_abs();

                        if client.ntp_accepted_count() >= 1 {
                            if client.should_snap_immediately(
                                drift_ms,
                                clock.is_playing(),
                                *initial_sync_locked,
                            ) {
                                let was_playing = clock.is_playing();
                                if let Some(ref mut sess) = session {
                                    perform_session_seek(
                                        sess,
                                        target_pts_ms,
                                        was_playing,
                                        &clock,
                                        &video_ring,
                                        &mut overflow_frame,
                                        &audio_tx,
                                        &mut current_audio_generation,
                                        audio_delay_ms,
                                        subtitle_delay_ms,
                                        &event_tx,
                                    );
                                } else {
                                    clock.seek_to(target_pts_ms as f64, now);
                                }
                                *initial_sync_locked = true;
                            } else {
                                *initial_sync_locked = true;
                                if drift_ms > 15 && clock.is_playing() {
                                    // Zone 2: 5Hz gated micro-speed PLL steering
                                    if now.duration_since(*last_pll_update)
                                        >= Duration::from_millis(200)
                                    {
                                        let actual_dt = now
                                            .duration_since(*last_pll_update)
                                            .as_secs_f64()
                                            .clamp(0.05, 1.0);
                                        *last_pll_update = now;
                                        if let Some(corr) = client.update_pll(actual_dt) {
                                            let target_speed = room_state.r_target;
                                            let steered_speed = corr
                                                .speed_multiplier
                                                .clamp(target_speed * 0.96, target_speed * 1.04);
                                            clock.reanchor(now, steered_speed);
                                            let _ = audio_tx
                                                .send(AudioWorkerCommand::SetSpeed(steered_speed));
                                        }
                                    }
                                } else if drift_ms <= 15 && clock.is_playing() {
                                    // Zone 1: Deadband - restore target rate
                                    let target_speed = room_state.r_target;
                                    if (clock.speed() - target_speed).abs() > 1e-4 {
                                        speed = target_speed;
                                        clock.reanchor(now, target_speed);
                                        let _ = audio_tx
                                            .send(AudioWorkerCommand::SetSpeed(target_speed));
                                    }
                                }
                            }
                        }
                    }
                }

                // Member list update (immediate on peer status arrival/change, otherwise ~2 Hz)
                let current_peer_count = client.peer_statuses().len();
                if current_peer_count != *last_peer_count
                    || now.duration_since(*last_member_update) >= Duration::from_millis(500)
                {
                    *last_peer_count = current_peer_count;
                    *last_member_update = now;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let local_hash = if current_media_hash != [0u8; 32] {
                        Some(current_media_hash)
                    } else {
                        None
                    };
                    let mut members = vec![RoomMember::with_media_and_hash(
                        local_username.clone(),
                        true,
                        false,
                        0.0,
                        current_file_name.clone(),
                        local_dur,
                        if session.is_some() {
                            Some(current_file_size)
                        } else {
                            None
                        },
                        local_ready,
                        local_hash,
                    )];

                    let peer_stats = client.peer_statuses().clone();
                    if peer_stats.is_empty() {
                        // Host status not yet received; add placeholder
                        members.push(RoomMember::new(
                            host_username.clone(),
                            false,
                            true,
                            client.median_rtt_ms() as f64,
                        ));
                    } else {
                        for status in peer_stats.values() {
                            let is_host_peer = status.client_id == 0;
                            let rtt = if is_host_peer {
                                client.median_rtt_ms() as f64
                            } else {
                                0.0
                            };
                            let peer_hash = if status.media_hash != [0u8; 32] {
                                Some(status.media_hash)
                            } else {
                                None
                            };
                            members.push(RoomMember::with_media_and_hash(
                                status.username.clone(),
                                false,
                                is_host_peer,
                                rtt,
                                if status.has_media && !status.filename.is_empty() {
                                    Some(status.filename.clone())
                                } else {
                                    None
                                },
                                if status.has_media {
                                    Some(status.duration_ms)
                                } else {
                                    None
                                },
                                if status.has_media {
                                    Some(status.file_size_bytes)
                                } else {
                                    None
                                },
                                status.is_ready,
                                peer_hash,
                            ));
                        }
                    }
                    let _ = event_tx.send(UiEvent::RoomMembersUpdated(members));
                }
            }
            WatchPartyRole::None => {}
        }

        // 4. Publish atomic telemetry snapshot to UI (~40 Hz)
        let now = Instant::now();
        if now.duration_since(last_snapshot_instant) >= Duration::from_millis(25) {
            last_snapshot_instant = now;
            let mut current_pts_ms = clock.current_pts(now);
            let dur_ms = session.as_ref().map(|s| s.duration_ms).unwrap_or(0);
            let mut is_playing = clock.is_playing();

            // EOF handling: loop back to 0 if repeat enabled, else auto-pause and notify UI
            if dur_ms > 0 && current_pts_ms >= dur_ms as f64 {
                if is_loop_enabled {
                    if let Some(ref mut sess) = session {
                        perform_session_seek(
                            sess,
                            0,
                            is_playing,
                            &clock,
                            &video_ring,
                            &mut overflow_frame,
                            &audio_tx,
                            &mut current_audio_generation,
                            audio_delay_ms,
                            subtitle_delay_ms,
                            &event_tx,
                        );
                    } else {
                        clock.seek_to(0.0, now);
                    }
                    current_pts_ms = 0.0;
                    if let WatchPartyRole::Host { server, .. } = &mut party_role {
                        server.set_media_state(0, speed, false);
                        let _ = server.broadcast_room_state();
                    }
                } else {
                    current_pts_ms = dur_ms as f64;
                    if is_playing {
                        clock.seek_to(dur_ms as f64, now);
                        clock.set_state(now, PlaybackState::Paused);
                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                        is_playing = false;
                        let _ = event_tx.send(UiEvent::PlaybackEofReached);
                        if let WatchPartyRole::Host { server, .. } = &mut party_role {
                            server.set_media_state(dur_ms, 1.0, true);
                            let _ = server.broadcast_room_state();
                        }
                    }
                }
            }

            let (
                peer_count,
                peers_ready,
                rtt_nanos,
                offset_nanos,
                phase_err_nanos,
                pll_ratio,
                has_mismatch,
            ) = match &party_role {
                WatchPartyRole::Host { server, .. } => {
                    let total = (server.clients().len() + 1).min(u16::MAX as usize) as u16;
                    let mut ready_count = if local_ready && session.is_some() {
                        1u16
                    } else {
                        0u16
                    };
                    let mut mismatch = false;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let local_file = current_file_name.as_deref();

                    for client_sess in server.clients().values() {
                        if client_sess.is_user_ready && client_sess.has_media {
                            ready_count = ready_count.saturating_add(1);
                        }
                        if client_sess.has_media {
                            let st = evaluate_file_match(
                                local_file,
                                local_dur,
                                Some(&client_sess.filename),
                                Some(client_sess.duration_ms),
                            );
                            if st == FileMatchStatus::Mismatch {
                                mismatch = true;
                            }
                        }
                    }
                    let rtt_ns = server.max_rtt_ms() * 1_000_000;
                    (
                        total,
                        ready_count,
                        rtt_ns,
                        0i64,
                        0i64,
                        (speed * 65536.0) as u32,
                        mismatch,
                    )
                }
                WatchPartyRole::Client { client, .. } => {
                    let total = (client.peer_statuses().len() + 1).min(u16::MAX as usize) as u16;
                    let mut ready_count = if local_ready && session.is_some() {
                        1u16
                    } else {
                        0u16
                    };
                    let mut mismatch = false;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let local_file = current_file_name.as_deref();

                    for status in client.peer_statuses().values() {
                        if status.is_ready && status.has_media {
                            ready_count = ready_count.saturating_add(1);
                        }
                        if status.has_media {
                            let st = evaluate_file_match(
                                local_file,
                                local_dur,
                                Some(&status.filename),
                                Some(status.duration_ms),
                            );
                            if st == FileMatchStatus::Mismatch {
                                mismatch = true;
                            }
                        }
                    }

                    let rtt_ns = client.median_rtt_nanos() as u64;
                    let offset_ns = client.filtered_offset_nanos().unwrap_or(0.0) as i64;
                    let phase_err_ms = if let Some(room) = client.latest_room_state() {
                        let target = room.compute_target_media_pts(
                            client.current_local_nanos(),
                            offset_ns as f64,
                        );
                        (target as i64) - (current_pts_ms.max(0.0) as i64)
                    } else {
                        0
                    };
                    (
                        total,
                        ready_count,
                        rtt_ns,
                        offset_ns,
                        phase_err_ms * 1_000_000,
                        (clock.speed() * 65536.0) as u32,
                        mismatch,
                    )
                }
                WatchPartyRole::None => (
                    1u16,
                    if local_ready && session.is_some() {
                        1u16
                    } else {
                        0u16
                    },
                    0,
                    0,
                    0,
                    (speed * 65536.0) as u32,
                    false,
                ),
            };

            let mut playback_state_flags = if is_playing { 1 } else { 0 };
            if gate_blocked {
                playback_state_flags |= SNAPSHOT_FLAG_GATE_BLOCKED;
            }
            if force_play {
                playback_state_flags |= SNAPSHOT_FLAG_FORCE_PLAY;
            }
            if local_ready {
                playback_state_flags |= SNAPSHOT_FLAG_LOCAL_READY;
            }
            if has_mismatch {
                playback_state_flags |= SNAPSHOT_FLAG_HAS_MISMATCH;
            }
            if is_muted {
                playback_state_flags |= SNAPSHOT_FLAG_MUTED;
            }
            if is_loop_enabled {
                playback_state_flags |= SNAPSHOT_FLAG_LOOP_ENABLED;
            }

            let snap = PlayerUiSnapshot {
                monotonic_timestamp: now.elapsed().as_nanos() as u64,
                media_position_pts: (current_pts_ms.max(0.0) * 1_000_000.0) as u64,
                media_duration_pts: dur_ms * 1_000_000,
                playback_state: playback_state_flags,
                network_rtt_nanos: rtt_nanos,
                clock_offset_nanos: offset_nanos,
                phase_error_nanos: phase_err_nanos,
                pll_speed_ratio_q16: pll_ratio,
                peer_count,
                peers_ready,
                room_sequence: 1,
            };
            exchange.publish(snap);
        }

        // 5. Sleep only when idle so decode/network bursts run with minimum jitter
        if !did_work {
            let sleep_ms = if clock.is_playing() { 1 } else { 4 };
            thread::sleep(Duration::from_millis(sleep_ms));
        }
    }
}

/// Launches the full desktop graphical player with integrated background decoding and presentation runtime.
pub fn launch_desktop_player_app(
    initial_file: Option<PathBuf>,
) -> Result<(), CoreError> {
    let exchange = Arc::new(TripleBufferSnapshotExchange::new());
    let video_ring = Arc::new(SpscRingBuffer::<VideoFrameRGBA, 32>::new());
    let clock = Arc::new(MasterClock::new(0.0, 1.0, PlaybackState::Paused));
    let (command_tx, command_rx) = channel::<PlayerCommand>();
    let (audio_tx, audio_rx) = channel::<AudioWorkerCommand>();
    let audio_shutdown_tx = audio_tx.clone();
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    // Spawn dedicated audio sink worker thread
    let audio_running = Arc::clone(&running);
    let audio_clock = Arc::clone(&clock);
    let audio_handle = thread::Builder::new()
        .name("syncverse-audio-worker".to_string())
        .spawn(move || {
            run_audio_sink_worker(audio_rx, audio_clock, audio_running);
        })
        .map_err(|e| CoreError::Runtime(format!("Failed to spawn audio worker thread: {e}")))?;

    // Spawn dedicated background playback coordinator thread
    let thread_exchange = Arc::clone(&exchange);
    let thread_video_ring = Arc::clone(&video_ring);
    let thread_clock = Arc::clone(&clock);
    let thread_running = Arc::clone(&running);

    let worker_handle = thread::Builder::new()
        .name("syncverse-playback-worker".to_string())
        .spawn(move || {
            run_playback_worker(
                command_rx,
                audio_tx,
                event_tx,
                thread_exchange,
                thread_video_ring,
                thread_clock,
                thread_running,
            );
        })
        .map_err(|e| CoreError::Runtime(format!("Failed to spawn playback worker thread: {e}")))?;

    // Run native graphical player on main thread (blocks until window is closed)
    let gui_res = syncverse_ui::run_desktop_player(
        initial_file,
        exchange,
        video_ring,
        clock,
        command_tx,
        event_rx,
    );

    // Stop background workers cleanly on exit
    running.store(false, Ordering::Relaxed);
    let _ = audio_shutdown_tx.send(AudioWorkerCommand::Shutdown);
    let _ = worker_handle.join();
    let _ = audio_handle.join();

    gui_res.map_err(|e| CoreError::Runtime(format!("GUI presentation error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_track_info() {
        let t1 = TrackInfo {
            track_id: 1,
            track_type: TrackType::Audio,
            codec_id: "aac".to_string(),
            language: Some("eng".to_string()),
            title: Some("English Commentary".to_string()),
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
            codec_private: None,
            ..Default::default()
        };
        let desc1 = format_track_info(&t1);
        assert_eq!(desc1, "English Commentary - ENG - aac - Stereo");

        let t2 = TrackInfo {
            track_id: 2,
            track_type: TrackType::Audio,
            codec_id: "opus".to_string(),
            language: None,
            title: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(6),
            codec_private: None,
            ..Default::default()
        };
        let desc2 = format_track_info(&t2);
        assert_eq!(desc2, "Track 2 - opus - 6ch");

        let t3 = TrackInfo {
            track_id: 3,
            track_type: TrackType::Audio,
            codec_id: "pcm_s16le".to_string(),
            language: Some("jpn".to_string()),
            title: None,
            width: None,
            height: None,
            sample_rate: Some(44100.0),
            channels: Some(1),
            codec_private: None,
            ..Default::default()
        };
        let desc3 = format_track_info(&t3);
        assert_eq!(desc3, "JPN - pcm_s16le - Mono");
    }

    #[test]
    fn test_audio_track_cycling_and_selection_logic() {
        let tracks = [
            TrackInfo {
                track_id: 1,
                track_type: TrackType::Audio,
                codec_id: "aac".to_string(),
                language: Some("eng".to_string()),
                title: Some("English Stereo".to_string()),
                width: None,
                height: None,
                sample_rate: Some(48000.0),
                channels: Some(2),
                codec_private: None,
                ..Default::default()
            },
            TrackInfo {
                track_id: 2,
                track_type: TrackType::Audio,
                codec_id: "opus".to_string(),
                language: Some("jpn".to_string()),
                title: Some("Japanese 5.1 Surround".to_string()),
                width: None,
                height: None,
                sample_rate: Some(48000.0),
                channels: Some(6),
                codec_private: None,
                ..Default::default()
            },
            TrackInfo {
                track_id: 5,
                track_type: TrackType::Audio,
                codec_id: "flac".to_string(),
                language: Some("fre".to_string()),
                title: None,
                width: None,
                height: None,
                sample_rate: Some(48000.0),
                channels: Some(2),
                codec_private: None,
                ..Default::default()
            },
        ];

        // Simulate cycling index math
        let mut current_id = Some(1u64);

        // Cycle 1 -> 2
        let cur_idx = tracks.iter().position(|t| Some(t.track_id) == current_id).unwrap();
        let next_idx = (cur_idx + 1) % tracks.len();
        current_id = Some(tracks[next_idx].track_id);
        assert_eq!(current_id, Some(2));
        assert_eq!(format_track_info(&tracks[next_idx]), "Japanese 5.1 Surround - JPN - opus - 6ch");

        // Cycle 2 -> 5
        let cur_idx = tracks.iter().position(|t| Some(t.track_id) == current_id).unwrap();
        let next_idx = (cur_idx + 1) % tracks.len();
        current_id = Some(tracks[next_idx].track_id);
        assert_eq!(current_id, Some(5));
        assert_eq!(format_track_info(&tracks[next_idx]), "FRE - flac - Stereo");

        // Cycle 5 -> 1 (wraps around)
        let cur_idx = tracks.iter().position(|t| Some(t.track_id) == current_id).unwrap();
        let next_idx = (cur_idx + 1) % tracks.len();
        current_id = Some(tracks[next_idx].track_id);
        assert_eq!(current_id, Some(1));
        assert_eq!(format_track_info(&tracks[next_idx]), "English Stereo - ENG - aac - Stereo");
    }

    #[test]
    fn test_audio_delay_and_volume_math() {
        let mut audio_delay_ms: i64 = 0;
        assert_eq!(audio_delay_ms, 0);

        // Set delay
        audio_delay_ms = 120;
        assert_eq!(audio_delay_ms, 120);

        // Adjust delay positive
        audio_delay_ms = audio_delay_ms.saturating_add(50);
        assert_eq!(audio_delay_ms, 170);

        // Adjust delay negative
        audio_delay_ms = audio_delay_ms.saturating_add(-200);
        assert_eq!(audio_delay_ms, -30);

        // Delayed PTS calculation
        let packet_pts: u64 = 1000;
        let delayed_pts = (packet_pts as i64 + audio_delay_ms).max(0) as u64;
        assert_eq!(delayed_pts, 970);

        let early_pts: u64 = 20;
        let delayed_early = (early_pts as i64 + audio_delay_ms).max(0) as u64;
        assert_eq!(delayed_early, 0); // clamped to 0

        // Volume clamping and scaling
        let vol = 1.2f32.clamp(0.0, 1.0);
        assert_eq!(vol, 1.0);

        let vol_low = (-0.5f32).clamp(0.0, 1.0);
        assert_eq!(vol_low, 0.0);

        let sample = 0.8f32;
        let scaled = (sample * 0.5f32).clamp(-1.0, 1.0);
        assert!((scaled - 0.4f32).abs() < 1e-6);

        // Overscaled sample clamped
        let hot_sample = 1.5f32;
        let clamped = (hot_sample * 1.0).clamp(-1.0, 1.0);
        assert_eq!(clamped, 1.0);
    }

    #[test]
    fn test_playback_eof_and_volume_unmute_invariants() {
        // 1. Volume setting unmuting invariant
        let mut volume: f32;
        let mut is_muted = true;
        let new_vol = 0.5f32.clamp(0.0, 1.0);
        volume = new_vol;
        if volume > 0.0 {
            is_muted = false;
        }
        assert_eq!(volume, 0.5);
        assert!(!is_muted);

        // Setting volume 0 keeps muted if already muted
        let mut is_muted_zero = true;
        let zero_vol = 0.0f32.clamp(0.0, 1.0);
        volume = zero_vol;
        if volume > 0.0 {
            is_muted_zero = false;
        }
        assert_eq!(volume, 0.0);
        assert!(is_muted_zero);

        // 2. Audio and Subtitle delay clamping to [-5000, 5000]
        let mut audio_delay: i64 = 4900;
        audio_delay = (audio_delay + 200).clamp(-5000, 5000);
        assert_eq!(audio_delay, 5000);

        let mut sub_delay: i64 = -4950;
        sub_delay = (sub_delay - 100).clamp(-5000, 5000);
        assert_eq!(sub_delay, -5000);

        // 3. EOF boundary clamping and replay detection
        let dur_ms: u64 = 60_000;
        let mut current_pts_ms: f64 = 60_050.0;
        let mut is_playing = true;

        if dur_ms > 0 && current_pts_ms >= dur_ms as f64 {
            current_pts_ms = dur_ms as f64;
            if is_playing {
                is_playing = false;
            }
        }
        assert_eq!(current_pts_ms, 60_000.0);
        assert!(!is_playing);

        // When user unpauses while positioned at EOF, target resets to 0
        let will_play = !is_playing;
        let mut target_reset_to_zero = false;
        if will_play && current_pts_ms >= dur_ms as f64 {
            target_reset_to_zero = true;
            current_pts_ms = 0.0;
        }
        assert!(target_reset_to_zero);
        assert_eq!(current_pts_ms, 0.0);
    }
}
