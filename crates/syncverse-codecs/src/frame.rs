//! Planar YUV420p, packed RGBA, and audio frame memory representations.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use crate::error::CodecError;

/// 64-byte aligned heap buffer with end-of-buffer SIMD tail padding.
pub struct AlignedBuffer {
    ptr: *mut u8,
    layout: Layout,
    len: usize,
    cap: usize,
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
            cap: capacity,
        }
    }

    /// Returns the underlying allocated byte capacity of this buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Adjusts the logical byte length within the already-allocated capacity without reallocating.
    #[inline]
    pub(crate) fn set_len_within_capacity(&mut self, new_len: usize) {
        assert!(
            new_len <= self.cap,
            "new_len {} exceeds AlignedBuffer capacity {}",
            new_len,
            self.cap
        );
        self.len = new_len;
    }

    /// Returns a mutable 32-bit slice view when the buffer length is a multiple of 4 bytes.
    #[inline]
    pub(crate) fn as_mut_u32_slice(&mut self) -> &mut [u32] {
        if self.len == 0 {
            return &mut [];
        }
        assert!(
            self.len.is_multiple_of(4),
            "AlignedBuffer length {} must be a multiple of 4 for u32 view",
            self.len
        );
        // SAFETY:
        // - `self.ptr` is non-null and 64-byte aligned (therefore strictly 4-byte aligned).
        // - `self.len` is verified to be a multiple of 4 and `self.len <= self.cap <= self.layout.size()`.
        // - `&mut self` guarantees exclusive mutable access for the returned slice lifetime.
        unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u32, self.len / 4) }
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
    /// Seek/epoch generation counter used to discard stale frames after seeks or track switches.
    pub generation: u64,
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
    /// Computes the 64-byte aligned row stride and total byte size for an RGBA frame.
    #[inline]
    fn geometry(width: u32, height: u32) -> (usize, usize) {
        let stride = (((width as usize) * 4) + 63) & !63;
        let total_size = stride * (height as usize);
        (stride, total_size)
    }

    /// Allocates a new `VideoFrameRGBA` with 64-byte row alignment and Alpha pre-initialized to 255.
    pub fn new(width: u32, height: u32, pts_ms: u64) -> Self {
        let (stride, total_size) = Self::geometry(width, height);
        let mut buffer = AlignedBuffer::new(total_size);

        if width > 0 && height > 0 {
            // Pre-initialize alpha channel to 255 in a vectorized 32-bit pass:
            // [R=0, G=0, B=0, A=255] == 0xFF00_0000 in little-endian u32 representation.
            let alpha_pixel = 0xFF00_0000_u32.to_le();
            let w = width as usize;
            let stride_u32 = stride / 4;
            let u32_slice = buffer.as_mut_u32_slice();
            if stride_u32 == w {
                u32_slice.fill(alpha_pixel);
            } else {
                for row in u32_slice.chunks_exact_mut(stride_u32) {
                    row[..w].fill(alpha_pixel);
                }
            }
        }

        Self {
            pts_ms,
            generation: 0,
            width,
            height,
            stride,
            data: buffer,
        }
    }

    /// Allocates a zeroed `VideoFrameRGBA` without filling alpha bytes.
    ///
    /// Intended for decoders and color converters (`sws_scale`, `convert_yuv420p_to_rgba`)
    /// that immediately overwrite every RGBA pixel in the frame.
    pub fn new_uninit_zeroed(width: u32, height: u32, pts_ms: u64) -> Self {
        let (stride, total_size) = Self::geometry(width, height);
        let buffer = AlignedBuffer::new(total_size);

        Self {
            pts_ms,
            generation: 0,
            width,
            height,
            stride,
            data: buffer,
        }
    }

    /// Builder method tagging this frame with a seek/epoch `generation`.
    #[inline]
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }

    /// Updates the seek/epoch `generation` tag in place.
    #[inline]
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }
}

/// Default maximum number of pooled RGBA buffers retained by [`RgbaFramePool`].
const DEFAULT_RGBA_POOL_CAPACITY: usize = 48;

#[derive(Debug)]
struct RgbaFramePoolInner {
    buffers: Mutex<Vec<AlignedBuffer>>,
    max_buffers: usize,
}

/// Thread-safe bounded pool of 64-byte aligned RGBA frame buffers.
///
/// Reuses `AlignedBuffer` heap allocations across 1080p/4K video playback so the
/// decoder avoids per-frame `mmap`/`munmap` kernel page faults. Cloning an
/// `RgbaFramePool` shares the same underlying pool via `Arc`.
#[derive(Debug, Clone)]
pub struct RgbaFramePool {
    inner: Arc<RgbaFramePoolInner>,
}

impl Default for RgbaFramePool {
    fn default() -> Self {
        Self::new(DEFAULT_RGBA_POOL_CAPACITY)
    }
}

impl RgbaFramePool {
    /// Creates a new `RgbaFramePool` retaining up to `max_buffers` recycled buffers.
    pub fn new(max_buffers: usize) -> Self {
        Self {
            inner: Arc::new(RgbaFramePoolInner {
                buffers: Mutex::new(Vec::with_capacity(max_buffers.min(DEFAULT_RGBA_POOL_CAPACITY))),
                max_buffers,
            }),
        }
    }

