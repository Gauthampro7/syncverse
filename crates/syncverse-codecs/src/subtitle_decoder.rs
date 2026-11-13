//! Universal subtitle decoder wrapping FFmpeg (`avcodec_decode_subtitle2`) with
//! pure-Rust packet parsing fallbacks for MP4 `mov_text`/`tx3g`, Matroska ASS/SSA,
//! SubRip, WebVTT, and rasterized RGBA bitmap conversion for DVD/PGS/DVB subtitles.

use std::ffi::CStr;
use std::sync::Arc;

use ffmpeg_next as ffmpeg;
use syncverse_common::subtitles::{SubtitleBitmap, SubtitleCue};
use syncverse_common::time::MediaTime;

use crate::error::CodecError;

/// Default cue duration in milliseconds when neither container packet nor decoded
/// `AVSubtitle` provides an explicit end display time for text cues.
const DEFAULT_TEXT_CUE_DURATION_MS: u64 = 3_000;

/// Default cue duration in milliseconds for open-ended bitmap cues (e.g. PGS)
/// that are expected to be terminated by a subsequent 0-rect clear packet.
const DEFAULT_BITMAP_CUE_DURATION_MS: u64 = 30_000;

/// A decoded subtitle event emitted by [`UniversalSubtitleDecoder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedSubtitleEvent {
    /// A timed subtitle cue (either plain text or a rasterized RGBA bitmap).
    Cue(SubtitleCue),
    /// An explicit subtitle clear event (e.g., MP4 `mov_text` `[0x00, 0x00]` or PGS/DVD 0-rect
    /// clear packet) indicating active cues should be truncated at `at_ms`.
    ClearActive {
        /// Presentation timestamp in milliseconds at which active cues end.
        at_ms: u64,
    },
}

/// Internal classification of subtitle codec families for dispatch and pure-Rust fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubtitleCodecKind {
    MovText,
    Ass,
    SubRip,
    WebVtt,
    Other,
}

impl SubtitleCodecKind {
    fn from_codec_id(id: ffmpeg::codec::Id) -> Self {
        match id {
            ffmpeg::codec::Id::MOV_TEXT => Self::MovText,
            ffmpeg::codec::Id::ASS | ffmpeg::codec::Id::SSA => Self::Ass,
            ffmpeg::codec::Id::SUBRIP
            | ffmpeg::codec::Id::SRT
            | ffmpeg::codec::Id::TEXT => Self::SubRip,
            ffmpeg::codec::Id::WEBVTT => Self::WebVtt,
            _ => Self::Other,
        }
    }

    fn from_name(name: &str) -> (Self, Option<ffmpeg::codec::Id>) {
        match name.trim().to_ascii_lowercase().as_str() {
            "mov_text" | "tx3g" => (Self::MovText, Some(ffmpeg::codec::Id::MOV_TEXT)),
            "ass" => (Self::Ass, Some(ffmpeg::codec::Id::ASS)),
            "ssa" => (Self::Ass, Some(ffmpeg::codec::Id::SSA)),
            "subrip" => (Self::SubRip, Some(ffmpeg::codec::Id::SUBRIP)),
            "srt" => (Self::SubRip, Some(ffmpeg::codec::Id::SRT)),
            "text" | "utf8" => (Self::SubRip, Some(ffmpeg::codec::Id::TEXT)),
            "webvtt" | "vtt" => (Self::WebVtt, Some(ffmpeg::codec::Id::WEBVTT)),
            "dvd_subtitle" | "dvdsub" | "vobsub" => {
                (Self::Other, Some(ffmpeg::codec::Id::DVD_SUBTITLE))
            }
            "hdmv_pgs_subtitle" | "pgssub" | "pgs" => {
                (Self::Other, Some(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE))
            }
            "dvb_subtitle" | "dvbsub" => (Self::Other, Some(ffmpeg::codec::Id::DVB_SUBTITLE)),
            "xsub" => (Self::Other, Some(ffmpeg::codec::Id::XSUB)),
            _ => (Self::Other, None),
        }
    }

    #[inline]
    fn supports_pure_rust_fallback(self) -> bool {
        matches!(
            self,
            Self::MovText | Self::Ass | Self::SubRip | Self::WebVtt
        )
    }
}

/// RAII guard ensuring FFmpeg's `AVSubtitle` allocations are always freed via `avsubtitle_free`.
struct AvSubtitleGuard(ffmpeg::Subtitle);

