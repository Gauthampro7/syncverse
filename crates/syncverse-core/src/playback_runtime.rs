//! Desktop playback runtime and background media decoding worker.
//!
//! Spawns a dedicated playback coordinator thread that handles commands from the desktop GUI
//! (`PlayerCommand`), demuxes containers with `UniversalDemuxer`, decodes video frames into
//! the SPSC ring buffer for GPU viewport rendering, streams normalized audio to `AutoAudioSink`
//! over a decoupled asynchronous channel, and publishes atomic UI snapshots to `TripleBufferSnapshotExchange`.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use syncverse_chat::{ChatRecord, RoomAnnouncement};
use syncverse_clock::{MasterClock, PlaybackState, SpscRingBuffer};
use syncverse_codecs::{UniversalAudioDecoder, UniversalVideoDecoder, VideoFrameRGBA};
use syncverse_demux::{Demuxer, TrackInfo, TrackType, UniversalDemuxer};
use syncverse_sync::{ClientNode, HostServer, PeerStatusPayload};
use syncverse_ui::{
    evaluate_file_match, FileMatchStatus, PlayerCommand, PlayerUiSnapshot, RoomMember,
    TripleBufferSnapshotExchange, UiEvent, SNAPSHOT_FLAG_FORCE_PLAY, SNAPSHOT_FLAG_GATE_BLOCKED,
    SNAPSHOT_FLAG_HAS_MISMATCH, SNAPSHOT_FLAG_LOCAL_READY,
};

use crate::audio_sink::{AudioSink, AutoAudioSink};

/// Commands delivered to the asynchronous audio streaming thread.
pub enum AudioWorkerCommand {
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

/// Active media playback session managing demuxer and decoders.
struct PlaybackSession {
    demuxer: UniversalDemuxer,
    video_decoder: Option<UniversalVideoDecoder>,
    audio_decoder: Option<UniversalAudioDecoder>,
    video_track_id: Option<u64>,
    audio_track_id: Option<u64>,
    available_audio_tracks: Vec<TrackInfo>,
    available_subtitle_tracks: Vec<syncverse_common::subtitles::SubtitleTrack>,
    selected_subtitle_track_index: Option<usize>,
    subtitle_track_id: Option<u64>,
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
        for t in embedded_subtitle_tracks {
            let title = t.title.clone().unwrap_or_else(|| {
                if let Some(ref lang) = t.language {
                    format!("Embedded Subtitle [{}]", lang.to_uppercase())
                } else {
                    format!("Embedded Subtitle (Track {})", t.track_id)
                }
            });
            available_subtitle_tracks.push(syncverse_common::subtitles::SubtitleTrack::new(
                t.track_id as usize,
                syncverse_common::subtitles::SubtitleSource::Embedded,
                t.codec_id.clone(),
                t.language.clone(),
                title,
                Vec::new(),
            ));
        }

        let external_tracks = syncverse_demux::subtitles::scan_external_subtitles(path);
        available_subtitle_tracks.extend(external_tracks);