    /// Acquires a `VideoFrameRGBA` backed by a pooled `AlignedBuffer` when one with
    /// sufficient capacity is available, or allocates a new 64-byte aligned buffer otherwise.
    pub fn acquire(&self, width: u32, height: u32, pts_ms: u64, generation: u64) -> VideoFrameRGBA {
        let (stride, total_size) = VideoFrameRGBA::geometry(width, height);
        if total_size == 0 {
            return VideoFrameRGBA::new_uninit_zeroed(width, height, pts_ms)
                .with_generation(generation);
        }

        let reused = {
            let mut guard = self
                .inner
                .buffers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(idx) = guard.iter().rposition(|buf| buf.capacity() >= total_size) {
                let mut buf = guard.swap_remove(idx);
                buf.set_len_within_capacity(total_size);
                Some(buf)
            } else {
                None
            }
        };

        let data = reused.unwrap_or_else(|| AlignedBuffer::new(total_size));
        VideoFrameRGBA {
            pts_ms,
            generation,
            width,
            height,
            stride,
            data,
        }
    }

    /// Returns a `VideoFrameRGBA`'s underlying `AlignedBuffer` to the pool for reuse.
    ///
    /// If the pool is at capacity, smaller stale buffers are evicted first so the pool
    /// adapts smoothly to resolution upgrades; excess buffers are dropped outside the lock.
    pub fn recycle(&self, frame: VideoFrameRGBA) {
        let cap = frame.data.capacity();
        if cap == 0 || self.inner.max_buffers == 0 {
            return;
        }

        let evicted: Option<AlignedBuffer> = {
            let mut guard = self
                .inner
                .buffers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if guard.len() < self.inner.max_buffers {
                guard.push(frame.data);
                None
            } else if let Some((min_idx, _)) = guard
                .iter()
                .enumerate()
                .min_by_key(|(_, b)| b.capacity())
                .filter(|(_, b)| b.capacity() < cap)
            {
                let old = std::mem::replace(&mut guard[min_idx], frame.data);
                Some(old)
            } else {
                Some(frame.data)
            }
        };
        drop(evicted);
    }

    /// Returns the number of buffers currently available in the pool.
    pub fn len(&self) -> usize {
        self.inner
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Returns `true` if the pool currently holds zero buffers.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the maximum number of buffers this pool can retain.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.max_buffers
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
        assert_eq!(frame.generation, 0);
        assert_eq!(frame.data[3], 255); // Alpha initialized
        assert_eq!(frame.data[7], 255);
        assert_eq!(frame.data[frame.data.len() - 1], 255);
    }

    #[test]
    fn test_rgba_frame_odd_width_and_generation() {
        let mut frame = VideoFrameRGBA::new(854, 480, 123).with_generation(7);
        assert_eq!(frame.pts_ms, 123);
        assert_eq!(frame.generation, 7);
        assert_eq!(frame.stride, 3456); // (854*4 + 63) & !63 = 3456
        // Verify alpha=255 for active pixels and 0 in stride padding
        assert_eq!(frame.data[853 * 4 + 3], 255);
        assert_eq!(frame.data[854 * 4 + 3], 0);
        assert_eq!(frame.data[frame.stride + 853 * 4 + 3], 255);

        frame.set_generation(9);
        assert_eq!(frame.generation, 9);

        let uninit = VideoFrameRGBA::new_uninit_zeroed(64, 64, 50).with_generation(3);
        assert_eq!(uninit.generation, 3);
        assert_eq!(uninit.data[3], 0);
        assert!(uninit.data.is_64_aligned());
    }

    #[test]
    fn test_rgba_frame_pool_reuse_and_bounds() {
        let pool = RgbaFramePool::new(2);
        assert_eq!(pool.capacity(), 2);
        assert!(pool.is_empty());

        let f1 = pool.acquire(1920, 1080, 100, 1);
        let ptr1 = f1.data.as_ptr() as usize;
        assert_eq!(f1.generation, 1);
        assert_eq!(f1.pts_ms, 100);
        assert!(f1.data.is_64_aligned());

        pool.recycle(f1);
        assert_eq!(pool.len(), 1);

        // Re-acquiring same or smaller size reuses the exact same allocation pointer!
        let f2 = pool.acquire(1280, 720, 200, 2);
        assert_eq!(f2.data.as_ptr() as usize, ptr1);
        assert_eq!(f2.data.len(), f2.stride * 720);
        assert_eq!(f2.generation, 2);
        assert_eq!(f2.pts_ms, 200);
        assert!(pool.is_empty());

        // Returning f2 and upgrading to 4K allocates a larger buffer, and pool retains larger
        pool.recycle(f2);
        let f3 = pool.acquire(3840, 2160, 300, 3);
        assert_ne!(f3.data.as_ptr() as usize, ptr1);
        assert_eq!(pool.len(), 1); // 1080p buffer stayed in pool since 4K needed larger

        // Pool capacity bound respected
        let f4 = pool.acquire(640, 480, 400, 4); // reuses the 1080p buffer
        let f5 = pool.acquire(640, 480, 500, 5);
        pool.recycle(f3);
        pool.recycle(f4);
        pool.recycle(f5);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn test_audio_pcm_frame() {
        let samples = vec![0.0f32; 1920]; // 960 stereo pairs
        let frame = AudioFramePCM::new(48000, 2, 0, samples).unwrap();
        assert!((frame.duration_secs() - 0.020).abs() < 1e-6);
    }
}