impl Drop for AvSubtitleGuard {
    fn drop(&mut self) {
        // SAFETY: `self.0` wraps a valid `AVSubtitle` (zero-initialized or populated by
        // `avcodec_decode_subtitle2`). `avsubtitle_free` safely frees any allocated rects
        // and resets the struct fields.
        unsafe {
            ffmpeg::ffi::avsubtitle_free(self.0.as_mut_ptr());
        }
    }
}

/// Universal subtitle decoder supporting FFmpeg subtitle codecs (`subrip`, `ass`, `ssa`,
/// `webvtt`, `mov_text`, `dvd_subtitle`, `hdmv_pgs_subtitle`, `dvb_subtitle`) and pure-Rust
/// packet parsing fallbacks.
pub struct UniversalSubtitleDecoder {
    decoder: Option<ffmpeg::codec::decoder::Subtitle>,
    codec_kind: SubtitleCodecKind,
    stream_time_base: ffmpeg::Rational,
    canvas_width: u32,
    canvas_height: u32,
}

impl UniversalSubtitleDecoder {
    /// Initializes a universal subtitle decoder from FFmpeg stream codec parameters and time base.
    ///
    /// Opens `Context::from_parameters(params.clone())?.decoder().subtitle()?`, falling back
    /// automatically to pure-Rust packet parsing for text subtitle codecs (`mov_text`, `ass`,
    /// `ssa`, `subrip`, `webvtt`) if FFmpeg decoder initialization fails.
    pub fn from_stream_params(
        params: &ffmpeg::codec::Parameters,
        stream_time_base: ffmpeg::Rational,
    ) -> Result<Self, CodecError> {
        let _ = ffmpeg::init();
        if params.medium() != ffmpeg::media::Type::Subtitle {
            return Err(CodecError::UnsupportedCodec(format!(
                "expected subtitle stream parameters, got {:?}",
                params.medium()
            )));
        }

        let codec_id = params.id();
        let codec_kind = SubtitleCodecKind::from_codec_id(codec_id);
        let effective_tb = if stream_time_base.denominator() > 0 && stream_time_base.numerator() > 0
        {
            stream_time_base
        } else {
            ffmpeg::Rational(1, 1000)
        };

        let opened = match ffmpeg::codec::context::Context::from_parameters(params.clone()) {
            Ok(mut ctx) => {
                // SAFETY: `ctx.as_mut_ptr()` returns a valid non-null `AVCodecContext` owned by `ctx`.
                // Setting `pkt_timebase` prior to opening informs FFmpeg of packet timestamp units.
                unsafe {
                    (*ctx.as_mut_ptr()).pkt_timebase = ffmpeg::ffi::AVRational {
                        num: effective_tb.numerator(),
                        den: effective_tb.denominator(),
                    };
                }
                ctx.decoder().subtitle()
            }
            Err(e) => Err(e),
        };

        match opened {
            Ok(sub_dec) => {
                // SAFETY: `sub_dec.as_ptr()` points to a valid opened `AVCodecContext`.
                let (canvas_width, canvas_height) = unsafe {
                    let raw = sub_dec.as_ptr();
                    ((*raw).width.max(0) as u32, (*raw).height.max(0) as u32)
                };
                Ok(Self {
                    decoder: Some(sub_dec),
                    codec_kind,
                    stream_time_base: effective_tb,
                    canvas_width,
                    canvas_height,
                })
            }
            Err(e) => {
                if codec_kind.supports_pure_rust_fallback() {
                    Ok(Self {
                        decoder: None,
                        codec_kind,
                        stream_time_base: effective_tb,
                        canvas_width: 0,
                        canvas_height: 0,
                    })
                } else {
                    Err(CodecError::Decoder(format!(
                        "failed to open subtitle decoder for {codec_id:?}: {e}"
                    )))
                }
            }
        }
    }

    /// Initializes a universal subtitle decoder from a codec name (e.g., `"mov_text"`, `"ass"`,
    /// `"subrip"`, `"webvtt"`, `"dvd_subtitle"`, `"hdmv_pgs_subtitle"`).
    ///
    /// Attempts to open the corresponding FFmpeg subtitle decoder when available and falls back
    /// seamlessly to pure-Rust packet parsing for text subtitle formats.
    pub fn new_packet_parser(codec_name: &str) -> Self {
        let _ = ffmpeg::init();
        let (codec_kind, codec_id_opt) = SubtitleCodecKind::from_name(codec_name);
        let tb = ffmpeg::Rational(1, 1000);

        let codec_opt = codec_id_opt
            .and_then(ffmpeg::decoder::find)
            .or_else(|| ffmpeg::decoder::find_by_name(codec_name.trim()));

        let decoder = codec_opt.and_then(|codec| {
            let mut ctx = ffmpeg::codec::context::Context::new_with_codec(codec);
            // SAFETY: `ctx.as_mut_ptr()` returns a valid non-null `AVCodecContext` owned by `ctx`.
            unsafe {
                (*ctx.as_mut_ptr()).pkt_timebase = ffmpeg::ffi::AVRational {
                    num: tb.numerator(),
                    den: tb.denominator(),
                };
            }
            ctx.decoder().subtitle().ok()
        });

        let (canvas_width, canvas_height) = match decoder {
            Some(ref sub_dec) => {
                // SAFETY: `sub_dec.as_ptr()` points to a valid opened `AVCodecContext`.
                unsafe {
                    let raw = sub_dec.as_ptr();
                    ((*raw).width.max(0) as u32, (*raw).height.max(0) as u32)
                }
            }
            None => (0, 0),
        };

        Self {
            decoder,
            codec_kind,
            stream_time_base: tb,
            canvas_width,
            canvas_height,
        }
    }

