//! Subtitle decoding, multi-track buffering, auto-selection, and RGBA bitmap compositing helpers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Sender;
use syncverse_codecs::frame::VideoFrameRGBA;
use syncverse_codecs::subtitle_decoder::{DecodedSubtitleEvent, UniversalSubtitleDecoder};
use syncverse_common::subtitles::{
    SubtitleCue as CommonSubtitleCue, SubtitleSource, SubtitleTrack as CommonSubtitleTrack,
    SubtitleTrackCandidate, select_initial_subtitle_track,
};
use syncverse_common::time::MediaTime;
use syncverse_demux::{TrackInfo, TrackType, UniversalDemuxer};
use syncverse_ui::event::UiEvent;
use syncverse_ui::gui::subtitle_overlay::{
    SubtitleCue as UiSubtitleCue, SubtitleTrack as UiSubtitleTrack,
};

/// Metadata and stateful decoder entry for an embedded container subtitle stream.
pub(crate) struct EmbeddedSubtitleDecoderEntry {
    /// Index of this stream inside `PlaybackSession::available_subtitle_tracks`.
    pub(crate) track_index: usize,
    /// Stateful FFmpeg / pure-Rust fallback decoder instance for this track.
    pub(crate) decoder: UniversalSubtitleDecoder,
}

/// Converts a slice of in-tree [`CommonSubtitleTrack`] objects into UI overlay tracks.
pub(crate) fn to_ui_subtitle_tracks(tracks: &[CommonSubtitleTrack]) -> Vec<UiSubtitleTrack> {
    tracks
        .iter()
        .map(|track| {
            let ui_cues = track
                .cues
                .iter()
                .filter(|cue| !cue.text.is_empty())
                .map(|cue| {
                    UiSubtitleCue::new(
                        cue.start.as_millis() as i64,
                        cue.end.as_millis() as i64,
                        cue.text.clone(),
                    )
                })
                .collect();
            UiSubtitleTrack::with_format_and_origin(
                track.id,
                track.title.clone(),
                track.language.clone(),
                ui_cues,
                track.format.clone(),
                track.source == SubtitleSource::External,
            )
        })
        .collect()
}

/// Converts an in-tree [`CommonSubtitleCue`] into a UI [`UiSubtitleCue`].
#[inline]
pub(crate) fn to_ui_subtitle_cue(cue: &CommonSubtitleCue) -> UiSubtitleCue {
    UiSubtitleCue::new(
        cue.start.as_millis() as i64,
        cue.end.as_millis() as i64,
        cue.text.clone(),
    )
}

/// Builds a [`UniversalSubtitleDecoder`] for each embedded subtitle stream in `track_infos`.
///
/// `stream_to_track_index` maps the demuxer stream `TrackInfo::id` to the index within
/// `PlaybackSession::available_subtitle_tracks`.
pub(crate) fn build_embedded_subtitle_decoders(
    demuxer: &UniversalDemuxer,
    track_infos: &[TrackInfo],
    stream_to_track_index: &HashMap<u64, usize>,
) -> HashMap<u64, EmbeddedSubtitleDecoderEntry> {
    let mut decoders = HashMap::new();

    for track in track_infos {
        if track.track_type != TrackType::Subtitle {
            continue;
        }
        let Some(&track_index) = stream_to_track_index.get(&track.track_id) else {
            continue;
        };

        let decoder = if let Some((params, time_base)) = demuxer.subtitle_stream_params(track.track_id) {
            match UniversalSubtitleDecoder::from_stream_params(&params, time_base) {
                Ok(dec) => dec,
                Err(_) => UniversalSubtitleDecoder::new_packet_parser(&track.codec_id),
            }
        } else {
            UniversalSubtitleDecoder::new_packet_parser(&track.codec_id)
        };

        decoders.insert(
            track.track_id,
            EmbeddedSubtitleDecoderEntry {
                track_index,
                decoder,
            },
        );
    }

    decoders
}

