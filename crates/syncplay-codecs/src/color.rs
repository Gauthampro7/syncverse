//! Pure-Rust Q8 fixed-point integer color space converter (ITU-R BT.601 and BT.709 limited-range).

use crate::error::CodecError;
use crate::frame::{VideoFrameRGBA, VideoFrameYUV420p};

/// Colorimetry standard for YUV to RGB color conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorStandard {
    /// Standard Definition (SDTV) ITU-R BT.601 limited-range.
    Bt601,
    /// High Definition (HDTV) ITU-R BT.709 limited-range.
    Bt709,
}

/// Formal trait for color space conversion from planar YUV420p to packed RGBA.
pub trait ColorSpaceConverter: Send + Sync {
    /// Converts a planar YUV420p frame into a packed RGBA presentation frame.
    fn convert_frame(
        &self,
        src: &VideoFrameYUV420p,
        dst: &mut VideoFrameRGBA,
    ) -> Result<(), CodecError>;

    /// Returns the color standard supported by this converter.
    fn standard(&self) -> ColorStandard;
}

/// Converter for ITU-R BT.601 limited-range color space with 2x2 chroma reuse.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bt601Converter;

impl Bt601Converter {
    /// Creates a new `Bt601Converter`.
    pub const fn new() -> Self {
        Self
    }
}

impl ColorSpaceConverter for Bt601Converter {
    fn convert_frame(
        &self,
        src: &VideoFrameYUV420p,
        dst: &mut VideoFrameRGBA,
    ) -> Result<(), CodecError> {
        convert_yuv420p_to_rgba(src, dst, ColorStandard::Bt601)
    }

    fn standard(&self) -> ColorStandard {
        ColorStandard::Bt601
    }
}

/// Converter for ITU-R BT.709 limited-range color space with 2x2 chroma reuse.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bt709Converter;

impl Bt709Converter {
    /// Creates a new `Bt709Converter`.
    pub const fn new() -> Self {
        Self
    }
}

impl ColorSpaceConverter for Bt709Converter {
    fn convert_frame(
        &self,
        src: &VideoFrameYUV420p,
        dst: &mut VideoFrameRGBA,
    ) -> Result<(), CodecError> {
        convert_yuv420p_to_rgba(src, dst, ColorStandard::Bt709)
    }

    fn standard(&self) -> ColorStandard {
        ColorStandard::Bt709
    }
}

/// Branchless saturation clamping of signed 32-bit integer to unsigned 8-bit integer [0, 255].
#[inline(always)]
pub fn clamp_u8(val: i32) -> u8 {
    val.clamp(0, 255) as u8
}

/// Computes the shared BT.601 chroma terms (R_chroma, G_chroma, B_chroma) for a given (U, V) pair.
#[inline(always)]
fn chroma_terms_bt601(u: u8, v: u8) -> (i32, i32, i32) {
    let d = (u as i32) - 128;
    let e = (v as i32) - 128;
    (
        409 * e + 128,
        -100 * d - 208 * e + 128,
        516 * d + 128,
    )
}

/// Computes the shared BT.709 chroma terms (R_chroma, G_chroma, B_chroma) for a given (U, V) pair.
#[inline(always)]
fn chroma_terms_bt709(u: u8, v: u8) -> (i32, i32, i32) {
    let d = (u as i32) - 128;
    let e = (v as i32) - 128;
    (
        459 * e + 128,
        -55 * d - 136 * e + 128,
        541 * d + 128,
    )
}

/// Converts a single YUV pixel (limited-range) to RGB using ITU-R BT.601 fixed-point Q8 arithmetic.
#[inline(always)]
pub fn yuv_to_rgb_bt601(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32) - 16;
    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt601(u, v);
    let y_term = 298 * c;

    let r = clamp_u8((y_term + r_chroma) >> 8);
    let g = clamp_u8((y_term + g_chroma) >> 8);
    let b = clamp_u8((y_term + b_chroma) >> 8);
    (r, g, b)
}

/// Converts a single YUV pixel (limited-range) to RGB using ITU-R BT.709 fixed-point Q8 arithmetic.
#[inline(always)]
pub fn yuv_to_rgb_bt709(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let c = (y as i32) - 16;
    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt709(u, v);
    let y_term = 298 * c;

    let r = clamp_u8((y_term + r_chroma) >> 8);
    let g = clamp_u8((y_term + g_chroma) >> 8);
    let b = clamp_u8((y_term + b_chroma) >> 8);
    (r, g, b)
}