    /// Decodes a single demuxed subtitle packet into zero or more [`DecodedSubtitleEvent`]s.
    pub fn decode_packet(
        &mut self,
        data: &[u8],
        pts_ms: u64,
        duration_ms: Option<u64>,
    ) -> Result<Vec<DecodedSubtitleEvent>, CodecError> {
        // MP4 mov_text / tx3g uses a 2-byte big-endian length prefix followed by UTF-8 text and
        // optional trailing modifier boxes (`styl`, `hlit`, etc.). Empty length signals ClearActive.
        if self.codec_kind == SubtitleCodecKind::MovText {
            return Ok(decode_mov_text_packet(data, pts_ms, duration_ms));
        }

        if let Some(ref mut dec) = self.decoder {
            let mut pkt = ffmpeg::Packet::copy(data);
            let (pts_ticks, dur_ticks) =
                ms_to_tb_ticks(pts_ms, duration_ms, self.stream_time_base);
            pkt.set_pts(Some(pts_ticks));
            pkt.set_dts(Some(pts_ticks));
            if let Some(dur) = dur_ticks {
                pkt.set_duration(dur);
            }

            let mut guard = AvSubtitleGuard(ffmpeg::Subtitle::new());
            match dec.decode(&pkt, &mut guard.0) {
                Ok(true) => {
                    return Ok(extract_events_from_avsubtitle(
                        &guard.0,
                        pts_ms,
                        duration_ms,
                        self.canvas_width,
                        self.canvas_height,
                    ));
                }
                Ok(false) => {
                    return Ok(Vec::new());
                }
                Err(e) => {
                    if !self.codec_kind.supports_pure_rust_fallback() {
                        return Err(CodecError::Decoder(format!(
                            "FFmpeg subtitle decode error: {e}"
                        )));
                    }
                }
            }
        }

        Ok(decode_text_packet_fallback(
            self.codec_kind,
            data,
            pts_ms,
            duration_ms,
        ))
    }

    /// Flushes internal decoder buffers (must be called on seeks to clear stateful decoders
    /// such as PGS/DVB).
    pub fn flush(&mut self) {
        if let Some(ref mut dec) = self.decoder {
            dec.flush();
        }
    }
}

/// Converts millisecond `pts_ms` and optional `duration_ms` into `tb` ticks.
fn ms_to_tb_ticks(
    pts_ms: u64,
    duration_ms: Option<u64>,
    tb: ffmpeg::Rational,
) -> (i64, Option<i64>) {
    let num = tb.numerator() as i128;
    let den = tb.denominator() as i128;
    if num <= 0 || den <= 0 {
        return (pts_ms as i64, duration_ms.map(|d| d as i64));
    }
    let pts_ticks = ((pts_ms as i128) * den / (num * 1000)) as i64;
    let dur_ticks = duration_ms.map(|d| ((d as i128) * den / (num * 1000)) as i64);
    (pts_ticks, dur_ticks)
}

