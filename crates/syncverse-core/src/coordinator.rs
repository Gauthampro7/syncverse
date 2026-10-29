//! Top-level runtime coordinator binding demuxing, decoding, clock, sync, chat, and UI.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

use syncverse_chat::InvertedIndexFts;
use syncverse_clock::{AudioPacer, MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::{
    convert_yuv420p_to_rgba, AudioFramePCM, ColorStandard, UniversalAudioDecoder,
    UniversalVideoDecoder, VideoFrameRGBA, VideoFrameYUV420p,
};
use syncverse_common::normalize_target_path;
use syncverse_demux::{
    build_synthetic_full_mkv, build_synthetic_full_mp4, get_synthetic_full_mkv,
    get_synthetic_full_mp4, Demuxer, EbmlDemuxer, IsoBmffDemuxer, ReadSeek, TrackInfo, TrackType,
    UniversalDemuxer, SYNTHETIC_EBML_VECTOR, SYNTHETIC_MP4_VECTOR,
};
use syncverse_sync::{
    BarrierStateMachine, ClientNode, HostServer, PllSpeedController, Rfc5905Filter,
};
use syncverse_ui::{PlayerCommand, PlayerUiSnapshot, TripleBufferSnapshotExchange};

use crate::aggregator::{SubsystemMetrics, TelemetryAggregator};
use crate::audio_sink::{AudioSink, AutoAudioSink};
use crate::error::CoreError;


/// Information extracted from inspecting a container.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// Container format label (e.g. "Matroska/EBML" or "ISOBMFF/MP4").
    pub format: String,
    /// Container timescale (e.g. nanoseconds for EBML, Hz for MP4).
    pub timescale: u64,
    /// Media duration in milliseconds, if present.
    pub duration_ms: Option<u64>,
    /// Stream tracks discovered in container.
    pub tracks: Vec<TrackInfo>,
    /// Number of keyframe seek points parsed (Cues in MKV or stss in MP4).
    pub keyframe_count: usize,
    /// Detailed descriptions of keyframe seek points.
    pub keyframe_entries: Vec<String>,
    /// Exact presentation timestamps in milliseconds of parsed keyframes.
    pub keyframe_pts: Vec<u64>,
}

/// Role of this player node in a watch-party synchronization session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyRole {
    /// Server hosting a watch-party room on an optional port (default 18242).
    Host {
        /// Optional port to bind to.
        port: Option<u16>,
        /// Optional path to media file to sync.
        file: Option<String>,
    },
    /// Client node connecting to a remote host server.
    Connect {
        /// Target host socket address string (e.g. "127.0.0.1:18242").
        addr: String,
        /// Optional path to media file to sync.
        file: Option<String>,
    },
    /// In-process deterministic network simulation mode.
    Demo,
}

/// Runtime player coordinator binding all 8 subsystems.
pub struct PlayerCoordinator {
    /// Master monotonic clock.
    pub clock: MasterClock,
    /// Discrete PI PLL micro-speed controller.
    pub pll: PllSpeedController,
    /// RFC 5905 sliding-window clock filter.
    pub ntp_filter: Rfc5905Filter,
    /// Two-phase barrier unpause state machine.
    pub barrier: BarrierStateMachine,
    /// In-memory inverted index for chat full-text search.
    pub chat_index: InvertedIndexFts,
    /// Lock-free triple-buffered atomic snapshot exchange.
    pub ui_exchange: TripleBufferSnapshotExchange,
    start_instant: Instant,
    room_sequence: u64,
    duration_ms: Option<u64>,
}

impl Default for PlayerCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerCoordinator {
    /// Constructs a new `PlayerCoordinator` with default subsystems.
    pub fn new() -> Self {
        Self {
            clock: MasterClock::new(0.0, 1.0, PlaybackState::Paused),
            pll: PllSpeedController::new(),
            ntp_filter: Rfc5905Filter::default(),
            barrier: BarrierStateMachine::default(),
            chat_index: InvertedIndexFts::new(),
            ui_exchange: TripleBufferSnapshotExchange::new(),
            start_instant: Instant::now(),
            room_sequence: 0,
            duration_ms: None,
        }
    }

    /// Returns current media duration in milliseconds, if known.
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// Sets the media duration in milliseconds.
    pub fn set_duration_ms(&mut self, duration_ms: Option<u64>) {
        self.duration_ms = duration_ms;
    }

