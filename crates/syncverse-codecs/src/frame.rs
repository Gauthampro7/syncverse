//! Planar YUV420p, packed RGBA, and audio frame memory representations.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fmt;
use std::ops::{Deref, DerefMut};
use crate::error::CodecError;

/// 64-byte aligned heap buffer with end-of-buffer SIMD tail padding.
pub struct AlignedBuffer {
    ptr: *mut u8,
    layout: Layout,
    len: usize,
}

// SAFETY: AlignedBuffer owns unique heap allocation, can be safely sent across threads.
unsafe impl Send for AlignedBuffer {}
// SAFETY: AlignedBuffer provides synchronized immutable access when shared.
unsafe impl Sync for AlignedBuffer {}

impl AlignedBuffer {
    /// Allocates a 64-byte cache-aligned buffer with additional tail padding.
    pub fn new(capacity: usize) -> Self {
        let align = 64;
        let layout = Layout::from_size_align(capacity.max(1), align)
            .expect("invalid 64-byte alignment layout");

        // SAFETY: Allocating zeroed memory using Layout with valid size and 64-byte alignment.
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        Self {
            ptr,
            layout,
            len: capacity,
        }
    }

    /// Returns a slice to the allocated buffer.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: Pointer is non-null, aligned, and valid for `len` bytes.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Returns a mutable slice to the allocated buffer.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: Pointer is non-null, aligned, and valid for `len` bytes.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Returns true if the pointer is aligned to 64 bytes.
    #[inline]
    pub fn is_64_aligned(&self) -> bool {
        (self.ptr as usize).is_multiple_of(64)
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // SAFETY: ptr was allocated with self.layout and is non-null.
        unsafe {
            dealloc(self.ptr, self.layout);
        }
    }
}

impl Deref for AlignedBuffer {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for AlignedBuffer {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl Clone for AlignedBuffer {
    fn clone(&self) -> Self {
        let mut copy = Self::new(self.len);
        copy.copy_from_slice(self.as_slice());
        copy
    }
}

impl fmt::Debug for AlignedBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AlignedBuffer(len={}, align=64, ptr={:p})",
            self.len, self.ptr
        )
    }
}

/// Planar YUV420p (I420) video frame.
///
/// Features:
/// - 64-byte aligned Y, U, and V plane buffers.
/// - 16-byte row stride alignment `(width + 15) & !15`.
/// - +16 bytes end-of-buffer SIMD tail padding to prevent vector over-reads.
#[derive(Debug, Clone)]
pub struct VideoFrameYUV420p {
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Video visible width in pixels.
    pub width: u32,
    /// Video visible height in pixels.
    pub height: u32,
    /// Row stride of luminance (Y) plane in bytes (>= (width + 15) & !15).
    pub stride_y: usize,
    /// Row stride of chroma blue-difference (U) plane in bytes.
    pub stride_u: usize,
    /// Row stride of chroma red-difference (V) plane in bytes.
    pub stride_v: usize,
    /// Luminance plane buffer.
    pub y_plane: AlignedBuffer,
    /// Chroma U (Cb) plane buffer.
    pub u_plane: AlignedBuffer,
    /// Chroma V (Cr) plane buffer.
    pub v_plane: AlignedBuffer,
}

impl VideoFrameYUV420p {
    /// Allocates a new `VideoFrameYUV420p` with aligned strides and SIMD tail padding.
    pub fn new(width: u32, height: u32, pts_ms: u64) -> Self {
        let stride_y = ((width as usize) + 15) & !15;
        let chroma_w = (width as usize).div_ceil(2);
        let stride_u = (chroma_w + 15) & !15;
        let stride_v = stride_u;
        let chroma_h = (height as usize).div_ceil(2);

        // Plane capacity = stride * height + 16 bytes SIMD tail padding
        let y_capacity = stride_y * (height as usize) + 16;
        let u_capacity = stride_u * chroma_h + 16;
        let v_capacity = stride_v * chroma_h + 16;

        Self {
            pts_ms,
            width,
            height,
            stride_y,
            stride_u,
            stride_v,
            y_plane: AlignedBuffer::new(y_capacity),
            u_plane: AlignedBuffer::new(u_capacity),
            v_plane: AlignedBuffer::new(v_capacity),
        }
    }
}