/// Decodes an MP4 `mov_text` / `tx3g` packet (`u16::from_be_bytes` length prefix + UTF-8 text,
/// ignoring trailing `styl`/`hlit` boxes).
fn decode_mov_text_packet(
    data: &[u8],
    pts_ms: u64,
    duration_ms: Option<u64>,
) -> Vec<DecodedSubtitleEvent> {
    if data.len() < 2 {
        return vec![DecodedSubtitleEvent::ClearActive { at_ms: pts_ms }];
    }

    let text_len = u16::from_be_bytes([data[0], data[1]]) as usize;
    if text_len == 0 {
        return vec![DecodedSubtitleEvent::ClearActive { at_ms: pts_ms }];
    }

    let text_end = 2usize.saturating_add(text_len).min(data.len());
    let text_bytes = &data[2..text_end];
    let raw_str = match std::str::from_utf8(text_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(text_bytes).into_owned(),
    };
    let cleaned = clean_srt_or_vtt_text(&raw_str);
    if cleaned.is_empty() {
        return vec![DecodedSubtitleEvent::ClearActive { at_ms: pts_ms }];
    }

    let dur = duration_ms
        .filter(|&d| d > 0)
        .unwrap_or(DEFAULT_TEXT_CUE_DURATION_MS);
    let start = MediaTime::from_millis(pts_ms);
    let end = MediaTime::from_millis(pts_ms.saturating_add(dur));
    vec![DecodedSubtitleEvent::Cue(SubtitleCue::new(
        start, end, cleaned,
    ))]
}

/// Pure-Rust packet decoder fallback for ASS/SSA, SubRip, and WebVTT packets.
fn decode_text_packet_fallback(
    kind: SubtitleCodecKind,
    data: &[u8],
    pts_ms: u64,
    duration_ms: Option<u64>,
) -> Vec<DecodedSubtitleEvent> {
    if data.is_empty() {
        return Vec::new();
    }

    let raw = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(data).into_owned(),
    };

    let text = match kind {
        SubtitleCodecKind::Ass => match parse_ass_event_text(&raw) {
            Some(Some(t)) => t,
            Some(None) => return Vec::new(),
            None => clean_ass_text(&raw),
        },
        SubtitleCodecKind::SubRip | SubtitleCodecKind::WebVtt | SubtitleCodecKind::Other => {
            clean_srt_or_vtt_text(&raw)
        }
        SubtitleCodecKind::MovText => return decode_mov_text_packet(data, pts_ms, duration_ms),
    };

    if text.is_empty() {
        return Vec::new();
    }

    let dur = duration_ms
        .filter(|&d| d > 0)
        .unwrap_or(DEFAULT_TEXT_CUE_DURATION_MS);
    vec![DecodedSubtitleEvent::Cue(SubtitleCue::new(
        MediaTime::from_millis(pts_ms),
        MediaTime::from_millis(pts_ms.saturating_add(dur)),
        text,
    ))]
}

/// Extracts [`DecodedSubtitleEvent`]s from an FFmpeg-decoded `AVSubtitle`.
fn extract_events_from_avsubtitle(
    sub: &ffmpeg::Subtitle,
    pts_ms: u64,
    duration_ms: Option<u64>,
    canvas_width: u32,
    canvas_height: u32,
) -> Vec<DecodedSubtitleEvent> {
    let start_ms = pts_ms.saturating_add(sub.start() as u64);
    let rects = sub.rects();

    if rects.len() == 0 {
        return vec![DecodedSubtitleEvent::ClearActive { at_ms: start_ms }];
    }

    let raw_end = sub.end();
    let has_explicit_end = raw_end > sub.start() && raw_end != u32::MAX;

    let mut events = Vec::new();
    for rect in rects {
        match rect {
            ffmpeg::subtitle::Rect::None(_) => {}
            ffmpeg::subtitle::Rect::Text(text_rect) => {
                // SAFETY: `text_rect.as_ptr()` is valid for the lifetime of `sub`. We check that
                // `(*ptr).text` is non-null before reading the NUL-terminated C string.
                let raw_text = unsafe {
                    let ptr = (*text_rect.as_ptr()).text;
                    if ptr.is_null() {
                        None
                    } else {
                        Some(
                            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes())
                                .into_owned(),
                        )
                    }
                };
                if let Some(raw) = raw_text {
                    let cleaned = clean_srt_or_vtt_text(&raw);
                    if !cleaned.is_empty() {
                        let end_ms = if has_explicit_end {
                            pts_ms.saturating_add(raw_end as u64)
                        } else {
                            start_ms.saturating_add(
                                duration_ms
                                    .filter(|&d| d > 0)
                                    .unwrap_or(DEFAULT_TEXT_CUE_DURATION_MS),
                            )
                        };
                        events.push(DecodedSubtitleEvent::Cue(SubtitleCue::new(
                            MediaTime::from_millis(start_ms),
                            MediaTime::from_millis(end_ms),
                            cleaned,
                        )));
                    }
                }
            }
            ffmpeg::subtitle::Rect::Ass(ass_rect) => {
                // SAFETY: `ass_rect.as_ptr()` is valid for the lifetime of `sub`. We check that
                // `(*ptr).ass` is non-null before reading the NUL-terminated C string.
                let raw_ass = unsafe {
                    let ptr = (*ass_rect.as_ptr()).ass;
                    if ptr.is_null() {
                        None
                    } else {
                        Some(
                            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes())
                                .into_owned(),
                        )
                    }
                };
                if let Some(raw) = raw_ass {
                    let cleaned_opt = match parse_ass_event_text(&raw) {
                        Some(maybe_text) => maybe_text,
                        None => {
                            let c = clean_ass_text(&raw);
                            if c.is_empty() {
                                None
                            } else {
                                Some(c)
                            }
                        }
                    };
                    if let Some(cleaned) = cleaned_opt {
                        let end_ms = if has_explicit_end {
                            pts_ms.saturating_add(raw_end as u64)
                        } else {
                            start_ms.saturating_add(
                                duration_ms
                                    .filter(|&d| d > 0)
                                    .unwrap_or(DEFAULT_TEXT_CUE_DURATION_MS),
                            )
                        };
                        events.push(DecodedSubtitleEvent::Cue(SubtitleCue::new(
                            MediaTime::from_millis(start_ms),
                            MediaTime::from_millis(end_ms),
                            cleaned,
                        )));
                    }
                }
            }
            ffmpeg::subtitle::Rect::Bitmap(bmp_rect) => {
                if let Some(bitmap) = decode_pal8_bitmap(&bmp_rect, canvas_width, canvas_height) {
                    let end_ms = if has_explicit_end {
                        pts_ms.saturating_add(raw_end as u64)
                    } else {
                        start_ms.saturating_add(
                            duration_ms
                                .filter(|&d| d > 0)
                                .unwrap_or(DEFAULT_BITMAP_CUE_DURATION_MS),
                        )
                    };
                    events.push(DecodedSubtitleEvent::Cue(SubtitleCue::new_bitmap(
                        MediaTime::from_millis(start_ms),
                        MediaTime::from_millis(end_ms),
                        bitmap,
                    )));
                }
            }
        }
    }

    events
}

