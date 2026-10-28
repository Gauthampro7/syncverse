//! Universal video bitstream decoder powered by FFmpeg (`ffmpeg-next`).
//!
//! Provides hardware-accelerated and software decoding for:
//! - H.264 (AVC)
//! - H.265 (HEVC)
//! - VP8
//! - VP9
//! - AV1
//! - MPEG-2
//! - MPEG-4 Part 2
//! - VC-1
//!
//! Decoded frames are mapped into cache-line aligned `VideoFrameYUV420p` buffers
//! (64-byte aligned planar heap buffers, 16-byte aligned row strides, +16B SIMD tail padding),
//! and converted to packed `VideoFrameRGBA` via in-tree Q8 fixed-point integer arithmetic or swscale.

use crate::color::{convert_yuv420p_to_rgba, ColorStandard};
use crate::error::CodecError;
use crate::frame::{VideoFrameRGBA, VideoFrameYUV420p};
use ffmpeg_next as ffmpeg;
use ffmpeg::software::scaling::{context::Context as ScalerContext, flag::Flags};

/// Universal video decoder supporting H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, and VC-1.
pub struct UniversalVideoDecoder {
    /// Underlying opened FFmpeg video decoder context.
    decoder: ffmpeg::decoder::Video,
    /// Lazily initialized swscale context for format normalization (e.g. NV12/10-bit -> YUV420P).
    scaler: Option<ScalerContext>,
    /// Last pixel format configured in the swscale scaler context.
    last_scaler_format: Option<ffmpeg::format::Pixel>,
    /// Last width configured in the swscale scaler context.
    last_scaler_width: u32,
    /// Last height configured in the swscale scaler context.
    last_scaler_height: u32,
    /// Intermediate frame buffer for format conversion.
    intermediate_frame: ffmpeg::util::frame::Video,
    /// Active color standard (BT.601 / BT.709).
    color_standard: ColorStandard,
    /// Tracked presentation timestamp of last processed packet or frame.
    last_pts_ms: u64,
}

impl UniversalVideoDecoder {
    /// Initializes a new universal video decoder from FFmpeg codec parameters.
    pub fn from_parameters(params: &ffmpeg::codec::Parameters) -> Result<Self, CodecError> {
        let mut ctx = ffmpeg::codec::context::Context::from_parameters(params.clone())
            .map_err(|e| CodecError::Decoder(format!("failed to create context from parameters: {e}")))?;

        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        let decoder = ctx.decoder().video()
            .map_err(|e| CodecError::Decoder(format!("failed to open video decoder: {e}")))?;
        let standard = Self::infer_color_standard(&decoder);

        Ok(Self {
            decoder,
            scaler: None,
            last_scaler_format: None,
            last_scaler_width: 0,
            last_scaler_height: 0,
            intermediate_frame: ffmpeg::util::frame::Video::empty(),
            color_standard: standard,
            last_pts_ms: 0,
        })
    }

    /// Initializes a universal video decoder for a given codec ID, width, height, and optional extradata.
    pub fn from_codec_id(
        id: ffmpeg::codec::Id,
        width: Option<u32>,
        height: Option<u32>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, CodecError> {
        let codec = ffmpeg::decoder::find(id)
            .ok_or_else(|| CodecError::UnsupportedCodec(format!("no FFmpeg decoder found for codec ID {id:?}")))?;
        let mut ctx = ffmpeg::codec::context::Context::new_with_codec(codec);

        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });

        // SAFETY: Context pointer is non-null and freshly allocated by Context::new_with_codec.
        // We configure initial width/height and extradata before calling avcodec_open2.
        unsafe {
            let raw = ctx.as_mut_ptr();
            if let Some(w) = width {
                (*raw).width = w as std::os::raw::c_int;
            }
            if let Some(h) = height {
                (*raw).height = h as std::os::raw::c_int;
            }

            // Set extradata if provided, or provide standard VC-1 fallback header if required
            let extra_slice: Option<&[u8]> = extradata.or_else(|| {
                if id == ffmpeg::codec::Id::VC1 {
                    // Standard 16-byte VC-1 Advanced Profile sequence header fallback:
                    // 00 00 01 0F (seq header) + 0xCA (profile 3, level 1, 4:2:0 chroma) + 00 00 01 0E (entry point)
                    static VC1_FALLBACK: [u8; 16] = [
                        0x00, 0x00, 0x01, 0x0f,
                        0xca, 0x80, 0x00, 0x00,
                        0x00, 0x00, 0x01, 0x0e,
                        0x00, 0x00, 0x00, 0x00,
                    ];
                    Some(&VC1_FALLBACK[..])
                } else {
                    None
                }
            });

            if let Some(extra) = extra_slice {
                if !extra.is_empty() {
                    let padded_len = extra.len() + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
                    let buf = ffmpeg::ffi::av_malloc(padded_len) as *mut u8;
                    if !buf.is_null() {
                        std::ptr::copy_nonoverlapping(extra.as_ptr(), buf, extra.len());
                        std::ptr::write_bytes(buf.add(extra.len()), 0, ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize);
                        (*raw).extradata = buf;
                        (*raw).extradata_size = extra.len() as std::os::raw::c_int;
                    }
                }
            }
        }

        let decoder = ctx.decoder().video()
            .map_err(|e| CodecError::Decoder(format!("failed to open video decoder for {id:?}: {e}")))?;
        let standard = Self::infer_color_standard(&decoder);