/// Converts a planar YUV420p frame into a packed RGBA presentation frame using Q8 integer arithmetic.
///
/// This hot loop performs zero heap allocations and uses an unrolled 2x2 chroma reuse kernel
/// to process 4 luma pixels per chroma load, drastically accelerating throughput while maintaining
/// exact mathematical equivalence.
pub fn convert_yuv420p_to_rgba(
    src: &VideoFrameYUV420p,
    dst: &mut VideoFrameRGBA,
    standard: ColorStandard,
) -> Result<(), CodecError> {
    if src.width != dst.width || src.height != dst.height {
        return Err(CodecError::DimensionMismatch {
            src_w: src.width,
            src_h: src.height,
            dst_w: dst.width,
            dst_h: dst.height,
        });
    }

    let width = src.width as usize;
    let height = src.height as usize;
    let y_plane = src.y_plane.as_slice();
    let u_plane = src.u_plane.as_slice();
    let v_plane = src.v_plane.as_slice();
    let dst_data = dst.data.as_mut_slice();

    let stride_y = src.stride_y;
    let stride_u = src.stride_u;
    let stride_v = src.stride_v;
    let dst_stride = dst.stride;

    let half_h = height / 2;
    let half_w = width / 2;

    match standard {
        ColorStandard::Bt601 => {
            // Main unrolled 2x2 blocks with chroma reuse
            for by in 0..half_h {
                let y0 = by * 2;
                let y1 = y0 + 1;
                let y0_offset = y0 * stride_y;
                let y1_offset = y1 * stride_y;
                let uv_row_offset = by * stride_u;
                let v_row_offset = by * stride_v;
                let dst0_offset = y0 * dst_stride;
                let dst1_offset = y1 * dst_stride;

                for bx in 0..half_w {
                    let x0 = bx * 2;
                    let x1 = x0 + 1;

                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt601(u_val, v_val);

                    // Pixel (x0, y0)
                    let c00 = (y_plane[y0_offset + x0] as i32) - 16;
                    let y_term00 = 298 * c00;
                    let px00 = dst0_offset + x0 * 4;
                    dst_data[px00] = clamp_u8((y_term00 + r_chroma) >> 8);
                    dst_data[px00 + 1] = clamp_u8((y_term00 + g_chroma) >> 8);
                    dst_data[px00 + 2] = clamp_u8((y_term00 + b_chroma) >> 8);
                    dst_data[px00 + 3] = 255;

                    // Pixel (x1, y0)
                    let c10 = (y_plane[y0_offset + x1] as i32) - 16;
                    let y_term10 = 298 * c10;
                    let px10 = dst0_offset + x1 * 4;
                    dst_data[px10] = clamp_u8((y_term10 + r_chroma) >> 8);
                    dst_data[px10 + 1] = clamp_u8((y_term10 + g_chroma) >> 8);
                    dst_data[px10 + 2] = clamp_u8((y_term10 + b_chroma) >> 8);
                    dst_data[px10 + 3] = 255;

                    // Pixel (x0, y1)
                    let c01 = (y_plane[y1_offset + x0] as i32) - 16;
                    let y_term01 = 298 * c01;
                    let px01 = dst1_offset + x0 * 4;
                    dst_data[px01] = clamp_u8((y_term01 + r_chroma) >> 8);
                    dst_data[px01 + 1] = clamp_u8((y_term01 + g_chroma) >> 8);
                    dst_data[px01 + 2] = clamp_u8((y_term01 + b_chroma) >> 8);
                    dst_data[px01 + 3] = 255;

                    // Pixel (x1, y1)
                    let c11 = (y_plane[y1_offset + x1] as i32) - 16;
                    let y_term11 = 298 * c11;
                    let px11 = dst1_offset + x1 * 4;
                    dst_data[px11] = clamp_u8((y_term11 + r_chroma) >> 8);
                    dst_data[px11 + 1] = clamp_u8((y_term11 + g_chroma) >> 8);
                    dst_data[px11 + 2] = clamp_u8((y_term11 + b_chroma) >> 8);
                    dst_data[px11 + 3] = 255;
                }

                // Trailing column if width is odd
                if !width.is_multiple_of(2) {
                    let x = width - 1;
                    let bx = x / 2;
                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt601(u_val, v_val);

                    let c0 = (y_plane[y0_offset + x] as i32) - 16;
                    let y_term0 = 298 * c0;
                    let px0 = dst0_offset + x * 4;
                    dst_data[px0] = clamp_u8((y_term0 + r_chroma) >> 8);
                    dst_data[px0 + 1] = clamp_u8((y_term0 + g_chroma) >> 8);
                    dst_data[px0 + 2] = clamp_u8((y_term0 + b_chroma) >> 8);
                    dst_data[px0 + 3] = 255;

                    let c1 = (y_plane[y1_offset + x] as i32) - 16;
                    let y_term1 = 298 * c1;
                    let px1 = dst1_offset + x * 4;
                    dst_data[px1] = clamp_u8((y_term1 + r_chroma) >> 8);
                    dst_data[px1 + 1] = clamp_u8((y_term1 + g_chroma) >> 8);
                    dst_data[px1 + 2] = clamp_u8((y_term1 + b_chroma) >> 8);
                    dst_data[px1 + 3] = 255;
                }
            }

            // Trailing row if height is odd
            if !height.is_multiple_of(2) {
                let y = height - 1;
                let by = y / 2;
                let y_offset = y * stride_y;
                let uv_row_offset = by * stride_u;
                let v_row_offset = by * stride_v;
                let dst_row_offset = y * dst_stride;

                for bx in 0..half_w {
                    let x0 = bx * 2;
                    let x1 = x0 + 1;

                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt601(u_val, v_val);

                    let c0 = (y_plane[y_offset + x0] as i32) - 16;
                    let y_term0 = 298 * c0;
                    let px0 = dst_row_offset + x0 * 4;
                    dst_data[px0] = clamp_u8((y_term0 + r_chroma) >> 8);
                    dst_data[px0 + 1] = clamp_u8((y_term0 + g_chroma) >> 8);
                    dst_data[px0 + 2] = clamp_u8((y_term0 + b_chroma) >> 8);
                    dst_data[px0 + 3] = 255;

                    let c1 = (y_plane[y_offset + x1] as i32) - 16;
                    let y_term1 = 298 * c1;
                    let px1 = dst_row_offset + x1 * 4;
                    dst_data[px1] = clamp_u8((y_term1 + r_chroma) >> 8);
                    dst_data[px1 + 1] = clamp_u8((y_term1 + g_chroma) >> 8);
                    dst_data[px1 + 2] = clamp_u8((y_term1 + b_chroma) >> 8);
                    dst_data[px1 + 3] = 255;
                }

                if !width.is_multiple_of(2) {
                    let x = width - 1;
                    let bx = x / 2;
                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt601(u_val, v_val);

                    let c = (y_plane[y_offset + x] as i32) - 16;
                    let y_term = 298 * c;
                    let px = dst_row_offset + x * 4;
                    dst_data[px] = clamp_u8((y_term + r_chroma) >> 8);
                    dst_data[px + 1] = clamp_u8((y_term + g_chroma) >> 8);
                    dst_data[px + 2] = clamp_u8((y_term + b_chroma) >> 8);
                    dst_data[px + 3] = 255;
                }
            }
        }
        ColorStandard::Bt709 => {
            // Main unrolled 2x2 blocks with chroma reuse
            for by in 0..half_h {
                let y0 = by * 2;
                let y1 = y0 + 1;
                let y0_offset = y0 * stride_y;
                let y1_offset = y1 * stride_y;
                let uv_row_offset = by * stride_u;
                let v_row_offset = by * stride_v;
                let dst0_offset = y0 * dst_stride;
                let dst1_offset = y1 * dst_stride;

                for bx in 0..half_w {
                    let x0 = bx * 2;
                    let x1 = x0 + 1;

                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt709(u_val, v_val);

                    // Pixel (x0, y0)
                    let c00 = (y_plane[y0_offset + x0] as i32) - 16;
                    let y_term00 = 298 * c00;
                    let px00 = dst0_offset + x0 * 4;
                    dst_data[px00] = clamp_u8((y_term00 + r_chroma) >> 8);
                    dst_data[px00 + 1] = clamp_u8((y_term00 + g_chroma) >> 8);
                    dst_data[px00 + 2] = clamp_u8((y_term00 + b_chroma) >> 8);
                    dst_data[px00 + 3] = 255;

                    // Pixel (x1, y0)
                    let c10 = (y_plane[y0_offset + x1] as i32) - 16;
                    let y_term10 = 298 * c10;
                    let px10 = dst0_offset + x1 * 4;
                    dst_data[px10] = clamp_u8((y_term10 + r_chroma) >> 8);
                    dst_data[px10 + 1] = clamp_u8((y_term10 + g_chroma) >> 8);
                    dst_data[px10 + 2] = clamp_u8((y_term10 + b_chroma) >> 8);
                    dst_data[px10 + 3] = 255;

                    // Pixel (x0, y1)
                    let c01 = (y_plane[y1_offset + x0] as i32) - 16;
                    let y_term01 = 298 * c01;
                    let px01 = dst1_offset + x0 * 4;
                    dst_data[px01] = clamp_u8((y_term01 + r_chroma) >> 8);
                    dst_data[px01 + 1] = clamp_u8((y_term01 + g_chroma) >> 8);
                    dst_data[px01 + 2] = clamp_u8((y_term01 + b_chroma) >> 8);
                    dst_data[px01 + 3] = 255;

                    // Pixel (x1, y1)
                    let c11 = (y_plane[y1_offset + x1] as i32) - 16;
                    let y_term11 = 298 * c11;
                    let px11 = dst1_offset + x1 * 4;
                    dst_data[px11] = clamp_u8((y_term11 + r_chroma) >> 8);
                    dst_data[px11 + 1] = clamp_u8((y_term11 + g_chroma) >> 8);
                    dst_data[px11 + 2] = clamp_u8((y_term11 + b_chroma) >> 8);
                    dst_data[px11 + 3] = 255;
                }

                // Trailing column if width is odd
                if !width.is_multiple_of(2) {
                    let x = width - 1;
                    let bx = x / 2;
                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt709(u_val, v_val);

                    let c0 = (y_plane[y0_offset + x] as i32) - 16;
                    let y_term0 = 298 * c0;
                    let px0 = dst0_offset + x * 4;
                    dst_data[px0] = clamp_u8((y_term0 + r_chroma) >> 8);
                    dst_data[px0 + 1] = clamp_u8((y_term0 + g_chroma) >> 8);
                    dst_data[px0 + 2] = clamp_u8((y_term0 + b_chroma) >> 8);
                    dst_data[px0 + 3] = 255;

                    let c1 = (y_plane[y1_offset + x] as i32) - 16;
                    let y_term1 = 298 * c1;
                    let px1 = dst1_offset + x * 4;
                    dst_data[px1] = clamp_u8((y_term1 + r_chroma) >> 8);
                    dst_data[px1 + 1] = clamp_u8((y_term1 + g_chroma) >> 8);
                    dst_data[px1 + 2] = clamp_u8((y_term1 + b_chroma) >> 8);
                    dst_data[px1 + 3] = 255;
                }
            }

            // Trailing row if height is odd
            if !height.is_multiple_of(2) {
                let y = height - 1;
                let by = y / 2;
                let y_offset = y * stride_y;
                let uv_row_offset = by * stride_u;
                let v_row_offset = by * stride_v;
                let dst_row_offset = y * dst_stride;

                for bx in 0..half_w {
                    let x0 = bx * 2;
                    let x1 = x0 + 1;

                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt709(u_val, v_val);

                    let c0 = (y_plane[y_offset + x0] as i32) - 16;
                    let y_term0 = 298 * c0;
                    let px0 = dst_row_offset + x0 * 4;
                    dst_data[px0] = clamp_u8((y_term0 + r_chroma) >> 8);
                    dst_data[px0 + 1] = clamp_u8((y_term0 + g_chroma) >> 8);
                    dst_data[px0 + 2] = clamp_u8((y_term0 + b_chroma) >> 8);
                    dst_data[px0 + 3] = 255;

                    let c1 = (y_plane[y_offset + x1] as i32) - 16;
                    let y_term1 = 298 * c1;
                    let px1 = dst_row_offset + x1 * 4;
                    dst_data[px1] = clamp_u8((y_term1 + r_chroma) >> 8);
                    dst_data[px1 + 1] = clamp_u8((y_term1 + g_chroma) >> 8);
                    dst_data[px1 + 2] = clamp_u8((y_term1 + b_chroma) >> 8);
                    dst_data[px1 + 3] = 255;
                }

                if !width.is_multiple_of(2) {
                    let x = width - 1;
                    let bx = x / 2;
                    let u_val = u_plane[uv_row_offset + bx];
                    let v_val = v_plane[v_row_offset + bx];
                    let (r_chroma, g_chroma, b_chroma) = chroma_terms_bt709(u_val, v_val);

                    let c = (y_plane[y_offset + x] as i32) - 16;
                    let y_term = 298 * c;
                    let px = dst_row_offset + x * 4;
                    dst_data[px] = clamp_u8((y_term + r_chroma) >> 8);
                    dst_data[px + 1] = clamp_u8((y_term + g_chroma) >> 8);
                    dst_data[px + 2] = clamp_u8((y_term + b_chroma) >> 8);
                    dst_data[px + 3] = 255;
                }
            }
        }
    }

    dst.pts_ms = src.pts_ms;
    Ok(())
}