        let vt = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .cloned();
        let at = available_audio_tracks.first().cloned();

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
            available_audio_tracks,
            available_subtitle_tracks,
            selected_subtitle_track_index: None,
            subtitle_track_id: None,
            duration_ms,
            eof_reached: false,
        })
    }

    /// Switches the active subtitle track to the specified index (`None` = Off/Disabled).
    /// Returns the human-readable description of the selected track or "Off".
    fn select_subtitle_track(&mut self, track_idx: Option<usize>) -> (Option<usize>, String) {
        match track_idx {
            Some(idx) if idx < self.available_subtitle_tracks.len() => {
                self.selected_subtitle_track_index = Some(idx);
                let track = &self.available_subtitle_tracks[idx];
                self.subtitle_track_id = if track.source == syncverse_common::subtitles::SubtitleSource::Embedded {
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
    /// Constructs a new `UniversalAudioDecoder` for the selected stream.
    /// Returns the human-readable description of the selected track on success.
    fn select_audio_track(&mut self, track_id: u64) -> Result<String, String> {
        let track = self
            .available_audio_tracks
            .iter()
            .find(|t| t.track_id == track_id)
            .cloned()
            .ok_or_else(|| format!("Audio track ID {} not found", track_id))?;

        let new_decoder = if let Some(params) = self.demuxer.stream_parameters(track.track_id as usize) {
            UniversalAudioDecoder::from_parameters(params, None)
                .map_err(|e| format!("Failed to create audio decoder: {e}"))?
        } else {
            UniversalAudioDecoder::from_codec_name(
                &track.codec_id,
                None,
                track.codec_private.as_deref(),
            )
            .map_err(|e| format!("Failed to create audio decoder: {e}"))?
        };

        self.audio_decoder = Some(new_decoder);
        self.audio_track_id = Some(track_id);

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

/// Active watch-party role and networking engine.
enum WatchPartyRole {
    None,
    Host {
        server: HostServer,
        _room: String,
        local_username: String,
        client_names: HashMap<SocketAddr, String>,
        last_state_broadcast: Instant,
        last_member_update: Instant,
        last_probe_sent: Instant,
    },
    Client {
        client: ClientNode,
        _room: String,
        local_username: String,
        host_username: String,
        last_probe_sent: Instant,
        last_member_update: Instant,
    },
}

/// Helper extracting local sender name for announcements and chat records.
fn get_sender_name(party_role: &WatchPartyRole) -> String {
    match party_role {
        WatchPartyRole::Host { local_username, .. } => local_username.clone(),
        WatchPartyRole::Client { local_username, .. } => local_username.clone(),
        WatchPartyRole::None => std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
    }
}

/// Helper broadcasting a standardized chat announcement locally and across connected room peers.
fn broadcast_chat_announcement(
    text: &str,
    now: Instant,
    clock: &MasterClock,
    party_role: &mut WatchPartyRole,
    event_tx: &Sender<UiEvent>,
) {
    let wall_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let media_ms = clock.current_pts(now).max(0.0) as u64;
    let rec = RoomAnnouncement::create_record(wall_ms, media_ms, text);
    let wire_bytes = rec.to_network_bytes();

    match party_role {
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

/// Helper broadcasting local peer media status to room peers.
fn broadcast_local_peer_status(
    has_media: bool,
    is_ready: bool,
    duration_ms: u64,
    file_size_bytes: u64,
    filename: &str,
    party_role: &mut WatchPartyRole,
) {
    match party_role {
        WatchPartyRole::Host {
            server,
            local_username,
            ..
        } => {
            let status = PeerStatusPayload {
                client_id: 0,
                has_media,
                is_ready,
                duration_ms,
                file_size_bytes,
                username: local_username.clone(),
                filename: filename.to_string(),
            };
            let _ = server.broadcast_peer_status(&status);
        }
        WatchPartyRole::Client {
            client,
            local_username,
            ..
        } => {
            let _ = client.send_peer_status(
                has_media,
                is_ready,
                duration_ms,
                file_size_bytes,
                local_username,
                filename,
            );
        }
        WatchPartyRole::None => {}
    }
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
    let mut local_ready: bool = false;
    let mut force_play: bool = false;
    let mut gate_blocked: bool = false;
    let mut volume: f32 = 1.0;
    let mut is_muted: bool = false;
    let mut audio_delay_ms: i64 = 0;
    let mut subtitle_delay_ms: i64 = 0;
    let mut speed: f64 = 1.0;
    let mut overflow_frame: Option<VideoFrameRGBA> = None;
    let mut last_snapshot_instant = Instant::now();
    let mut party_role = WatchPartyRole::None;

    while running.load(Ordering::Relaxed) {
        // 1. Drain all pending UI commands
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                PlayerCommand::OpenFile(path) => {
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
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

                            let ui_subtitle_tracks: Vec<syncverse_ui::gui::subtitle_overlay::SubtitleTrack> = new_session
                                .available_subtitle_tracks
                                .iter()
                                .map(|t| {
                                    let cues = t.cues.iter().map(|c| syncverse_ui::gui::subtitle_overlay::SubtitleCue {
                                        start_ms: c.start.as_millis() as i64,
                                        end_ms: c.end.as_millis() as i64,
                                        text: c.text.clone(),
                                    }).collect();
                                    syncverse_ui::gui::subtitle_overlay::SubtitleTrack::new(
                                        t.id,
                                        t.display_label(),
                                        t.language.clone(),
                                        cues,
                                    )
                                })
                                .collect();

                            let _ = event_tx.send(UiEvent::TracksDiscovered {
                                audio_tracks: audio_tracks_info,
                                subtitle_tracks: subtitle_tracks_info,
                            });
                            let _ = event_tx.send(UiEvent::SubtitleDataDiscovered(ui_subtitle_tracks));
                            if let Some(at) = new_session.available_audio_tracks.first() {
                                let _ = event_tx.send(UiEvent::AudioTrackSelected {
                                    track_id: at.track_id,
                                    name: format_track_info(at),
                                });
                            }

                            session = Some(new_session);
                            overflow_frame = None;
                            while video_ring.try_pop().is_some() {}

                            let now = Instant::now();
                            clock.seek_to(0.0, now);
                            clock.set_state(now, PlaybackState::Playing);

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
                    clock.set_state(now, PlaybackState::Playing);
                    let _ = audio_tx.send(AudioWorkerCommand::Resume);

                    let sender = get_sender_name(&party_role);
                    if let WatchPartyRole::Host { server, .. } = &mut party_role {
                        let current_pts = clock.current_pts(now).max(0.0) as u64;
                        server.set_media_state(current_pts, speed, false);
                        let _ = server.prepare_barrier_unpause(50);
                        let _ = server.broadcast_room_state();
                    }

                    let msg = RoomAnnouncement::resumed(&sender);
                    broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                }
                PlayerCommand::TogglePause => {
                    let now = Instant::now();
                    let will_play = !clock.is_playing();
                    let sender = get_sender_name(&party_role);

                    if will_play {
                        // Unpause trigger: evaluate unpause gate across room peers
                        let mut unready_peers = Vec::new();
                        let in_watch_party = match &party_role {
                            WatchPartyRole::Host { .. } | WatchPartyRole::Client { .. } => true,
                            WatchPartyRole::None => false,
                        };

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
                            clock.set_state(now, PlaybackState::Playing);
                            let _ = audio_tx.send(AudioWorkerCommand::Resume);

                            if let WatchPartyRole::Host { server, .. } = &mut party_role {
                                let current_pts = clock.current_pts(now).max(0.0) as u64;
                                server.set_media_state(current_pts, speed, false);
                                let _ = server.prepare_barrier_unpause(50);
                                let _ = server.broadcast_room_state();
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

                        if let WatchPartyRole::Host { server, .. } = &mut party_role {
                            let current_pts = clock.current_pts(now).max(0.0) as u64;
                            server.set_media_state(current_pts, speed, true);
                            let _ = server.broadcast_room_state();
                        }

                        let msg = RoomAnnouncement::paused(&sender);
                        broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                    }
                }
                PlayerCommand::Stop => {
                    let now = Instant::now();
                    clock.seek_to(0.0, now);
                    clock.set_state(now, PlaybackState::Paused);
                    gate_blocked = false;
                    overflow_frame = None;
                    while video_ring.try_pop().is_some() {}
                    let _ = audio_tx.send(AudioWorkerCommand::Reset);
                    if let Some(ref mut sess) = session {
                        sess.seek(0, &video_ring);
                    }
                    if let WatchPartyRole::Host { server, .. } = &mut party_role {
                        server.set_media_state(0, speed, true);
                        let _ = server.broadcast_room_state();
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
                    match &mut party_role {
                        WatchPartyRole::Host { server, .. } => {
                            server.set_media_state(target_ms, speed, !clock.is_playing());
                            let _ = server.broadcast_room_state();
                        }
                        WatchPartyRole::Client { client, .. } => {
                            let _ = client.send_seek(target_ms);
                        }
                        WatchPartyRole::None => {}
                    }
                    let now = Instant::now();
                    let sender = get_sender_name(&party_role);
                    let msg = RoomAnnouncement::seeked_ms(&sender, target_ms);
                    broadcast_chat_announcement(&msg, now, &clock, &mut party_role, &event_tx);
                }
                PlayerCommand::SetSpeed(spd) => {
                    speed = spd.clamp(0.2, 4.0);
                    let now = Instant::now();
                    clock.reanchor(now, speed);
                    if let WatchPartyRole::Host { server, .. } = &mut party_role {
                        let current_pts = clock.current_pts(now).max(0.0) as u64;
                        server.set_media_state(current_pts, speed, !clock.is_playing());
                        let _ = server.broadcast_room_state();
                    }
                }
                PlayerCommand::SetVolume(vol) => {
                    volume = vol.clamp(0.0, 1.0);
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Volume: {}%", (volume * 100.0).round() as u32),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::ToggleMute => {
                    is_muted = !is_muted;
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: if is_muted {
                            "Audio: Muted".to_string()
                        } else {
                            format!("Volume: {}%", (volume * 100.0).round() as u32)
                        },
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::SelectAudioTrack(track_id) => {
                    if let Some(ref mut sess) = session {
                        match sess.select_audio_track(track_id) {
                            Ok(name) => {
                                let _ = audio_tx.send(AudioWorkerCommand::Reset);
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
                            let _ = audio_tx.send(AudioWorkerCommand::Reset);
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
                    audio_delay_ms = delay_ms;
                    let _ = event_tx.send(UiEvent::AudioDelayChanged(audio_delay_ms));
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Audio Delay: {audio_delay_ms} ms"),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::AdjustAudioDelay(delta_ms) => {
                    audio_delay_ms = audio_delay_ms.saturating_add(delta_ms);
                    let _ = event_tx.send(UiEvent::AudioDelayChanged(audio_delay_ms));
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Audio Delay: {audio_delay_ms} ms"),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::SelectSubtitleTrack(track_index) => {
                    if let Some(ref mut sess) = session {
                        let (idx, name) = sess.select_subtitle_track(track_index);
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
                PlayerCommand::SetSubtitleDelay(delay_ms) => {
                    subtitle_delay_ms = delay_ms;
                    let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Subtitle Delay: {:+} ms", subtitle_delay_ms),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::AdjustSubtitleDelay(delta_ms) => {
                    subtitle_delay_ms = subtitle_delay_ms.saturating_add(delta_ms);
                    let _ = event_tx.send(UiEvent::SubtitleDelayChanged(subtitle_delay_ms));
                    let _ = event_tx.send(UiEvent::HudNotification {
                        text: format!("Subtitle Delay: {:+} ms", subtitle_delay_ms),
                        timestamp: Instant::now(),
                    });
                }
                PlayerCommand::CycleAspectRatio => {
                    // Handled by UI presentation layer / video viewport
                }
                PlayerCommand::SendChatMessage(text) | PlayerCommand::ExecuteSlashCommand(text) => {
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
                PlayerCommand::HostParty { port, room, username } => {
                    let bind_addr = format!("0.0.0.0:{}", port);
                    match HostServer::bind(&bind_addr) {
                        Ok(mut server) => {
                            let now = Instant::now();
                            let current_pts = clock.current_pts(now).max(0.0) as u64;
                            let is_paused = !clock.is_playing();
                            server.set_media_state(current_pts, speed, is_paused);

                            let dur_ms = session.as_ref().map(|s| s.duration_ms);
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
                            let rec = ChatRecord::new_text(
                                wall_ms,
                                current_pts,
                                [0u8; 16],
                                "System".to_string(),
                                &format!("★ Hosting watch party in room '{}' on port {}", room, port),
                            );
                            let _ = event_tx.send(UiEvent::NewChatRecord(rec));

                            party_role = WatchPartyRole::Host {
                                server,
                                _room: room,
                                local_username: username,
                                client_names: HashMap::new(),
                                last_state_broadcast: now,
                                last_member_update: now,
                                last_probe_sent: now,
                            };

                            broadcast_local_peer_status(
                                session.is_some(),
                                local_ready,
                                dur_ms.unwrap_or(0),
                                current_file_size,
                                current_file_name.as_deref().unwrap_or(""),
                                &mut party_role,
                            );
                        }
                        Err(e) => {
                            eprintln!("[ERROR] Failed to bind HostServer on {}: {}", bind_addr, e);
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
                PlayerCommand::ConnectParty { addr, room, username } => {
                    let target_addr: Option<SocketAddr> = addr
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut iter| iter.next());

                    if let Some(target) = target_addr {
                        let client_id = (SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as u32)
                            .unwrap_or(1))
                            & 0x7FFFFFFF;

                        match ClientNode::bind("0.0.0.0:0", target, client_id) {
                            Ok(mut client) => {
                                let _ = client.send_ntp_probe();

                                let now = Instant::now();
                                let wall_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                let current_pts = clock.current_pts(now).max(0.0) as u64;

                                let join_rec = ChatRecord::new_text(
                                    wall_ms,
                                    current_pts,
                                    [0u8; 16],
                                    username.clone(),
                                    &format!("👋 Joined room '{}'", room),
                                );
                                let _ = client.send_chat_message_with_pts(&join_rec.to_network_bytes(), current_pts);

                                let dur_ms = session.as_ref().map(|s| s.duration_ms);
                                let _ = client.send_peer_status(
                                    session.is_some(),
                                    local_ready,
                                    dur_ms.unwrap_or(0),
                                    current_file_size,
                                    &username,
                                    current_file_name.as_deref().unwrap_or(""),
                                );

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

                                let rec = ChatRecord::new_text(
                                    wall_ms,
                                    current_pts,
                                    [0u8; 16],
                                    "System".to_string(),
                                    &format!("🔗 Connected to {} (Room '{}')", addr, room),
                                );
                                let _ = event_tx.send(UiEvent::NewChatRecord(rec));

                                party_role = WatchPartyRole::Client {
                                    client,
                                    _room: room,
                                    local_username: username,
                                    host_username: "Host".to_string(),
                                    last_probe_sent: now,
                                    last_member_update: now,
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
                                        let delayed_pts = (packet.pts_ms as i64 + audio_delay_ms).max(0) as u64;
                                        if let Ok(pcm_frames) =
                                            adec.decode_packet(&packet.data, delayed_pts)
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
                                                    let clamped: Vec<f32> = pcm
                                                        .samples
                                                        .iter()
                                                        .map(|s| s.clamp(-1.0, 1.0))
                                                        .collect();
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::Samples(clamped),
                                                    );
                                                } else {
                                                    let scaled: Vec<f32> = pcm
                                                        .samples
                                                        .iter()
                                                        .map(|s| (s * effective_vol).clamp(-1.0, 1.0))
                                                        .collect();
                                                    let _ = audio_tx.send(
                                                        AudioWorkerCommand::Samples(scaled),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                else if Some(packet.track_id) == sess.subtitle_track_id {
                                    let text = if let Ok(s) = std::str::from_utf8(&packet.data) {
                                        syncverse_demux::subtitles::clean_srt_text(s)
                                    } else {
                                        String::new()
                                    };
                                    if !text.is_empty() {
                                        let dur = packet.duration_ms.unwrap_or(3000);
                                        let cue = syncverse_ui::gui::subtitle_overlay::SubtitleCue {
                                            start_ms: packet.pts_ms as i64,
                                            end_ms: (packet.pts_ms + dur) as i64,
                                            text,
                                        };
                                        let _ = event_tx.send(UiEvent::SubtitleCueDemuxed(cue));
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

        // 3. Process watch-party networking and synchronize clocks
        match &mut party_role {
            WatchPartyRole::Host {
                server,
                _room: _,
                local_username,
                client_names,
                last_state_broadcast,
                last_member_update,
                last_probe_sent,
            } => {
                // Poll incoming UDP datagrams
                while let Ok(Some((_, sender_addr))) = server.poll_and_process() {
                    did_work = true;
                    client_names
                        .entry(sender_addr)
                        .or_insert_with(|| format!("Peer-{}", sender_addr.port()));
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

                let now = Instant::now();
                // Send NTP probes to clients (~1 Hz) to measure RTT
                if now.duration_since(*last_probe_sent) >= Duration::from_millis(1000) {
                    *last_probe_sent = now;
                    let targets: Vec<SocketAddr> = server.clients().keys().copied().collect();
                    for target in targets {
                        let _ = server.send_ntp_probe_to(target);
                    }
                }

                // Periodic room state broadcast (~4 Hz)
                if now.duration_since(*last_state_broadcast) >= Duration::from_millis(250) {
                    *last_state_broadcast = now;
                    let current_pts = clock.current_pts(now).max(0.0) as u64;
                    let is_paused = !clock.is_playing();
                    server.set_media_state(current_pts, speed, is_paused);
                    let _ = server.broadcast_room_state();
                }

                // Periodic member list update (~2 Hz)
                if now.duration_since(*last_member_update) >= Duration::from_millis(500) {
                    *last_member_update = now;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let mut members = vec![RoomMember::with_media(
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
                            members.push(RoomMember::with_media(
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

                let now = Instant::now();
                // Send NTP probes to host (~1 Hz)
                if now.duration_since(*last_probe_sent) >= Duration::from_millis(1000) {
                    *last_probe_sent = now;
                    let _ = client.send_ntp_probe();
                }

                // Synchronize playback timeline from host's RoomState
                if let Some(room_state) = client.latest_room_state().cloned() {
                    let theta_hat = client.filtered_offset_nanos().unwrap_or(0.0);
                    let now_nanos = client.current_local_nanos();
                    let target_pts_ms = room_state.compute_target_media_pts(now_nanos, theta_hat);
                    let local_pts_ms = clock.current_pts(now).max(0.0) as u64;

                    // Sync pause/play state
                    if room_state.is_paused && clock.is_playing() {
                        clock.set_state(now, PlaybackState::Paused);
                        let _ = audio_tx.send(AudioWorkerCommand::Pause);
                    } else if !room_state.is_paused && !clock.is_playing() {
                        clock.set_state(now, PlaybackState::Playing);
                        let _ = audio_tx.send(AudioWorkerCommand::Resume);
                    }

                    // Tri-zone drift sync:
                    let drift_ms = (target_pts_ms as i64 - local_pts_ms as i64).abs();
                    if drift_ms > 1500 {
                        // Zone 3: Hard seek
                        if let Some(ref mut sess) = session {
                            sess.seek(target_pts_ms, &video_ring);
                            clock.seek_to(target_pts_ms as f64, now);
                        }
                    } else if drift_ms > 15 && clock.is_playing() {
                        // Zone 2: Micro-speed steering
                        if let Some(corr) = client.update_pll(0.025) {
                            let steered_speed = corr.speed_multiplier.clamp(0.96, 1.04);
                            clock.reanchor(now, steered_speed);
                        }
                    } else if drift_ms <= 15 && clock.is_playing() {
                        // Zone 1: Deadband - normal speed
                        if (speed - 1.0).abs() > 1e-4 {
                            speed = 1.0;
                            clock.reanchor(now, 1.0);
                        }
                    }
                }

                // Periodic member list update (~2 Hz)
                if now.duration_since(*last_member_update) >= Duration::from_millis(500) {
                    *last_member_update = now;
                    let local_dur = session.as_ref().map(|s| s.duration_ms);
                    let mut members = vec![RoomMember::with_media(
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
                            members.push(RoomMember::with_media(
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
            let current_pts_ms = clock.current_pts(now);
            let dur_ms = session.as_ref().map(|s| s.duration_ms).unwrap_or(0);
            let is_playing = clock.is_playing();

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

        // 5. Rate-limit thread execution
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
    let (event_tx, event_rx) = channel::<UiEvent>();
    let running = Arc::new(AtomicBool::new(true));

    // Spawn dedicated audio sink worker thread
    let audio_running = Arc::clone(&running);
    let audio_handle = thread::Builder::new()
        .name("syncverse-audio-worker".to_string())
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
        .map_err(|e| format!("Failed to spawn playback worker thread: {e}"))?;

    // Run native graphical player on main thread (blocks until window is closed)
    let gui_res = syncverse_ui::run_desktop_player(
        initial_file,
        exchange,
        video_ring,
        clock,
        command_tx,
        event_rx,
    );

    // Stop background workers on exit
    running.store(false, Ordering::Relaxed);
    let _ = worker_handle.join();
    let _ = audio_handle.join();

    gui_res.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
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
}
