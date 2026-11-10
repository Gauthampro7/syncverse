//! Real-time video picture adjustments (brightness, contrast, saturation, gamma) with LUT acceleration.

/// Real-time video picture color adjustments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoColorAdjustments {
    /// Whether color adjustment post-processing is enabled.
    pub enabled: bool,
    /// Brightness offset in range [-1.0, 1.0], default 0.0.
    pub brightness: f32,
    /// Contrast multiplier in range [0.0, 2.0], default 1.0.
    pub contrast: f32,
    /// Saturation multiplier in range [0.0, 3.0], default 1.0.
    pub saturation: f32,
    /// Gamma correction power in range [0.2, 2.5], default 1.0.
    pub gamma: f32,
}

impl Default for VideoColorAdjustments {
    fn default() -> Self {
        Self {
            enabled: false,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
        }
    }
}

impl VideoColorAdjustments {
    /// Constructs default (identity) video adjustments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets all adjustment sliders to their neutral identity values.
    pub fn reset(&mut self) {
        self.brightness = 0.0;
        self.contrast = 1.0;
        self.saturation = 1.0;
        self.gamma = 1.0;
    }

    /// Returns `true` if the configuration results in a bit-exact no-op identity transformation.
    pub fn is_identity(&self) -> bool {
        if !self.enabled {
            return true;
        }
        self.brightness.abs() < 0.005
            && (self.contrast - 1.0).abs() < 0.005
            && (self.saturation - 1.0).abs() < 0.005
            && (self.gamma - 1.0).abs() < 0.005
    }

    /// Computes a 256-entry lookup table for the non-linear brightness, contrast, and gamma stage.
    pub fn compute_lut(&self) -> [u8; 256] {
        let mut lut = [0u8; 256];
        let contrast = self.contrast.clamp(0.0, 2.0);
        let brightness = self.brightness.clamp(-1.0, 1.0);
        let gamma = self.gamma.clamp(0.2, 2.5);
        let inv_gamma = if (gamma - 1.0).abs() > 0.005 {
            1.0 / gamma
        } else {
            1.0
        };

        for (i, slot) in lut.iter_mut().enumerate() {
            let mut v = (i as f32) / 255.0;
            // Contrast centered around 0.5 + brightness shift
            v = (v - 0.5) * contrast + 0.5 + brightness;
            let clamped = v.clamp(0.0, 1.0);
            let final_val = if inv_gamma != 1.0 && clamped > 0.0 {
                clamped.powf(inv_gamma)
            } else {
                clamped
            };
            *slot = (final_val * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
        lut
    }

    /// Applies brightness, contrast, gamma, and saturation adjustments from `src` to `dst`.
    ///
    /// If `stride == width * 4`, reads contiguously; otherwise unpacks row padding.
    pub fn apply_to_rgba(
        &self,
        src: &[u8],
        dst: &mut Vec<u8>,
        width: usize,
        height: usize,
        stride: usize,
    ) {
        let total_bytes = width * height * 4;
        dst.clear();
        dst.reserve(total_bytes);

        if self.is_identity() {
            if stride == width * 4 {
                dst.extend_from_slice(&src[..total_bytes]);
            } else {
                for y in 0..height {
                    let start = y * stride;
                    let end = start + width * 4;
                    dst.extend_from_slice(&src[start..end]);
                }
            }
            return;
        }

        let lut = self.compute_lut();
        let sat = self.saturation.clamp(0.0, 3.0);
        let has_saturation = (sat - 1.0).abs() >= 0.01;
        let sat_q8 = (sat * 256.0 + 0.5) as i32;

        unsafe {
            dst.set_len(total_bytes);
        }
        let out = dst.as_mut_slice();

        for y in 0..height {
            let in_row = &src[y * stride..y * stride + width * 4];
            let out_row = &mut out[y * width * 4..(y + 1) * width * 4];

            for x in 0..width {
                let px_in = &in_row[x * 4..x * 4 + 4];
                let px_out = &mut out_row[x * 4..x * 4 + 4];

                let mut r = lut[px_in[0] as usize] as i32;
                let mut g = lut[px_in[1] as usize] as i32;
                let mut b = lut[px_in[2] as usize] as i32;
                let a = px_in[3];

                if has_saturation {
                    // Q8 standard Rec.601/709 luma approximation: Y = (77*R + 150*G + 29*B) >> 8
                    let y_luma = (77 * r + 150 * g + 29 * b) >> 8;
                    r = y_luma + (((r - y_luma) * sat_q8) >> 8);
                    g = y_luma + (((g - y_luma) * sat_q8) >> 8);
                    b = y_luma + (((b - y_luma) * sat_q8) >> 8);
                    r = r.clamp(0, 255);
                    g = g.clamp(0, 255);
                    b = b.clamp(0, 255);
                }

                px_out[0] = r as u8;
                px_out[1] = g as u8;
                px_out[2] = b as u8;
                px_out[3] = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_adjustments_default_and_identity() {
        let adj = VideoColorAdjustments::default();
        assert!(!adj.enabled);
        assert!(adj.is_identity());

        let mut adj_enabled = VideoColorAdjustments {
            enabled: true,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
        };
        assert!(adj_enabled.is_identity());

        adj_enabled.brightness = 0.1;
        assert!(!adj_enabled.is_identity());
        adj_enabled.reset();
        assert!(adj_enabled.is_identity());
    }

    #[test]
    fn test_lut_contrast_and_brightness() {
        let adj = VideoColorAdjustments {
            enabled: true,
            brightness: 0.1,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
        };
        let lut = adj.compute_lut();
        // Midpoint 128 should be brighter
        assert!(lut[128] > 128);

        let adj_dark = VideoColorAdjustments {
            enabled: true,
            brightness: -0.1,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
        };
        let lut_dark = adj_dark.compute_lut();
        assert!(lut_dark[128] < 128);
    }

    #[test]
    fn test_apply_saturation_grayscale() {
        let adj_grayscale = VideoColorAdjustments {
            enabled: true,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 0.0,
            gamma: 1.0,
        };
        let src = vec![255, 0, 0, 255]; // Pure red
        let mut dst = Vec::new();
        adj_grayscale.apply_to_rgba(&src, &mut dst, 1, 1, 4);

        assert_eq!(dst.len(), 4);
        // In grayscale (saturation = 0), R, G, and B should be identical to luma (77 for pure red)
        assert_eq!(dst[0], dst[1]);
        assert_eq!(dst[1], dst[2]);
        assert_eq!(dst[3], 255);
    }
}