/// Single character cell representing two vertically stacked sub-pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalColorCell {
    /// Red component of upper sub-pixel (foreground).
    pub top_r: u8,
    /// Green component of upper sub-pixel (foreground).
    pub top_g: u8,
    /// Blue component of upper sub-pixel (foreground).
    pub top_b: u8,
    /// Red component of lower sub-pixel (background).
    pub bot_r: u8,
    /// Green component of lower sub-pixel (background).
    pub bot_g: u8,
    /// Blue component of lower sub-pixel (background).
    pub bot_b: u8,
}

/// Presentation buffer for rendering frames to ANSI 24-bit TrueColor terminal displays
/// using Unicode half-block characters (`▀`, `U+2580`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTrueColorBuffer {
    /// Number of terminal columns (width in characters).
    pub cols: usize,
    /// Number of terminal rows (height in characters; displays 2 * rows vertical pixels).
    pub rows: usize,
    /// Flat row-major vector of terminal cells (`cols * rows`).
    pub cells: Vec<TerminalColorCell>,
}

impl TerminalTrueColorBuffer {
    /// Constructs a new `TerminalTrueColorBuffer` of specified character dimensions.
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            cells: vec![TerminalColorCell::default(); cols * rows],
        }
    }

    /// Resizes the buffer to match updated terminal dimensions.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if self.cols != cols || self.rows != rows {
            self.cols = cols;
            self.rows = rows;
            self.cells = vec![TerminalColorCell::default(); cols * rows];
        }
    }

    /// Renders the buffer contents to an ANSI 24-bit TrueColor string with
    /// run-length color deduplication.
    ///
    /// Deduplication minimizes terminal bandwidth and avoids screen flickering by
    /// suppressing redundant escape sequences when consecutive sub-pixels share the same color.
    pub fn render_ansi(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();

        let mut current_fg: Option<(u8, u8, u8)> = None;
        let mut current_bg: Option<(u8, u8, u8)> = None;

        for r in 0..self.rows {
            let row_offset = r * self.cols;
            for c in 0..self.cols {
                let cell = self.cells[row_offset + c];
                let fg = (cell.top_r, cell.top_g, cell.top_b);
                let bg = (cell.bot_r, cell.bot_g, cell.bot_b);

                if current_fg != Some(fg) {
                    let _ = write!(out, "\x1b[38;2;{};{};{}m", fg.0, fg.1, fg.2);
                    current_fg = Some(fg);
                }
                if current_bg != Some(bg) {
                    let _ = write!(out, "\x1b[48;2;{};{};{}m", bg.0, bg.1, bg.2);
                    current_bg = Some(bg);
                }
                out.push('▀');
            }
            out.push_str("\x1b[0m\n");
            current_fg = None;
            current_bg = None;
        }
    }
}