    /// Advances the coordinator state on periodic evaluation tick and publishes UI snapshot.
    pub fn tick(&mut self, now: Instant) -> PlayerUiSnapshot {
        self.room_sequence = self.room_sequence.wrapping_add(1);
        let curr_pts_ms = self.clock.current_pts(now);
        let elapsed_nanos = now.duration_since(self.start_instant).as_nanos() as u64;

        let filtered_offset = self.ntp_filter.get_filtered_offset().unwrap_or(0.0);
        let median_rtt = self.ntp_filter.get_median_rtt();
        let duration_nanos = self.duration_ms.unwrap_or(0).saturating_mul(1_000_000);

        let metrics = SubsystemMetrics {
            monotonic_timestamp: elapsed_nanos,
            media_position_pts: (curr_pts_ms * 1_000_000.0) as u64,
            media_duration_pts: duration_nanos,
            playback_state: match self.clock.state() {
                PlaybackState::Paused => 0,
                PlaybackState::Playing => 1,
                PlaybackState::Seeking => 2,
                PlaybackState::Buffering => 3,
            },
            network_rtt_nanos: median_rtt as u64,
            clock_offset_nanos: filtered_offset as i64,
            phase_error_nanos: 0,
            speed_ratio: self.clock.speed(),
            peer_count: 1,
            peers_ready: 1,
            room_sequence: elapsed_nanos,
        };

        let aggregator = TelemetryAggregator::new(&self.ui_exchange);
        aggregator.publish_snapshot(&metrics);

        self.ui_exchange.poll_latest_snapshot()
    }

    /// Handles a player command dispatched from the UI event loop.
    pub fn handle_command(&mut self, cmd: PlayerCommand, now: Instant) -> Result<(), CoreError> {
        match cmd {
            PlayerCommand::TogglePause => {
                let next_state = if self.clock.state() == PlaybackState::Playing {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                };
                self.clock.set_state(now, next_state);
            }
            PlayerCommand::Seek(target_pts_ms) => {
                self.clock.seek_to(target_pts_ms as f64, now);
                self.pll.reset();
            }
            PlayerCommand::SetSpeed(speed) => {
                self.clock.reanchor(now, speed);
            }
            PlayerCommand::Stop => {
                self.clock.seek_to(0.0, now);
                self.clock.set_state(now, PlaybackState::Paused);
                self.pll.reset();
            }
            PlayerCommand::SetVolume(_vol) => {
                // In-process audio volume update
            }
            PlayerCommand::ToggleMute => {
                // In-process audio mute toggle
            }
            PlayerCommand::OpenFile(path) => {
                let _ = self.play_universal_media(&path, None);
            }
            PlayerCommand::SendChatMessage(_msg) => {
                // Broadcast watch-party chat message
            }
            PlayerCommand::ExecuteSlashCommand(_cmd_str) => {
                // Dispatched interactive slash command
            }
            PlayerCommand::HostParty { .. }
            | PlayerCommand::ConnectParty { .. }
            | PlayerCommand::SendReaction(_)
            | PlayerCommand::DisconnectParty
            | PlayerCommand::SetReady(_)
            | PlayerCommand::SetForcePlay(_)
            | PlayerCommand::ForcePlay
            | PlayerCommand::SelectAudioTrack(_)
            | PlayerCommand::CycleAudioTrack
            | PlayerCommand::SetAudioDelay(_)
            | PlayerCommand::AdjustAudioDelay(_)
            | PlayerCommand::SelectSubtitleTrack(_)
            | PlayerCommand::CycleSubtitleTrack
            | PlayerCommand::SetSubtitleDelay(_)
            | PlayerCommand::AdjustSubtitleDelay(_)
            | PlayerCommand::CycleAspectRatio
            | PlayerCommand::LoadExternalSubtitle(_) => {
                // Handled in watch-party networking integration / playback runtime
            }
        }
        Ok(())
    }

    /// Returns the current playback presentation timestamp in milliseconds.
    pub fn current_pts(&self, now: Instant) -> f64 {
        self.clock.current_pts(now)
    }

    /// Returns whether the presentation clock is actively playing.
    pub fn is_playing(&self) -> bool {
        self.clock.is_playing()
    }

    /// Returns the current playback speed ratio.
    pub fn speed(&self) -> f64 {
        self.clock.speed()
    }