/// Packed 32-bit RGBA presentation frame buffer.
#[derive(Debug, Clone)]
pub struct VideoFrameRGBA {
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Visible width in pixels.
    pub width: u32,
    /// Visible height in pixels.
    pub height: u32,
    /// Row stride in bytes (aligned to 64 bytes).
    pub stride: usize,
    /// Packed pixel buffer `[R, G, B, A]`.
    pub data: AlignedBuffer,
}

impl VideoFrameRGBA {
    /// Allocates a new `VideoFrameRGBA` with 64-byte row alignment and Alpha pre-initialized to 255.
    pub fn new(width: u32, height: u32, pts_ms: u64) -> Self {
        let stride = (((width as usize) * 4) + 63) & !63;
        let total_size = stride * (height as usize);
        let mut buffer = AlignedBuffer::new(total_size);

        // Pre-initialize alpha channel (every 4th byte) to 255
        let slice = buffer.as_mut_slice();
        for y in 0..(height as usize) {
            let row_start = y * stride;
            for x in 0..(width as usize) {
                let pixel_start = row_start + x * 4;
                slice[pixel_start + 3] = 255;
            }
        }

        Self {
            pts_ms,
            width,
            height,
            stride,
            data: buffer,
        }
    }
}

/// Interleaved 32-bit floating-point audio PCM frame representation.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFramePCM {
    /// Audio sampling rate in Hz (e.g. 48,000 or 44,100).
    pub sample_rate: u32,
    /// Number of audio channels (e.g. 2 for stereo).
    pub channels: u16,
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Interleaved normalized audio samples in linear range `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl AudioFramePCM {
    /// Constructs a new `AudioFramePCM`, verifying sample boundedness.
    pub fn new(
        sample_rate: u32,
        channels: u16,
        pts_ms: u64,
        samples: Vec<f32>,
    ) -> Result<Self, CodecError> {
        if sample_rate == 0 || channels == 0 {
            return Err(CodecError::InvalidAudio(
                "sample_rate and channels must be > 0".into(),
            ));
        }
        for &s in &samples {
            if s.is_nan() || !(-1.0..=1.0).contains(&s) {
                return Err(CodecError::InvalidAudio(format!(
                    "sample value {} exceeds [-1.0, 1.0] bounds",
                    s
                )));
            }
        }
        Ok(Self {
            sample_rate,
            channels,
            pts_ms,
            samples,
        })
    }

    /// Computes the effective audio frame duration in seconds.
    #[inline]
    pub fn duration_secs(&self) -> f64 {
        if self.channels == 0 || self.sample_rate == 0 {
            0.0
        } else {
            (self.samples.len() / (self.channels as usize)) as f64 / (self.sample_rate as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yuv420p_stride_and_padding_1080p() {
        let frame = VideoFrameYUV420p::new(1920, 1080, 0);
        assert_eq!(frame.stride_y, 1920);
        assert_eq!(frame.stride_u, 960);
        assert_eq!(frame.stride_v, 960);
        assert!(frame.y_plane.is_64_aligned());
        assert!(frame.u_plane.is_64_aligned());
        assert!(frame.v_plane.is_64_aligned());

        // Check tail padding: size must be stride * height + 16
        assert_eq!(frame.y_plane.len(), 1920 * 1080 + 16);
        assert_eq!(frame.u_plane.len(), 960 * 540 + 16);
    }

    #[test]
    fn test_yuv420p_stride_480p_unaligned() {
        let frame = VideoFrameYUV420p::new(854, 480, 0);
        assert_eq!(frame.stride_y, 864); // (854 + 15) & !15 = 864
        assert_eq!(frame.stride_u, 432); // ((854+1)/2 = 427; (427+15)&!15 = 432)
        assert_eq!(frame.stride_v, 432);
        assert_eq!(frame.y_plane.len(), 864 * 480 + 16);
    }

    #[test]
    fn test_rgba_frame_geometry() {
        let frame = VideoFrameRGBA::new(1920, 1080, 0);
        assert_eq!(frame.stride, 1920 * 4); // 7680 (multiple of 64)
        assert_eq!(frame.data.len(), 7680 * 1080);
        assert_eq!(frame.data[3], 255); // Alpha initialized
        assert_eq!(frame.data[7], 255);
    }

    #[test]
    fn test_audio_pcm_frame() {
        let samples = vec![0.0f32; 1920]; // 960 stereo pairs
        let frame = AudioFramePCM::new(48000, 2, 0, samples).unwrap();
        assert!((frame.duration_secs() - 0.020).abs() < 1e-6);
    }
}