/// Scales and converts a planar `VideoFrameYUV420p` into a `TerminalTrueColorBuffer`
/// using pure-Rust Q8 fixed-point arithmetic.
pub fn convert_yuv420p_to_terminal(
    src: &VideoFrameYUV420p,
    dst: &mut TerminalTrueColorBuffer,
    standard: ColorStandard,
) -> Result<(), CodecError> {
    if src.width == 0 || src.height == 0 || dst.cols == 0 || dst.rows == 0 {
        return Ok(());
    }

    let src_w = src.width as usize;
    let src_h = src.height as usize;
    let target_h = dst.rows * 2;

    let y_plane = src.y_plane.as_slice();
    let u_plane = src.u_plane.as_slice();
    let v_plane = src.v_plane.as_slice();

    let stride_y = src.stride_y;
    let stride_u = src.stride_u;
    let stride_v = src.stride_v;

    let yuv_to_rgb = match standard {
        ColorStandard::Bt601 => yuv_to_rgb_bt601,
        ColorStandard::Bt709 => yuv_to_rgb_bt709,
    };

    for r in 0..dst.rows {
        let y_top = ((r * 2 * src_h) / target_h).min(src_h - 1);
        let y_bot = (((r * 2 + 1) * src_h) / target_h).min(src_h - 1);

        let y_top_row = y_top * stride_y;
        let y_bot_row = y_bot * stride_y;

        let uv_top_row = (y_top / 2) * stride_u;
        let uv_bot_row = (y_bot / 2) * stride_u;
        let v_top_row = (y_top / 2) * stride_v;
        let v_bot_row = (y_bot / 2) * stride_v;

        let dst_row = r * dst.cols;

        for c in 0..dst.cols {
            let x = ((c * src_w) / dst.cols).min(src_w - 1);
            let cx = x / 2;

            // Top pixel (foreground)
            let yt = y_plane[y_top_row + x];
            let ut = u_plane[uv_top_row + cx];
            let vt = v_plane[v_top_row + cx];
            let (top_r, top_g, top_b) = yuv_to_rgb(yt, ut, vt);

            // Bottom pixel (background)
            let yb = y_plane[y_bot_row + x];
            let ub = u_plane[uv_bot_row + cx];
            let vb = v_plane[v_bot_row + cx];
            let (bot_r, bot_g, bot_b) = yuv_to_rgb(yb, ub, vb);

            dst.cells[dst_row + c] = TerminalColorCell {
                top_r,
                top_g,
                top_b,
                bot_r,
                bot_g,
                bot_b,
            };
        }
    }

    Ok(())
}