/// Checks whether any subtitle file in the same directory as `media_path` has a stem
/// matching the media file's stem exactly (case-insensitive) for auto-selection priority.
pub(crate) fn find_exact_stem_sidecar_languages(media_path: &Path) -> Vec<String> {
    let Some(parent) = media_path.parent() else {
        return Vec::new();
    };
    let Some(media_stem) = media_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let media_stem_lower = media_stem.to_ascii_lowercase();
    let mut exact_matches = Vec::new();

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let ext_lower = ext.to_ascii_lowercase();
            if !matches!(ext_lower.as_str(), "srt" | "vtt" | "ass" | "ssa" | "sub") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.to_ascii_lowercase() == media_stem_lower {
                    exact_matches.push(stem.to_ascii_lowercase());
                }
            }
        }
    }

    exact_matches
}

/// Runs VLC-style initial subtitle track selection over embedded and external tracks.
pub(crate) fn choose_initial_subtitle_track(
    tracks: &[CommonSubtitleTrack],
    embedded_flags: &[(bool, bool)],
    has_exact_stem_external: bool,
    preferred_lang: Option<&str>,
) -> Option<usize> {
    let mut candidates = Vec::with_capacity(tracks.len());
    let mut assigned_exact_external = false;

    for (index, track) in tracks.iter().enumerate() {
        let is_external = track.source == SubtitleSource::External;
        let (is_forced, is_default) = if is_external {
            (false, false)
        } else {
            embedded_flags.get(index).copied().unwrap_or((false, false))
        };
        let exact_stem_match = if is_external
            && has_exact_stem_external
            && !assigned_exact_external
            && track.title == "External Subtitle"
        {
            assigned_exact_external = true;
            true
        } else {
            false
        };

        candidates.push(SubtitleTrackCandidate {
            index,
            is_external,
            is_forced,
            is_default,
            language: track.language.clone(),
            exact_stem_match,
        });
    }

    select_initial_subtitle_track(&candidates, preferred_lang)
}

/// Decodes a demuxed embedded subtitle packet, buffers its cues into the track store
/// regardless of whether the track is selected, and forwards active-track events to the UI.
pub(crate) fn process_embedded_subtitle_packet(
    entry: &mut EmbeddedSubtitleDecoderEntry,
    packet_data: &[u8],
    pts_ms: u64,
    duration_ms: u64,
    tracks: &mut [CommonSubtitleTrack],
    active_subtitle_track: Option<usize>,
    ui_event_tx: Option<&Sender<UiEvent>>,
) {
    let Ok(events) = entry
        .decoder
        .decode_packet(packet_data, pts_ms, Some(duration_ms))
    else {
        return;
    };
    if events.is_empty() {
        return;
    }

    let track_idx = entry.track_index;
    let Some(track) = tracks.get_mut(track_idx) else {
        return;
    };
    let is_active = active_subtitle_track == Some(track_idx);

    for event in events {
        match event {
            DecodedSubtitleEvent::Cue(cue) => {
                let ui_cue = if is_active && !cue.text.is_empty() {
                    Some(to_ui_subtitle_cue(&cue))
                } else {
                    None
                };
                let inserted = track.insert_cue_dedup(cue);
                if inserted {
                    if let (Some(tx), Some(ui_cue)) = (ui_event_tx, ui_cue) {
                        let _ = tx.send(UiEvent::SubtitleCueDemuxed(ui_cue));
                    }
                }
            }
            DecodedSubtitleEvent::ClearActive { at_ms } => {
                let clear_time = MediaTime::from_millis(at_ms);
                track.truncate_active_cues_at(clear_time);
                if is_active {
                    if let Some(tx) = ui_event_tx {
                        let _ = tx.send(UiEvent::SubtitleCueDemuxed(UiSubtitleCue::new(
                            at_ms as i64,
                            at_ms as i64,
                            "",
                        )));
                    }
                }
            }
        }
    }
}