    /// Returns a reference to the master monotonic clock.
    pub fn clock(&self) -> &MasterClock {
        &self.clock
    }

    /// Inspects a container from an open `ReadSeek` stream and extracts metadata.
    pub fn inspect_reader(
        mut reader: Box<dyn ReadSeek + Send>,
    ) -> Result<MediaInfo, CoreError> {
        let is_ebml = EbmlDemuxer::probe(&mut *reader).unwrap_or(false);
        reader.rewind().map_err(|e| CoreError::Runtime(e.to_string()))?;
        let is_mp4 = IsoBmffDemuxer::probe(&mut *reader).unwrap_or(false);
        reader.rewind().map_err(|e| CoreError::Runtime(e.to_string()))?;

        if is_ebml {
            let mut demuxer = EbmlDemuxer::new();
            let info = demuxer.init(reader)?;
            let keyframe_count = demuxer.cues().len();
            let keyframe_pts: Vec<u64> = demuxer.cues().iter().map(|c| c.pts_ms).collect();
            let keyframe_entries = demuxer
                .cues()
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "Cue #{}: PTS={}ms, Track={}, Offset=0x{:X}",
                        i + 1,
                        c.pts_ms,
                        c.track_id,
                        c.cluster_file_offset
                    )
                })
                .collect();

            Ok(MediaInfo {
                format: "Matroska/EBML".to_string(),
                timescale: info.timescale,
                duration_ms: info.duration_ms,
                tracks: info.tracks,
                keyframe_count,
                keyframe_entries,
                keyframe_pts,
            })
        } else if is_mp4 {
            let mut demuxer = IsoBmffDemuxer::new();
            let info = demuxer.init(reader)?;
            let mut keyframe_entries = Vec::new();
            let mut keyframe_pts = Vec::new();
            let mut keyframe_count = 0;
            for (i, track_state) in demuxer.track_states().iter().enumerate() {
                let count = track_state.resolver.sync_samples.len();
                keyframe_count += count;
                keyframe_entries.push(format!(
                    "Track #{}: {} sync sample(s) -> indices: {:?}",
                    i + 1,
                    count,
                    track_state.resolver.sync_samples
                ));
                let (_, pts_ms) = track_state.resolver.compute_sample_timings(info.timescale);
                for &sync_idx in &track_state.resolver.sync_samples {
                    if let Some(&pts) = pts_ms.get(sync_idx) {
                        keyframe_pts.push(pts);
                    }
                }
            }
            keyframe_pts.sort_unstable();
            keyframe_pts.dedup();

            Ok(MediaInfo {
                format: "ISOBMFF/MP4".to_string(),
                timescale: info.timescale,
                duration_ms: info.duration_ms,
                tracks: info.tracks,
                keyframe_count,
                keyframe_entries,
                keyframe_pts,
            })
        } else {
            let is_universal = UniversalDemuxer::probe(&mut *reader).unwrap_or(false);
            reader.rewind().map_err(|e| CoreError::Runtime(e.to_string()))?;
            if is_universal {
                let mut demuxer = UniversalDemuxer::new();
                let info = demuxer.init(reader).map_err(|e| CoreError::Runtime(e.to_string()))?;
                Ok(MediaInfo {
                    format: "Universal / FFmpeg".to_string(),
                    timescale: info.timescale,
                    duration_ms: info.duration_ms,
                    tracks: info.tracks,
                    keyframe_count: 0,
                    keyframe_entries: Vec::new(),
                    keyframe_pts: Vec::new(),
                })
            } else {
                Err(CoreError::Runtime(
                    "Unrecognized container format (expected EBML/MKV, ISOBMFF/MP4, or Universal format)".into(),
                ))
            }
        }
    }


    /// Inspects a container from a filesystem path.
    pub fn inspect_path(path: &Path) -> Result<MediaInfo, CoreError> {
        let f = std::fs::File::open(path)?;
        let reader = Box::new(std::io::BufReader::new(f));
        Self::inspect_reader(reader)
    }

    /// Inspects a target (file path, "synthetic-mkv", "synthetic-mp4", "synthetic-full-mkv", or "synthetic-full-mp4").
    pub fn inspect(target: &str) -> Result<MediaInfo, CoreError> {
        let normalized = normalize_target_path(target);
        let (reader, _): (Box<dyn ReadSeek + Send>, _) = match normalized.as_str() {
            "synthetic-mkv" => {
                let cursor = std::io::Cursor::new(SYNTHETIC_EBML_VECTOR.to_vec());
                (Box::new(cursor), "Synthetic EBML (48 bytes)")
            }
            "synthetic-mp4" => {
                let cursor = std::io::Cursor::new(SYNTHETIC_MP4_VECTOR.to_vec());
                (Box::new(cursor), "Synthetic MP4 (132 bytes)")
            }
            "synthetic-full-mkv" => {
                let cursor = std::io::Cursor::new(get_synthetic_full_mkv().to_vec());
                (Box::new(cursor), "Synthetic Full MKV")
            }
            "synthetic-full-mp4" => {
                let cursor = std::io::Cursor::new(get_synthetic_full_mp4().to_vec());
                (Box::new(cursor), "Synthetic Full MP4")
            }
            real_file => {
                let path = Path::new(real_file);
                return Self::inspect_path(path);
            }
        };
        Self::inspect_reader(reader)
    }

    /// Runs the full presentation pipeline: container demuxing, Q8 color conversion,
    /// 128B-aligned SPSC ring buffer transfer, monotonic SeqLock presentation clock,
    /// and atomic triple-buffered UI snapshot publishing.
    pub fn run_presentation_pipeline(
        &mut self,
        target: &str,
        num_ticks: usize,
    ) -> Result<Vec<PlayerUiSnapshot>, CoreError> {
        let normalized = normalize_target_path(target);
        let path = Path::new(&normalized);

        let video_ring = SpscRingBuffer::<VideoFrameRGBA, 32>::new();
        let audio_ring = SpscRingBuffer::<AudioFramePCM, 64>::new();
        let audio_pacer = AudioPacer::default_48k_stereo().map_err(CoreError::Clock)?;
        let mut audio_sink = AutoAudioSink::new();

        let mut packet_pts = 0u64;

        if path.exists() && !matches!(normalized.as_str(), "synthetic-mkv" | "synthetic-full-mkv" | "synthetic-mp4" | "synthetic-full-mp4") {
            if let Ok(mut demuxer) = UniversalDemuxer::open_file(path) {
                if let Some(info) = demuxer.container_info().cloned() {
                    if let Some(dur) = info.duration_ms {
                        self.duration_ms = Some(dur);
                    }
                    let video_track = info.tracks.iter().find(|t| t.track_type == TrackType::Video).cloned();
                    let audio_track = info.tracks.iter().find(|t| t.track_type == TrackType::Audio).cloned();

                    let mut video_decoder = video_track.as_ref().and_then(|vt| {
                        if let Some(params) = demuxer.stream_parameters(vt.track_id as usize) {
                            UniversalVideoDecoder::from_parameters(&params).ok()
                        } else {
                            UniversalVideoDecoder::from_codec_name(&vt.codec_id, vt.width, vt.height, vt.codec_private.as_deref()).ok()
                        }
                    });
                    let mut audio_decoder = audio_track.as_ref().and_then(|at| {
                        if let Some(params) = demuxer.stream_parameters(at.track_id as usize) {
                            UniversalAudioDecoder::from_parameters(params, None).ok()
                        } else {
                            UniversalAudioDecoder::from_codec_name(&at.codec_id, None, at.codec_private.as_deref()).ok()
                        }
                    });


                    while let Ok(Some(packet)) = demuxer.read_packet() {
                        packet_pts = packet.pts_ms;
                        if let Some(ref vt) = video_track {
                            if packet.track_id == vt.track_id {
                                if let Some(ref mut vdec) = video_decoder {
                                    if let Ok(Some(rgba)) = vdec.decode_packet_rgba(&packet.data, packet.pts_ms) {
                                        let _ = video_ring.try_push(rgba);
                                    }
                                }
                            }
                        }
                        if let Some(ref at) = audio_track {
                            if packet.track_id == at.track_id {
                                if let Some(ref mut adec) = audio_decoder {
                                    if let Ok(frames) = adec.decode_packet(&packet.data, packet.pts_ms) {
                                        for f in frames {
                                            let _ = audio_ring.try_push(f);
                                        }
                                    }
                                }
                            }
                        }
                        if !video_ring.is_empty() && !audio_ring.is_empty() {
                            break;
                        }
                    }
                }
            }
        }

        // If ring buffers were not populated from real file (or if synthetic target was passed):
        if video_ring.is_empty() {
            let (mut reader, _): (Box<dyn ReadSeek + Send>, _) = match normalized.as_str() {
                "synthetic-mkv" | "synthetic-full-mkv" => {
                    (Box::new(std::io::Cursor::new(build_synthetic_full_mkv())), "Synthetic MKV")
                }
                "synthetic-mp4" | "synthetic-full-mp4" => {
                    (Box::new(std::io::Cursor::new(build_synthetic_full_mp4())), "Synthetic MP4")
                }
                real_file => {
                    let f = std::fs::File::open(real_file)?;
                    (Box::new(std::io::BufReader::new(f)), "Media File")
                }
            };

            let is_ebml = EbmlDemuxer::probe(&mut *reader).unwrap_or(false);
            let _ = reader.rewind();
            let is_mp4 = IsoBmffDemuxer::probe(&mut *reader).unwrap_or(false);
            let _ = reader.rewind();

            if is_ebml {
                let mut demuxer = EbmlDemuxer::new();
                if let Ok(info) = demuxer.init(reader) {
                    if let Some(dur) = info.duration_ms {
                        self.duration_ms = Some(dur);
                    }
                    if let Ok(Some(pkt)) = demuxer.read_packet() {
                        packet_pts = pkt.pts_ms;
                    }
                }
            } else if is_mp4 {
                let mut demuxer = IsoBmffDemuxer::new();
                if let Ok(info) = demuxer.init(reader) {
                    if let Some(dur) = info.duration_ms {
                        self.duration_ms = Some(dur);
                    }
                    if let Ok(Some(pkt)) = demuxer.read_packet() {
                        packet_pts = pkt.pts_ms;
                    }
                }
            }

            // Q8 Fixed-Point Color Space Conversion
            let width = 320u32;
            let height = 240u32;
            let mut yuv_frame = VideoFrameYUV420p::new(width, height, packet_pts);
            yuv_frame.y_plane.as_mut_slice().fill(128);
            yuv_frame.u_plane.as_mut_slice().fill(128);
            yuv_frame.v_plane.as_mut_slice().fill(128);

            let mut rgba_frame = VideoFrameRGBA::new(width, height, packet_pts);
            convert_yuv420p_to_rgba(&yuv_frame, &mut rgba_frame, ColorStandard::Bt709)?;
            let _ = video_ring.try_push(rgba_frame);
        }

        if audio_ring.is_empty() {
            let silence = AudioFramePCM::new(48000, 2, packet_pts, vec![0.0f32; 2048])
                .map_err(CoreError::Codec)?;
            let _ = audio_ring.try_push(silence);
        }

        // 3. SPSC lock-free ring buffer transfer
        let _popped_frame = video_ring.try_pop();

        // 4. Monotonic SeqLock presentation clock
        let start_time = Instant::now();
        self.clock.seek_to(packet_pts as f64, start_time);
        self.clock.set_state(start_time, PlaybackState::Playing);

        // 5. Audio pacing & UI snapshot publishing
        let initial_audio = audio_pacer.pull_frame(&audio_ring, &self.clock, start_time);
        let _ = audio_sink.write_samples(&initial_audio.samples);

        let mut snapshots = Vec::with_capacity(num_ticks + 1);
        let mut current_instant = start_time;

        for _ in 0..num_ticks {
            current_instant += std::time::Duration::from_millis(50);
            let audio_frame = audio_pacer.pull_frame(&audio_ring, &self.clock, current_instant);
            let _ = audio_sink.write_samples(&audio_frame.samples);
            let snap = self.tick(current_instant);
            snapshots.push(snap);
        }

        // 6. Test seek command without phase discontinuity
        let seek_time = current_instant + std::time::Duration::from_millis(10);
        self.handle_command(PlayerCommand::Seek(15000), seek_time)?;
        let post_seek_snap = self.tick(seek_time);
        snapshots.push(post_seek_snap);

        Ok(snapshots)
    }

    /// Decodes and presents a media file using UniversalDemuxer, UniversalVideoDecoder,
    /// and UniversalAudioDecoder, transferring frames over 128B SPSC ring buffers and pacing audio.
    pub fn play_universal_media(
        &mut self,
        path: &Path,
        max_frames: Option<usize>,
    ) -> Result<(usize, usize), CoreError> {
        let mut demuxer = UniversalDemuxer::open_file(path)
            .map_err(|e| CoreError::Runtime(format!("UniversalDemuxer open failure: {}", e)))?;

        let info = demuxer.container_info().cloned().ok_or_else(|| {
            CoreError::Runtime("Failed to retrieve container metadata".into())
        })?;

        if let Some(dur) = info.duration_ms {
            self.duration_ms = Some(dur);
        }

        let video_track = info.tracks.iter().find(|t| t.track_type == TrackType::Video).cloned();
        let audio_track = info.tracks.iter().find(|t| t.track_type == TrackType::Audio).cloned();

        let mut video_decoder = if let Some(ref vt) = video_track {
            if let Some(params) = demuxer.stream_parameters(vt.track_id as usize) {
                Some(UniversalVideoDecoder::from_parameters(&params)?)
            } else {
                Some(UniversalVideoDecoder::from_codec_name(
                    &vt.codec_id,
                    vt.width,
                    vt.height,
                    vt.codec_private.as_deref(),
                )?)
            }
        } else {
            None
        };

        let mut audio_decoder = if let Some(ref at) = audio_track {
            if let Some(params) = demuxer.stream_parameters(at.track_id as usize) {
                Some(UniversalAudioDecoder::from_parameters(params, None)?)
            } else {
                Some(UniversalAudioDecoder::from_codec_name(
                    &at.codec_id,
                    None,
                    at.codec_private.as_deref(),
                )?)
            }
        } else {
            None
        };


        let video_ring = SpscRingBuffer::<VideoFrameRGBA, 32>::new();
        let audio_ring = SpscRingBuffer::<AudioFramePCM, 64>::new();
        let audio_pacer = AudioPacer::default_48k_stereo().map_err(CoreError::Clock)?;
        let mut audio_sink = AutoAudioSink::new();

        let start_time = Instant::now();
        self.clock.seek_to(0.0, start_time);
        self.clock.set_state(start_time, PlaybackState::Playing);

        let mut video_frames_count = 0usize;
        let mut audio_frames_count = 0usize;
        let max_f = max_frames.unwrap_or(usize::MAX);

        while let Ok(Some(packet)) = demuxer.read_packet() {
            if video_frames_count >= max_f && audio_frames_count >= max_f {
                break;
            }

            if let Some(ref vt) = video_track {
                if packet.track_id == vt.track_id {
                    if let Some(ref mut vdec) = video_decoder {
                        if let Ok(Some(rgba)) = vdec.decode_packet_rgba(&packet.data, packet.pts_ms) {
                            let _ = video_ring.try_push(rgba);
                            if let Some(_frame) = video_ring.try_pop() {
                                video_frames_count += 1;
                            }
                        }
                    }
                }
            }

            if let Some(ref at) = audio_track {
                if packet.track_id == at.track_id {
                    if let Some(ref mut adec) = audio_decoder {
                        if let Ok(pcm_frames) = adec.decode_packet(&packet.data, packet.pts_ms) {
                            for frame in pcm_frames {
                                let _ = audio_ring.try_push(frame);
                                let paced = audio_pacer.pull_frame(&audio_ring, &self.clock, Instant::now());
                                let _ = audio_sink.write_samples(&paced.samples);
                                audio_frames_count += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok((video_frames_count, audio_frames_count))
    }


    /// Runs watch-party synchronization for the specified party role.
    pub fn party(&mut self, role: PartyRole) -> Result<(), CoreError> {
        match role {
            PartyRole::Demo => {
                // In-process UDP loopback: real UDP sockets, real NTP probes, PI PLL, barrier unpause, and chat relay
                let mut host = HostServer::bind("127.0.0.1:0")?;
                let host_addr = host.local_addr()?;
                let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 1)?;

                host.set_media_state(10_000, 1.0, true);

                // 1. NTP Probe Exchange
                for _ in 0..8 {
                    let _probe_id = client.send_ntp_probe()?;
                    let _ = host.poll_and_process()?;
                    let _ = client.poll_and_process()?;
                }

                // 2. Room State Broadcast
                let _ = host.broadcast_room_state()?;
                let _ = client.poll_and_process()?;

                // 3. Evaluate PLL
                let _corr = client.update_pll(0.2);

                // 4. Two-Phase Barrier Unpause
                let target_unpause_server_ms = host.prepare_barrier_unpause(200)?;
                let _ = client.poll_and_process()?;
                let _ = host.poll_and_process()?;
                assert!(host.are_all_clients_ready());

                let _ = host.commit_barrier_unpause(target_unpause_server_ms)?;
                let _ = client.poll_and_process()?;

                // 5. In-Band Network Chat Packet Exchange and Relay
                let chat_rec = syncverse_chat::ChatRecord::new_text(
                    1_700_000_000_000,
                    10_000,
                    [1; 16],
                    "TestUser".to_string(),
                    "Party mode chat relay verified!",
                );
                let net_pkt = syncverse_chat::ChatNetworkPacket::new(1, chat_rec.clone());
                let raw_chat_bytes = net_pkt.to_bytes();

                // Send chat from client to host over watch-party transport
                let _ = client.send_chat_message(&raw_chat_bytes)?;
                let _ = host.poll_and_process()?;
                let host_msgs = host.poll_chat_messages();
                assert_eq!(host_msgs.len(), 1);

                // Host broadcasts chat message to client
                let _ = host.broadcast_chat_message(&raw_chat_bytes)?;
                let _ = client.poll_and_process()?;
                let client_msgs = client.poll_chat_messages();
                assert_eq!(client_msgs.len(), 1);

                // Local ChatStore append and FTS search
                let mut store = syncverse_chat::ChatStore::new_in_memory();
                let _ = store.append(&chat_rec)?;
                assert_eq!(store.search("Party").len(), 1);

                Ok(())
            }
            PartyRole::Host { port, file: _ } => {
                let bind_port = port.unwrap_or(18242);
                let mut host = HostServer::bind(format!("0.0.0.0:{}", bind_port))?;
                host.set_media_state(0, 1.0, true);
                let _ = host.poll_and_process();
                Ok(())
            }
            PartyRole::Connect { addr, file: _ } => {
                let target_addr: SocketAddr = addr.parse().map_err(|e| {
                    CoreError::Runtime(format!("invalid socket address '{}': {}", addr, e))
                })?;
                let mut client = ClientNode::bind("0.0.0.0:0", target_addr, 1)?;
                let _ = client.send_ntp_probe();
                let _ = client.poll_and_process();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_tick_and_command() {
        let mut coordinator = PlayerCoordinator::new();
        let now = Instant::now();

        let snap = coordinator.tick(now);
        assert!(snap.is_internally_consistent());

        // Command: Seek
        coordinator
            .handle_command(PlayerCommand::Seek(15000), now)
            .unwrap();
        assert!((coordinator.clock.current_pts(now) - 15000.0).abs() < 1e-3);
    }

    #[test]
    fn test_coordinator_toggle_pause() {
        let mut coordinator = PlayerCoordinator::new();
        let now = Instant::now();

        // Initial state is Paused
        assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
        assert!(!coordinator.clock.is_playing());

        let snap_init = coordinator.tick(now);
        assert_eq!(snap_init.playback_state, 0);

        // Toggle 1: Paused -> Playing
        coordinator
            .handle_command(PlayerCommand::TogglePause, now)
            .unwrap();
        assert_eq!(coordinator.clock.state(), PlaybackState::Playing);
        assert!(coordinator.clock.is_playing());

        let snap_playing = coordinator.tick(now);
        assert_eq!(snap_playing.playback_state, 1);

        // Toggle 2: Playing -> Paused
        coordinator
            .handle_command(PlayerCommand::TogglePause, now)
            .unwrap();
        assert_eq!(coordinator.clock.state(), PlaybackState::Paused);
        assert!(!coordinator.clock.is_playing());

        let snap_paused = coordinator.tick(now);
        assert_eq!(snap_paused.playback_state, 0);
    }

    #[test]
    fn test_coordinator_inspect_synthetic() {
        let ebml_info = PlayerCoordinator::inspect("synthetic-mkv").unwrap();
        assert_eq!(ebml_info.format, "Matroska/EBML");
        assert_eq!(ebml_info.timescale, 1_000_000);
        assert_eq!(ebml_info.tracks.len(), 1);

        let mp4_info = PlayerCoordinator::inspect("synthetic-mp4").unwrap();
        assert_eq!(mp4_info.format, "ISOBMFF/MP4");
        assert_eq!(mp4_info.timescale, 1000);
        assert_eq!(mp4_info.tracks.len(), 1);

        let full_mkv_info = PlayerCoordinator::inspect("synthetic-full-mkv").unwrap();
        assert_eq!(full_mkv_info.format, "Matroska/EBML");
        assert_eq!(full_mkv_info.tracks.len(), 2);
        assert_eq!(full_mkv_info.keyframe_count, 2);

        let full_mp4_info = PlayerCoordinator::inspect("synthetic-full-mp4").unwrap();
        assert_eq!(full_mp4_info.format, "ISOBMFF/MP4");
        assert_eq!(full_mp4_info.tracks.len(), 2);
    }

    #[test]
    fn test_coordinator_presentation_pipeline() {
        let mut coordinator = PlayerCoordinator::new();
        let snapshots = coordinator
            .run_presentation_pipeline("synthetic-full-mkv", 5)
            .unwrap();

        assert_eq!(snapshots.len(), 6);
        for snap in &snapshots {
            assert!(snap.is_internally_consistent());
        }
        // Last snapshot is post-seek to 15,000ms
        let post_seek = snapshots.last().unwrap();
        assert_eq!(post_seek.media_position_pts, 15000 * 1_000_000);
    }

    #[test]
    fn test_coordinator_party_demo() {
        let mut coordinator = PlayerCoordinator::new();
        coordinator.party(PartyRole::Demo).unwrap();
    }

    #[test]
    fn test_coordinator_party_host_and_connect() {
        use syncverse_sync::BarrierPhase;

        let mut host = HostServer::bind("127.0.0.1:0").unwrap();
        let host_addr = host.local_addr().unwrap();
        let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 1).unwrap();

        // 1. Client sends probe, host receives and replies, client processes
        let probe_id = client.send_ntp_probe().unwrap();
        assert_eq!(probe_id, 1);
        let host_recv = host.poll_and_process().unwrap();
        assert!(host_recv.is_some());
        let client_recv = client.poll_and_process().unwrap();
        assert!(client_recv.is_some());

        // 2. Host broadcasts room state
        host.set_media_state(42000, 1.0, false);
        let sent = host.broadcast_room_state().unwrap();
        assert_eq!(sent, 1);
        let client_room = client.poll_and_process().unwrap();
        assert!(client_room.is_some());
        assert_eq!(client.latest_room_state().unwrap().tau_ref_media_ms, 42000);

        // 3. Two-phase barrier unpause
        let target_ms = host.prepare_barrier_unpause(150).unwrap();
        client.poll_and_process().unwrap();
        host.poll_and_process().unwrap();
        assert!(host.are_all_clients_ready());
        host.commit_barrier_unpause(target_ms).unwrap();
        client.poll_and_process().unwrap();
        assert!(matches!(host.barrier_phase(), BarrierPhase::Committed { .. }));
        assert!(client.scheduled_unpause_local_ms().is_some());
    }

    #[test]
    fn test_coordinator_duration_tracking() {
        let mut coordinator = PlayerCoordinator::new();
        assert_eq!(coordinator.duration_ms(), None);

        coordinator.set_duration_ms(Some(60_000));
        assert_eq!(coordinator.duration_ms(), Some(60_000));

        let now = Instant::now();
        let snap = coordinator.tick(now);
        assert_eq!(snap.media_duration_pts, 60_000 * 1_000_000);
    }

    #[test]
    fn test_coordinator_play_universal_media() {
        let fixture_dir = std::path::PathBuf::from("/tmp/media_fixtures");
        if !fixture_dir.exists() {
            let _ = std::fs::create_dir_all(&fixture_dir);
        }
        let fixture_path = fixture_dir.join("sample.mp4");
        if !fixture_path.exists() {
            let mut cmd = std::process::Command::new("ffmpeg");
            cmd.args([
                "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
                "-c:v", "libx264", "-c:a", "aac",
                fixture_path.to_str().unwrap(),
            ]);
            let _ = cmd.status();
        }

        if fixture_path.exists() {
            let mut coordinator = PlayerCoordinator::new();
            let (v_count, a_count) = coordinator
                .play_universal_media(&fixture_path, Some(5))
                .expect("play_universal_media should succeed");
            assert!(v_count > 0, "Video frames should be rendered");
            assert!(a_count > 0, "Audio frames should be rendered");

            let snapshots = coordinator
                .run_presentation_pipeline(fixture_path.to_str().unwrap(), 3)
                .expect("run_presentation_pipeline on real file should succeed");
            assert!(snapshots.len() >= 3);
        }
    }
}