/// Scales and converts a packed `VideoFrameRGBA` into a `TerminalTrueColorBuffer`.
pub fn convert_rgba_to_terminal(
    src: &VideoFrameRGBA,
    dst: &mut TerminalTrueColorBuffer,
) -> Result<(), CodecError> {
    if src.width == 0 || src.height == 0 || dst.cols == 0 || dst.rows == 0 {
        return Ok(());
    }

    let src_w = src.width as usize;
    let src_h = src.height as usize;
    let target_h = dst.rows * 2;
    let data = src.data.as_slice();
    let stride = src.stride;

    for r in 0..dst.rows {
        let y_top = ((r * 2 * src_h) / target_h).min(src_h - 1);
        let y_bot = (((r * 2 + 1) * src_h) / target_h).min(src_h - 1);

        let row_top = y_top * stride;
        let row_bot = y_bot * stride;
        let dst_row = r * dst.cols;

        for c in 0..dst.cols {
            let x = ((c * src_w) / dst.cols).min(src_w - 1);
            let idx_top = row_top + x * 4;
            let idx_bot = row_bot + x * 4;

            dst.cells[dst_row + c] = TerminalColorCell {
                top_r: data[idx_top],
                top_g: data[idx_top + 1],
                top_b: data[idx_top + 2],
                bot_r: data[idx_bot],
                bot_g: data[idx_bot + 1],
                bot_b: data[idx_bot + 2],
            };
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bt601_reference_primaries() {
        // Reference Black: (16, 128, 128) -> (0, 0, 0)
        let (r, g, b) = yuv_to_rgb_bt601(16, 128, 128);
        assert_eq!((r, g, b), (0, 0, 0));

        // Reference White: (235, 128, 128) -> (255, 255, 255)
        let (r, g, b) = yuv_to_rgb_bt601(235, 128, 128);
        assert_eq!((r, g, b), (255, 255, 255));

        // Reference Saturated Red: (81, 90, 240) -> (R >= 250, G <= 2, B <= 2)
        let (r, g, b) = yuv_to_rgb_bt601(81, 90, 240);
        assert!(r >= 250, "Red must be >= 250, got {}", r);
        assert!(g <= 2, "Green must be <= 2, got {}", g);
        assert!(b <= 2, "Blue must be <= 2, got {}", b);
    }

    #[test]
    fn test_bt709_reference_primaries() {
        // Reference White: (235, 128, 128) -> (255, 255, 255)
        let (r, g, b) = yuv_to_rgb_bt709(235, 128, 128);
        assert_eq!((r, g, b), (255, 255, 255));

        // Pure Blue: (41, 240, 110) -> (B >= 250, R <= 2)
        let (r, _, b) = yuv_to_rgb_bt709(41, 240, 110);
        assert!(b >= 250, "Blue must be >= 250, got {}", b);
        assert!(r <= 2, "Red must be <= 2, got {}", r);
    }

    #[test]
    fn test_full_frame_conversion_zero_allocation() {
        let width = 64;
        let height = 64;
        let mut yuv = VideoFrameYUV420p::new(width, height, 100);
        // Fill with black
        yuv.y_plane.as_mut_slice().fill(16);
        yuv.u_plane.as_mut_slice().fill(128);
        yuv.v_plane.as_mut_slice().fill(128);

        let mut rgba = VideoFrameRGBA::new(width, height, 0);
        convert_yuv420p_to_rgba(&yuv, &mut rgba, ColorStandard::Bt601).unwrap();

        assert_eq!(rgba.pts_ms, 100);
        // Check pixel (0,0) is black with alpha=255
        assert_eq!(rgba.data[0], 0);
        assert_eq!(rgba.data[1], 0);
        assert_eq!(rgba.data[2], 0);
        assert_eq!(rgba.data[3], 255);
    }

    #[test]
    fn test_converter_traits() {
        let width = 32;
        let height = 32;
        let mut yuv = VideoFrameYUV420p::new(width, height, 42);
        yuv.y_plane.as_mut_slice().fill(235);
        yuv.u_plane.as_mut_slice().fill(128);
        yuv.v_plane.as_mut_slice().fill(128);

        let mut rgba_601 = VideoFrameRGBA::new(width, height, 0);
        let conv_601 = Bt601Converter::new();
        assert_eq!(conv_601.standard(), ColorStandard::Bt601);
        conv_601.convert_frame(&yuv, &mut rgba_601).unwrap();
        assert_eq!(rgba_601.data[0], 255);
        assert_eq!(rgba_601.data[1], 255);
        assert_eq!(rgba_601.data[2], 255);
        assert_eq!(rgba_601.data[3], 255);

        let mut rgba_709 = VideoFrameRGBA::new(width, height, 0);
        let conv_709 = Bt709Converter::new();
        assert_eq!(conv_709.standard(), ColorStandard::Bt709);
        conv_709.convert_frame(&yuv, &mut rgba_709).unwrap();
        assert_eq!(rgba_709.data[0], 255);
        assert_eq!(rgba_709.data[1], 255);
        assert_eq!(rgba_709.data[2], 255);
        assert_eq!(rgba_709.data[3], 255);

        // Trait object polymorphism
        let converters: Vec<Box<dyn ColorSpaceConverter>> = vec![
            Box::new(Bt601Converter::new()),
            Box::new(Bt709Converter::new()),
        ];
        assert_eq!(converters[0].standard(), ColorStandard::Bt601);
        assert_eq!(converters[1].standard(), ColorStandard::Bt709);
    }

    #[test]
    fn test_mathematical_equivalence_unrolled_vs_scalar() {
        // Test multiple dimensions including odd dimensions
        let test_sizes = [(16, 16), (32, 24), (17, 19), (65, 33), (80, 45)];

        for (width, height) in test_sizes {
            let mut yuv = VideoFrameYUV420p::new(width, height, 1234);

            // Populate planes with pseudorandom gradient values
            for y in 0..(height as usize) {
                for x in 0..(width as usize) {
                    yuv.y_plane.as_mut_slice()[y * yuv.stride_y + x] =
                        ((x * 7 + y * 13 + 16) % 220 + 16) as u8;
                }
            }
            let chroma_h = (height as usize).div_ceil(2);
            let chroma_w = (width as usize).div_ceil(2);
            for y in 0..chroma_h {
                for x in 0..chroma_w {
                    yuv.u_plane.as_mut_slice()[y * yuv.stride_u + x] =
                        ((x * 11 + y * 17 + 64) % 200 + 16) as u8;
                    yuv.v_plane.as_mut_slice()[y * yuv.stride_v + x] =
                        ((x * 19 + y * 23 + 32) % 200 + 16) as u8;
                }
            }

            for standard in [ColorStandard::Bt601, ColorStandard::Bt709] {
                let mut rgba_unrolled = VideoFrameRGBA::new(width, height, 0);
                convert_yuv420p_to_rgba(&yuv, &mut rgba_unrolled, standard).unwrap();

                // Compute exact scalar per-pixel reference
                let y_slice = yuv.y_plane.as_slice();
                let u_slice = yuv.u_plane.as_slice();
                let v_slice = yuv.v_plane.as_slice();
                let rgba_data = rgba_unrolled.data.as_slice();

                for y in 0..(height as usize) {
                    for x in 0..(width as usize) {
                        let y_val = y_slice[y * yuv.stride_y + x];
                        let u_val = u_slice[(y >> 1) * yuv.stride_u + (x >> 1)];
                        let v_val = v_slice[(y >> 1) * yuv.stride_v + (x >> 1)];

                        let (exp_r, exp_g, exp_b) = match standard {
                            ColorStandard::Bt601 => yuv_to_rgb_bt601(y_val, u_val, v_val),
                            ColorStandard::Bt709 => yuv_to_rgb_bt709(y_val, u_val, v_val),
                        };

                        let px_offset = y * rgba_unrolled.stride + x * 4;
                        let act_r = rgba_data[px_offset];
                        let act_g = rgba_data[px_offset + 1];
                        let act_b = rgba_data[px_offset + 2];
                        let act_a = rgba_data[px_offset + 3];

                        assert_eq!(
                            (act_r, act_g, act_b, act_a),
                            (exp_r, exp_g, exp_b, 255),
                            "Mismatch at pixel ({}, {}) for {:?} in {}x{} frame",
                            x,
                            y,
                            standard,
                            width,
                            height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_terminal_buffer_rendering_and_deduplication() {
        let mut buf = TerminalTrueColorBuffer::new(2, 1);
        // Cell 0: Red top (255,0,0), Blue bottom (0,0,255)
        buf.cells[0] = TerminalColorCell {
            top_r: 255,
            top_g: 0,
            top_b: 0,
            bot_r: 0,
            bot_g: 0,
            bot_b: 255,
        };
        // Cell 1: Same colors (should deduplicate escape codes)
        buf.cells[1] = TerminalColorCell {
            top_r: 255,
            top_g: 0,
            top_b: 0,
            bot_r: 0,
            bot_g: 0,
            bot_b: 255,
        };

        let mut ansi = String::new();
        buf.render_ansi(&mut ansi);

        // Expected: single fg code, single bg code, two '▀', then reset and newline
        let expected = "\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀▀\x1b[0m\n";
        assert_eq!(ansi, expected);
    }

    #[test]
    fn test_terminal_buffer_frame_size_bounding() {
        // Standard 80x24 terminal: 1920 cells
        let buf = TerminalTrueColorBuffer::new(80, 24);
        let mut ansi = String::new();
        buf.render_ansi(&mut ansi);

        // For uniform black frame with deduplication:
        // ~1 escape sequence per row + 80 half-blocks + reset = very small
        assert!(
            ansi.len() < 18 * 1024,
            "Rendered frame must be < 18KB, was {} bytes",
            ansi.len()
        );
    }

    #[test]
    fn test_yuv_and_rgba_to_terminal_conversion() {
        let mut yuv = VideoFrameYUV420p::new(16, 16, 0);
        yuv.y_plane.as_mut_slice().fill(16); // Black
        yuv.u_plane.as_mut_slice().fill(128);
        yuv.v_plane.as_mut_slice().fill(128);

        let mut term_buf = TerminalTrueColorBuffer::new(8, 4);
        convert_yuv420p_to_terminal(&yuv, &mut term_buf, ColorStandard::Bt601).unwrap();

        assert_eq!(term_buf.cells.len(), 32);
        assert_eq!(term_buf.cells[0].top_r, 0);
        assert_eq!(term_buf.cells[0].bot_r, 0);

        let rgba = VideoFrameRGBA::new(16, 16, 0);
        let mut term_buf2 = TerminalTrueColorBuffer::new(8, 4);
        convert_rgba_to_terminal(&rgba, &mut term_buf2).unwrap();
        assert_eq!(term_buf2.cells.len(), 32);
    }
}