/// Converts an FFmpeg `AVSubtitleRect` 8-bit paletted bitmap (`data[0]` indices + `data[1]`
/// `AV_PIX_FMT_RGB32` ARGB palette) into a packed RGBA [`SubtitleBitmap`].
fn decode_pal8_bitmap(
    bmp_rect: &ffmpeg::subtitle::Bitmap<'_>,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<SubtitleBitmap> {
    // SAFETY: `bmp_rect.as_ptr()` is valid for the lifetime of the `AVSubtitle` borrow.
    // All pointer dereferences and slice constructions are bounded by `w`, `h`, `linesize[0]`,
    // and `nb_colors` after validating non-null pointers and positive dimensions.
    unsafe {
        let raw = bmp_rect.as_ptr();
        let x = (*raw).x.max(0) as u32;
        let y = (*raw).y.max(0) as u32;
        let w = (*raw).w;
        let h = (*raw).h;
        let nb_colors = (*raw).nb_colors;
        let data_ptr = (*raw).data[0];
        let palette_ptr = (*raw).data[1];
        let linesize = (*raw).linesize[0];

        if w <= 0
            || h <= 0
            || nb_colors <= 0
            || data_ptr.is_null()
            || palette_ptr.is_null()
            || linesize < w
        {
            return None;
        }

        let width = w as usize;
        let height = h as usize;
        let stride = linesize as usize;
        let pixel_count = width.checked_mul(height)?;
        let rgba_len = pixel_count.checked_mul(4)?;

        let palette_len = (nb_colors as usize).min(256);
        let palette_bytes = std::slice::from_raw_parts(palette_ptr, palette_len * 4);
        let mut palette = [[0u8; 4]; 256];
        for (i, entry) in palette.iter_mut().take(palette_len).enumerate() {
            let argb = u32::from_ne_bytes([
                palette_bytes[i * 4],
                palette_bytes[i * 4 + 1],
                palette_bytes[i * 4 + 2],
                palette_bytes[i * 4 + 3],
            ]);
            let a = ((argb >> 24) & 0xFF) as u8;
            let r = ((argb >> 16) & 0xFF) as u8;
            let g = ((argb >> 8) & 0xFF) as u8;
            let b = (argb & 0xFF) as u8;
            *entry = [r, g, b, a];
        }

        let mut rgba = Vec::with_capacity(rgba_len);
        for row in 0..height {
            let row_slice = std::slice::from_raw_parts(data_ptr.add(row * stride), width);
            for &idx in row_slice {
                rgba.extend_from_slice(&palette[idx as usize]);
            }
        }

        Some(SubtitleBitmap {
            x,
            y,
            width: width as u32,
            height: height as u32,
            canvas_width,
            canvas_height,
            rgba: Arc::from(rgba),
        })
    }
}