/// Composites any active rasterized RGBA bitmap subtitles (PGS, DVD VobSub, DVB) onto
/// a decoded `VideoFrameRGBA` before it enters `video_ring`.
pub(crate) fn composite_active_bitmap_subtitle(
    frame: &mut VideoFrameRGBA,
    active_track: Option<&CommonSubtitleTrack>,
    pts_ms: u64,
    subtitle_delay_ms: i64,
) {
    let Some(track) = active_track else {
        return;
    };
    if frame.width == 0 || frame.height == 0 {
        return;
    }

    let effective_pts_ms = (pts_ms as i64 - subtitle_delay_ms).max(0) as u64;
    let effective_time = MediaTime::from_millis(effective_pts_ms);
    let active_cues = track.active_cues_at(effective_time);
    if active_cues.is_empty() {
        return;
    }

    let frame_w = frame.width as usize;
    let frame_h = frame.height as usize;
    let frame_stride = frame.stride;
    let dst_bytes = frame.data.as_mut_slice();

    for cue in active_cues {
        let Some(ref bmp) = cue.bitmap else {
            continue;
        };
        let bw = bmp.width as usize;
        let bh = bmp.height as usize;
        if bw == 0 || bh == 0 {
            continue;
        }
        let expected_rgba_len = bw.saturating_mul(bh).saturating_mul(4);
        if bmp.rgba.len() < expected_rgba_len {
            continue;
        }

        let canvas_w = if bmp.canvas_width > 0 {
            bmp.canvas_width as usize
        } else {
            frame_w
        };
        let canvas_h = if bmp.canvas_height > 0 {
            bmp.canvas_height as usize
        } else {
            frame_h
        };

        let dst_x0 = (bmp.x as usize).saturating_mul(frame_w) / canvas_w;
        let dst_y0 = (bmp.y as usize).saturating_mul(frame_h) / canvas_h;
        let dst_w = bw.saturating_mul(frame_w).div_ceil(canvas_w).max(1);
        let dst_h = bh.saturating_mul(frame_h).div_ceil(canvas_h).max(1);

        let x_end = dst_x0.saturating_add(dst_w).min(frame_w);
        let y_end = dst_y0.saturating_add(dst_h).min(frame_h);
        if dst_x0 >= x_end || dst_y0 >= y_end {
            continue;
        }

        for dy in dst_y0..y_end {
            let sy = ((dy - dst_y0) * bh / dst_h).min(bh - 1);
            let dst_row_off = dy.saturating_mul(frame_stride);
            let src_row_off = sy.saturating_mul(bw).saturating_mul(4);

            for dx in dst_x0..x_end {
                let sx = ((dx - dst_x0) * bw / dst_w).min(bw - 1);
                let src_idx = src_row_off + sx * 4;
                let alpha = bmp.rgba[src_idx + 3] as u32;
                if alpha == 0 {
                    continue;
                }

                let dst_idx = dst_row_off + dx * 4;
                if dst_idx + 4 > dst_bytes.len() {
                    continue;
                }

                if alpha == 255 {
                    dst_bytes[dst_idx] = bmp.rgba[src_idx];
                    dst_bytes[dst_idx + 1] = bmp.rgba[src_idx + 1];
                    dst_bytes[dst_idx + 2] = bmp.rgba[src_idx + 2];
                    dst_bytes[dst_idx + 3] = 255;
                } else {
                    let inv_alpha = 255 - alpha;
                    let sr = bmp.rgba[src_idx] as u32;
                    let sg = bmp.rgba[src_idx + 1] as u32;
                    let sb = bmp.rgba[src_idx + 2] as u32;

                    let dr = dst_bytes[dst_idx] as u32;
                    let dg = dst_bytes[dst_idx + 1] as u32;
                    let db = dst_bytes[dst_idx + 2] as u32;

                    dst_bytes[dst_idx] = ((sr * alpha + dr * inv_alpha) / 255) as u8;
                    dst_bytes[dst_idx + 1] = ((sg * alpha + dg * inv_alpha) / 255) as u8;
                    dst_bytes[dst_idx + 2] = ((sb * alpha + db * inv_alpha) / 255) as u8;
                    dst_bytes[dst_idx + 3] = 255;
                }
            }
        }
    }
}