        Ok(Self {
            decoder,
            scaler: None,
            last_scaler_format: None,
            last_scaler_width: 0,
            last_scaler_height: 0,
            intermediate_frame: ffmpeg::util::frame::Video::empty(),
            color_standard: standard,
            last_pts_ms: 0,
        })
    }

    /// Maps codec string name or FourCC into `ffmpeg::codec::Id`.
    pub fn map_codec_name(name: &str) -> Option<ffmpeg::codec::Id> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "h264" | "avc" | "avc1" | "avc3" | "x264" | "v_mpeg4/iso/avc" => {
                Some(ffmpeg::codec::Id::H264)
            }
            "hevc" | "h265" | "hev1" | "hvc1" | "x265" | "v_mpegh/iso/hevc" => {
                Some(ffmpeg::codec::Id::HEVC)
            }
            "vp8" | "vp80" | "v_vp8" => Some(ffmpeg::codec::Id::VP8),
            "vp9" | "vp90" | "v_vp9" => Some(ffmpeg::codec::Id::VP9),
            "av1" | "av01" | "v_av1" => Some(ffmpeg::codec::Id::AV1),
            "mpeg2" | "mpeg2video" | "mp2v" | "mpg2" | "v_mpeg2" => {
                Some(ffmpeg::codec::Id::MPEG2VIDEO)
            }
            "mpeg4" | "mp4v" | "xvid" | "divx" | "v_mpeg4/iso/asp" => {
                Some(ffmpeg::codec::Id::MPEG4)
            }
            "vc1" | "wvc1" | "wmv3" | "v_ms/vfw/fourcc" => Some(ffmpeg::codec::Id::VC1),
            _ => ffmpeg::decoder::find_by_name(&lower).map(|d| d.id()),
        }
    }

    /// Initializes a universal video decoder by matching a track codec identifier.
    pub fn from_codec_name(
        name: &str,
        width: Option<u32>,
        height: Option<u32>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, CodecError> {
        let codec_id = Self::map_codec_name(name)
            .ok_or_else(|| CodecError::UnsupportedCodec(format!("unrecognized video codec name: '{name}'")))?;
        Self::from_codec_id(codec_id, width, height, extradata)
    }

    /// Sends a compressed packet payload to the video decoder.
    pub fn send_packet(
        &mut self,
        data: &[u8],
        pts_ms: u64,
        dts_ms: Option<u64>,
        is_keyframe: bool,
    ) -> Result<(), CodecError> {
        let mut packet = ffmpeg::Packet::copy(data);
        packet.set_pts(Some(pts_ms as i64));
        if let Some(dts) = dts_ms {
            packet.set_dts(Some(dts as i64));
        }
        if is_keyframe {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
        }

        self.last_pts_ms = pts_ms;
        self.decoder.send_packet(&packet)
            .map_err(|e| CodecError::Decoder(format!("send_packet error: {e}")))
    }

    /// Sends raw FFmpeg packet to the video decoder.
    pub fn send_raw_packet(&mut self, packet: &ffmpeg::Packet) -> Result<(), CodecError> {
        if let Some(pts) = packet.pts() {
            self.last_pts_ms = pts.max(0) as u64;
        }
        self.decoder.send_packet(packet)
            .map_err(|e| CodecError::Decoder(format!("send_raw_packet error: {e}")))
    }

    /// Receives the next decoded video frame mapped into memory-aligned `VideoFrameYUV420p`.
    pub fn receive_frame(&mut self) -> Result<Option<VideoFrameYUV420p>, CodecError> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts_ms = decoded.pts().map(|p| p.max(0) as u64).unwrap_or(self.last_pts_ms);
                let frame = self.map_frame_to_yuv420p(&decoded, pts_ms)?;
                Ok(Some(frame))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(CodecError::Decoder(format!("receive_frame error: {e}"))),
        }
    }

    /// Receives the next decoded video frame converted to packed `VideoFrameRGBA` via in-tree Q8 conversion.
    pub fn receive_frame_rgba(&mut self) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.receive_preroll_or_rgba(None)
    }

    /// Receives the next decoded video frame, discarding preroll frames before `min_pts_ms` without
    /// performing expensive pixel format or color space conversions.
    pub fn receive_preroll_or_rgba(
        &mut self,
        min_pts_ms: Option<u64>,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        let mut decoded = ffmpeg::util::frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                let pts_ms = decoded.pts().map(|p| p.max(0) as u64).unwrap_or(self.last_pts_ms);
                if let Some(min_pts) = min_pts_ms {
                    if pts_ms + 15 < min_pts {
                        return Ok(None);
                    }
                }
                let yuv = self.map_frame_to_yuv420p(&decoded, pts_ms)?;
                let mut rgba = VideoFrameRGBA::new(yuv.width, yuv.height, yuv.pts_ms);
                convert_yuv420p_to_rgba(&yuv, &mut rgba, self.color_standard)?;
                Ok(Some(rgba))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(CodecError::Decoder(format!("receive_frame error: {e}"))),
        }
    }

    /// Decodes a packet and returns a reconstructed `VideoFrameYUV420p` if ready.
    pub fn decode_packet(
        &mut self,
        data: &[u8],
        pts_ms: u64,
    ) -> Result<Option<VideoFrameYUV420p>, CodecError> {
        self.send_packet(data, pts_ms, None, false)?;
        self.receive_frame()
    }

    /// Decodes a packet and returns a reconstructed `VideoFrameRGBA` if ready.
    pub fn decode_packet_rgba(
        &mut self,
        data: &[u8],
        pts_ms: u64,
    ) -> Result<Option<VideoFrameRGBA>, CodecError> {
        self.send_packet(data, pts_ms, None, false)?;
        self.receive_frame_rgba()
    }

    /// Drains all buffered frames from the decoder at end-of-stream.
    pub fn drain(&mut self) -> Result<Vec<VideoFrameYUV420p>, CodecError> {
        self.decoder.send_eof()
            .map_err(|e| CodecError::Decoder(format!("send_eof error: {e}")))?;
        let mut frames = Vec::new();
        while let Some(frame) = self.receive_frame()? {
            frames.push(frame);
        }
        Ok(frames)
    }

    /// Flushes internal decoder buffers (call immediately after random-access seek).
    pub fn flush(&mut self) {
        self.decoder.flush();
        self.scaler = None;
        self.last_scaler_format = None;
        self.last_scaler_width = 0;
        self.last_scaler_height = 0;
    }

    /// Visible width in pixels.
    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    /// Visible height in pixels.
    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Active color standard.
    pub fn color_standard(&self) -> ColorStandard {
        self.color_standard
    }

    /// Sets explicit color standard.
    pub fn set_color_standard(&mut self, standard: ColorStandard) {
        self.color_standard = standard;
    }

    /// Infers color standard from decoder context color space, falling back to ITU-R 709 for HD.
    fn infer_color_standard(decoder: &ffmpeg::decoder::Video) -> ColorStandard {
        match decoder.color_space() {
            ffmpeg::color::Space::BT709 => ColorStandard::Bt709,
            ffmpeg::color::Space::BT470BG
            | ffmpeg::color::Space::SMPTE170M
            | ffmpeg::color::Space::SMPTE240M => ColorStandard::Bt601,
            _ => {
                if decoder.height() >= 720 {
                    ColorStandard::Bt709
                } else {
                    ColorStandard::Bt601
                }
            }
        }
    }

    /// Maps an FFmpeg decoded video frame into in-tree `VideoFrameYUV420p`
    /// strictly enforcing:
    /// - 64-byte aligned planar heap buffers (`y_plane`, `u_plane`, `v_plane`).
    /// - 16-byte aligned row strides (`(width + 15) & !15`).
    /// - +16B SIMD tail padding at the end of each plane buffer.
    pub fn map_frame_to_yuv420p(
        &mut self,
        src: &ffmpeg::util::frame::Video,
        pts_ms: u64,
    ) -> Result<VideoFrameYUV420p, CodecError> {
        let width = src.width();
        let height = src.height();

        let yuv_src = if src.format() == ffmpeg::format::Pixel::YUV420P
            || src.format() == ffmpeg::format::Pixel::YUVJ420P
        {
            src
        } else {
            // Lazy format conversion via swscale if source is NV12, P010, RGB, etc.
            let needs_new_scaler = self.scaler.is_none()
                || self.last_scaler_format != Some(src.format())
                || self.last_scaler_width != width
                || self.last_scaler_height != height;

            if needs_new_scaler {
                let s = ScalerContext::get(
                    src.format(),
                    width,
                    height,
                    ffmpeg::format::Pixel::YUV420P,
                    width,
                    height,
                    Flags::BILINEAR,
                ).map_err(|e| CodecError::Decoder(format!("swscale context initialization failed: {e}")))?;
                self.scaler = Some(s);
                self.last_scaler_format = Some(src.format());
                self.last_scaler_width = width;
                self.last_scaler_height = height;
                self.intermediate_frame = ffmpeg::util::frame::Video::empty();
            }
            let s = self.scaler.as_mut().unwrap();
            s.run(src, &mut self.intermediate_frame)
                .map_err(|e| CodecError::Decoder(format!("swscale frame conversion failed: {e}")))?;
            &self.intermediate_frame
        };

        let mut out = VideoFrameYUV420p::new(width, height, pts_ms);

        // Copy Y luminance plane row-by-row
        let src_y = yuv_src.data(0);
        let src_stride_y = yuv_src.stride(0);
        let dst_y = out.y_plane.as_mut_slice();
        let dst_stride_y = out.stride_y;
        let w_usize = width as usize;

        for r in 0..(height as usize) {
            let s_start = r * src_stride_y;
            let d_start = r * dst_stride_y;
            dst_y[d_start..d_start + w_usize].copy_from_slice(&src_y[s_start..s_start + w_usize]);
            if dst_stride_y > w_usize {
                dst_y[d_start + w_usize..d_start + dst_stride_y].fill(0);
            }
        }
        let y_cap = dst_y.len();
        dst_y[y_cap - 16..y_cap].fill(0);

        // Copy U and V chroma planes row-by-row
        let chroma_w = (width as usize).div_ceil(2);
        let chroma_h = (height as usize).div_ceil(2);

        let src_u = yuv_src.data(1);
        let src_stride_u = yuv_src.stride(1);
        let dst_u = out.u_plane.as_mut_slice();
        let dst_stride_u = out.stride_u;

        for r in 0..chroma_h {
            let s_start = r * src_stride_u;
            let d_start = r * dst_stride_u;
            dst_u[d_start..d_start + chroma_w].copy_from_slice(&src_u[s_start..s_start + chroma_w]);
            if dst_stride_u > chroma_w {
                dst_u[d_start + chroma_w..d_start + dst_stride_u].fill(0);
            }
        }
        let u_cap = dst_u.len();
        dst_u[u_cap - 16..u_cap].fill(0);

        let src_v = yuv_src.data(2);
        let src_stride_v = yuv_src.stride(2);
        let dst_v = out.v_plane.as_mut_slice();
        let dst_stride_v = out.stride_v;

        for r in 0..chroma_h {
            let s_start = r * src_stride_v;
            let d_start = r * dst_stride_v;
            dst_v[d_start..d_start + chroma_w].copy_from_slice(&src_v[s_start..s_start + chroma_w]);
            if dst_stride_v > chroma_w {
                dst_v[d_start + chroma_w..d_start + dst_stride_v].fill(0);
            }
        }
        let v_cap = dst_v.len();
        dst_v[v_cap - 16..v_cap].fill(0);

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_instantiation_all_codecs() {
        ffmpeg::init().expect("ffmpeg init");

        let test_codecs = [
            ("h264", ffmpeg::codec::Id::H264),
            ("hevc", ffmpeg::codec::Id::HEVC),
            ("vp8", ffmpeg::codec::Id::VP8),
            ("vp9", ffmpeg::codec::Id::VP9),
            ("av1", ffmpeg::codec::Id::AV1),
            ("mpeg2video", ffmpeg::codec::Id::MPEG2VIDEO),
            ("mpeg4", ffmpeg::codec::Id::MPEG4),
            ("vc1", ffmpeg::codec::Id::VC1),
        ];

        for (name, id) in &test_codecs {
            let dec = UniversalVideoDecoder::from_codec_id(*id, Some(640), Some(480), None);
            assert!(dec.is_ok(), "Failed to instantiate decoder for {name} ({id:?}): {:?}", dec.err());
            assert_eq!(UniversalVideoDecoder::map_codec_name(name), Some(*id));
        }
    }

    #[test]
    fn test_yuv420p_mapping_alignments() {
        ffmpeg::init().expect("ffmpeg init");

        let mut dec = UniversalVideoDecoder::from_codec_id(ffmpeg::codec::Id::H264, Some(854), Some(480), None)
            .expect("init decoder");

        // Simulate NV12 frame to test swscale conversion path
        let nv12 = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::NV12, 854, 480);
        let yuv = dec.map_frame_to_yuv420p(&nv12, 1234).expect("map nv12");

        assert_eq!(yuv.width, 854);
        assert_eq!(yuv.height, 480);
        assert_eq!(yuv.pts_ms, 1234);

        // Invariant 1: 64-byte aligned planar buffers
        assert!(yuv.y_plane.is_64_aligned(), "Y plane must be 64B aligned");
        assert!(yuv.u_plane.is_64_aligned(), "U plane must be 64B aligned");
        assert!(yuv.v_plane.is_64_aligned(), "V plane must be 64B aligned");

        // Invariant 2: 16-byte row stride alignment ((width + 15) & !15)
        assert_eq!(yuv.stride_y, 864);
        assert_eq!(yuv.stride_u, 432);
        assert_eq!(yuv.stride_v, 432);
        assert_eq!(yuv.stride_y % 16, 0);
        assert_eq!(yuv.stride_u % 16, 0);
        assert_eq!(yuv.stride_v % 16, 0);

        // Invariant 3: +16B SIMD tail padding
        assert_eq!(yuv.y_plane.len(), 864 * 480 + 16);
        assert_eq!(yuv.u_plane.len(), 432 * 240 + 16);
        assert_eq!(yuv.v_plane.len(), 432 * 240 + 16);
    }
}