/// Parses an ASS `Dialogue:` line or Matroska 8-comma ASS packet
/// (`ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text`) into cleaned text.
///
/// Returns:
/// - `Some(Some(text))` if `raw` is a valid ASS event with non-empty cleaned text.
/// - `Some(None)` if `raw` is a valid ASS event whose cleaned text is empty (e.g., `\p1` drawing).
/// - `None` if `raw` does not match an ASS `Dialogue:` or 8-comma Matroska packet header.
fn parse_ass_event_text(raw: &str) -> Option<Option<String>> {
    let trimmed = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if trimmed.is_empty() {
        return Some(None);
    }

    if let Some(after_prefix) = trimmed.strip_prefix("Dialogue:") {
        let parts: Vec<&str> = after_prefix.trim_start().splitn(10, ',').collect();
        if parts.len() == 10 {
            let cleaned = clean_ass_text(parts[9]);
            return Some(if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            });
        }
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(9, ',').collect();
    if parts.len() == 9
        && parts[0].trim().parse::<i64>().is_ok()
        && parts[1].trim().parse::<i64>().is_ok()
    {
        let cleaned = clean_ass_text(parts[8]);
        return Some(if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        });
    }

    None
}

/// Cleans ASS override tags (`{...}`), suppresses `\p<N>` vector drawing text, and converts
/// `\N`/`\n` -> `\n` and `\h` -> `' '`.
fn clean_ass_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut in_drawing_mode = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' {
            let block_start = i + 1;
            let mut block_end = block_start;
            while block_end < chars.len() && chars[block_end] != '}' {
                block_end += 1;
            }
            let block: String = chars[block_start..block_end].iter().collect();
            for raw_tag in block.split('\\').skip(1) {
                let tag = raw_tag.trim();
                if let Some(after_p) = tag.strip_prefix('p') {
                    let digit_len = after_p
                        .bytes()
                        .take_while(|b| b.is_ascii_digit())
                        .count();
                    if digit_len > 0 {
                        if let Ok(scale) = after_p[..digit_len].parse::<u32>() {
                            in_drawing_mode = scale > 0;
                        }
                    }
                } else if tag == "r" || tag.starts_with('r') {
                    in_drawing_mode = false;
                }
            }
            i = if block_end < chars.len() {
                block_end + 1
            } else {
                block_end
            };
            continue;
        }

        if in_drawing_mode {
            i += 1;
            continue;
        }

        if ch == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'N' | 'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                'h' => {
                    out.push(' ');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        out.push(ch);
        i += 1;
    }

    out.trim().to_string()
}

/// Strips inline HTML/VTT tags (`<...>`) and curly tags (`{...}`) and decodes HTML entities.
fn clean_srt_or_vtt_text(raw: &str) -> String {
    let trimmed = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let mut out = String::with_capacity(trimmed.len());
    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '<' => {
                if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '>') {
                    let end = i + 1 + rel_end;
                    if !chars[i + 1..end].contains(&'\n') {
                        i = end + 1;
                        continue;
                    }
                }
                out.push('<');
                i += 1;
            }
            '{' => {
                if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '}') {
                    let end = i + 1 + rel_end;
                    if !chars[i + 1..end].contains(&'\n') {
                        i = end + 1;
                        continue;
                    }
                }
                out.push('{');
                i += 1;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }

    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mov_text_long_payload_with_trailing_styl_and_clear() {
        let mut dec = UniversalSubtitleDecoder::new_packet_parser("mov_text");

        // Text >= 128 bytes (length = 140 = 0x008C) + trailing binary `styl` atom containing non-UTF8 bytes
        let long_text = "A".repeat(140);
        let mut pkt = Vec::new();
        pkt.extend_from_slice(&(long_text.len() as u16).to_be_bytes());
        pkt.extend_from_slice(long_text.as_bytes());
        pkt.extend_from_slice(&[0x00, 0x00, 0x00, 0x1E, b's', b't', b'y', b'l', 0xFF, 0xFE, 0x80]);

        let events = dec.decode_packet(&pkt, 1_500, Some(2_500)).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DecodedSubtitleEvent::Cue(cue) => {
                assert_eq!(cue.start, MediaTime::from_millis(1_500));
                assert_eq!(cue.end, MediaTime::from_millis(4_000));
                assert_eq!(cue.text, long_text);
                assert!(cue.bitmap.is_none());
            }
            other => panic!("expected Cue, got {other:?}"),
        }

        // Empty [0x00, 0x00] clear packet emits ClearActive
        let clear_events = dec.decode_packet(&[0x00, 0x00], 4_200, None).unwrap();
        assert_eq!(
            clear_events,
            vec![DecodedSubtitleEvent::ClearActive { at_ms: 4_200 }]
        );
    }

    #[test]
    fn test_ffmpeg_subrip_and_ass_decode() {
        let mut subrip_dec = UniversalSubtitleDecoder::new_packet_parser("subrip");
        let events = subrip_dec
            .decode_packet(b"<i>Hello</i> from <b>FFmpeg</b> SubRip!", 2_000, Some(3_000))
            .unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DecodedSubtitleEvent::Cue(cue) => {
                assert_eq!(cue.start, MediaTime::from_millis(2_000));
                assert_eq!(cue.end, MediaTime::from_millis(5_000));
                assert_eq!(cue.text, "Hello from FFmpeg SubRip!");
            }
            other => panic!("expected Cue, got {other:?}"),
        }

        let mut ass_dec = UniversalSubtitleDecoder::new_packet_parser("ass");
        let long_style_ass =
            b"12,0,VeryLongAnimeDialogueStyleNameOverThirtyChars,SpeakerName,0,0,0,,{\\b1}Line 1\\NLine 2";
        let ass_events = ass_dec
            .decode_packet(long_style_ass, 10_000, Some(2_000))
            .unwrap();
        assert_eq!(ass_events.len(), 1);
        match &ass_events[0] {
            DecodedSubtitleEvent::Cue(cue) => {
                assert_eq!(cue.text, "Line 1\nLine 2");
            }
            other => panic!("expected Cue, got {other:?}"),
        }

        // Drawing-only ASS packet must be suppressed (0 events)
        let drawing_ass = b"13,0,SignStyle,,0,0,0,,{\\p1}m 0 0 l 100 0 100 100 0 100{\\p0}";
        let drawing_events = ass_dec
            .decode_packet(drawing_ass, 12_000, Some(1_000))
            .unwrap();
        assert!(drawing_events.is_empty());
    }

    #[test]
    fn test_ffmpeg_dvd_subtitle_bitmap_decode() {
        let mut dvd_dec = UniversalSubtitleDecoder::new_packet_parser("dvd_subtitle");
        // Synthetic 2x2 DVD SPU packet:
        // Packet size = 0x0024 (36 bytes), DCSQ offset = 0x0010 (16 bytes)
        // Pixel data (4 bytes top field + 4 bytes bottom field? Let's craft valid RLE SPU):
        // Offset 0..2: total size = 0x0024
        // Offset 2..4: DCSQ offset = 0x0010
        // Offset 4..8: field 0 RLE (row 0: color 1 x2 -> 0x09 (0b00_0010_01 = 2px color 1, wait: 4-bit nibble 0x8 | color 1 -> 0x09? No: 0x08 | 1 = 0x09 is count=2, color=1 -> 1 byte = 0x09, aligned to byte! Let's pad row to 2 bytes: 0x09, 0x00))
        // Actually in DVD SPU RLE, nibbles are: count(2 bits) + color(2 bits) when count != 0 -> 1 nibble = 4 bits: (2 << 2) | 1 = 0x9. Two pixels in one row = 0x90 (high nibble 0x9, low nibble 0x0 pad).
        // Let's build a minimal valid DVD SPU packet for a 2x2 rect at (10, 20):
        // Offsets:
        // 0..2: 0x00, 0x20 (size = 32)
        // 2..4: 0x00, 0x08 (DCSQ offset = 8)
        // 4..6: 0x90, 0x00 (top field row 0: 2px color 1, wait: 0x9 = count 2 color 1, byte-align)
        // 6..8: 0xA0, 0x00 (bottom field row 1: 2px color 2)
        // DCSQ at offset 8:
        // 8..10: 0x00, 0x00 (PTS delay 0)
        // 10..12: 0x00, 0x08 (next DCSQ = 8 -> end)
        // Commands:
        // 12: 0x03 (SET_COLOR: 2 bytes -> c3,c2,c1,c0 = 0x32, 0x10)
        // 15: 0x04 (SET_CONTR: 2 bytes -> a3,a2,a1,a0 = 0xFF, 0xF0)
        // 18: 0x05 (SET_DSPXA: 6 bytes -> x1=10, x2=11, y1=20, y2=21)
        //     x1=10 (0x00A), x2=11 (0x00B) -> [0x00, 0xA0, 0x0B]
        //     y1=20 (0x014), y2=21 (0x015) -> [0x01, 0x40, 0x15]
        // 25: 0x06 (SET_CHG_DSPXA: 4 bytes -> top=0x0004, bot=0x0006)
        // 30: 0x01 (STA_DSP: start display)
        // 31: 0xFF (CMD_END)
        let spu: [u8; 32] = [
            0x00, 0x20, // packet size = 32
            0x00, 0x08, // DCSQ offset = 8
            0x90, 0x00, // top field (even row): 2px color 1
            0xA0, 0x00, // bottom field (odd row): 2px color 2
            // DCSQ @ 8:
            0x00, 0x00, // delay = 0
            0x00, 0x08, // next DCSQ = self (end)
            0x03, 0x32, 0x10, // SET_COLOR
            0x04, 0xFF, 0xF0, // SET_CONTR (alpha > 0)
            0x05, 0x00, 0xA0, 0x0B, 0x01, 0x40, 0x15, // SET_DSPXA (10..11, 20..21 => 2x2)
            0x06, 0x00, 0x04, 0x00, 0x06, // SET_CHG_DSPXA (top=4, bot=6)
            0x01, // STA_DSP
            0xFF, // CMD_END
        ];

        let events = dvd_dec.decode_packet(&spu, 5_000, Some(2_000)).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DecodedSubtitleEvent::Cue(cue) => {
                assert_eq!(cue.start, MediaTime::from_millis(5_000));
                let bmp = cue.bitmap.as_ref().expect("expected bitmap payload");
                assert_eq!(bmp.x, 10);
                assert_eq!(bmp.y, 20);
                assert_eq!(bmp.width, 2);
                assert_eq!(bmp.height, 2);
                assert_eq!(bmp.rgba.len(), 2 * 2 * 4);
                // Ensure non-zero alpha pixels were decoded from palette
                assert!(bmp.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0));
            }
            other => panic!("expected Cue with bitmap, got {other:?}"),
        }
    }

    #[test]
    fn test_avsubtitle_zero_rect_emits_clear_active() {
        let sub = ffmpeg::Subtitle::new();
        let events = extract_events_from_avsubtitle(&sub, 7_500, None, 1920, 1080);
        assert_eq!(
            events,
            vec![DecodedSubtitleEvent::ClearActive { at_ms: 7_500 }]
        );
    }

    #[test]
    fn test_from_stream_params_rejects_non_subtitle_and_opens_subtitle_params() {
        let non_sub_params = ffmpeg::codec::Parameters::new();
        let err = UniversalSubtitleDecoder::from_stream_params(
            &non_sub_params,
            ffmpeg::Rational(1, 1000),
        );
        assert!(err.is_err());

        // Construct valid SUBRIP Parameters and verify from_stream_params opens and decodes
        let mut subrip_params = ffmpeg::codec::Parameters::new();
        // SAFETY: `subrip_params.as_mut_ptr()` points to a valid allocated `AVCodecParameters`.
        unsafe {
            let raw = subrip_params.as_mut_ptr();
            (*raw).codec_type = ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE;
            (*raw).codec_id = ffmpeg::ffi::AVCodecID::AV_CODEC_ID_SUBRIP;
        }
        let mut subrip_dec = UniversalSubtitleDecoder::from_stream_params(
            &subrip_params,
            ffmpeg::Rational(1, 1000),
        )
        .expect("from_stream_params should open SUBRIP decoder");
        let events = subrip_dec
            .decode_packet(b"From stream params", 3_000, Some(1_500))
            .unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DecodedSubtitleEvent::Cue(c) => {
                assert_eq!(c.text, "From stream params");
                assert_eq!(c.start, MediaTime::from_millis(3_000));
                assert_eq!(c.end, MediaTime::from_millis(4_500));
            }
            other => panic!("expected Cue, got {other:?}"),
        }
        subrip_dec.flush();

        // Construct MOV_TEXT Parameters and verify [0x00, 0x00] clear & long UTF-8 decode
        let mut mov_params = ffmpeg::codec::Parameters::new();
        // SAFETY: `mov_params.as_mut_ptr()` points to a valid allocated `AVCodecParameters`.
        unsafe {
            let raw = mov_params.as_mut_ptr();
            (*raw).codec_type = ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE;
            (*raw).codec_id = ffmpeg::ffi::AVCodecID::AV_CODEC_ID_MOV_TEXT;
        }
        let mut mov_dec = UniversalSubtitleDecoder::from_stream_params(
            &mov_params,
            ffmpeg::Rational(1, 1000),
        )
        .expect("from_stream_params should open MOV_TEXT decoder");
        assert_eq!(
            mov_dec.decode_packet(&[0x00, 0x00], 9_000, None).unwrap(),
            vec![DecodedSubtitleEvent::ClearActive { at_ms: 9_000 }]
        );
    }
}
