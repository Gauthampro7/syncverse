//! Aspect ratio override configurations for video viewport presentation.

/// Helper trait allowing both float and unsigned/signed integer types as viewport dimensions.
pub trait IntoDimension: Copy {
    /// Converts this dimension into `f32`.
    fn into_dimension(self) -> f32;
}

impl IntoDimension for f32 {
    #[inline]
    fn into_dimension(self) -> f32 {
        self
    }
}

impl IntoDimension for f64 {
    #[inline]
    fn into_dimension(self) -> f32 {
        self as f32
    }
}

impl IntoDimension for u32 {
    #[inline]
    fn into_dimension(self) -> f32 {
        self as f32
    }
}

impl IntoDimension for i32 {
    #[inline]
    fn into_dimension(self) -> f32 {
        self as f32
    }
}

impl IntoDimension for usize {
    #[inline]
    fn into_dimension(self) -> f32 {
        self as f32
    }
}

/// User-selectable aspect ratio override mode for video playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AspectRatioOverride {
    /// Use natural video stream aspect ratio (default).
    #[default]
    Original,
    /// Force 16:9 widescreen aspect ratio.
    Ratio16_9,
    /// Force 4:3 classic television aspect ratio.
    Ratio4_3,
    /// Force 21:9 cinema ultrawide aspect ratio.
    Ratio21_9,
    /// Force 1:1 square aspect ratio.
    Ratio1_1,
    /// Fill the entire available viewport without preserving aspect ratio (stretch).
    FillStretch,
}

impl AspectRatioOverride {
    /// All available presets in cyclic order.
    pub const ALL: [AspectRatioOverride; 6] = [
        Self::Original,
        Self::Ratio16_9,
        Self::Ratio4_3,
        Self::Ratio21_9,
        Self::Ratio1_1,
        Self::FillStretch,
    ];

    /// Human-readable display label for UI menus and HUD overlays.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Ratio16_9 => "16:9",
            Self::Ratio4_3 => "4:3",
            Self::Ratio21_9 => "21:9",
            Self::Ratio1_1 => "1:1",
            Self::FillStretch => "Fill / Stretch",
        }
    }

    /// Cycles sequentially to the next aspect ratio preset.
    pub fn cycle_next(&self) -> Self {
        match self {
            Self::Original => Self::Ratio16_9,
            Self::Ratio16_9 => Self::Ratio4_3,
            Self::Ratio4_3 => Self::Ratio21_9,
            Self::Ratio21_9 => Self::Ratio1_1,
            Self::Ratio1_1 => Self::FillStretch,
            Self::FillStretch => Self::Original,
        }
    }

    /// Computes the target aspect ratio width-to-height ($W / H$) for this mode.
    ///
    /// Returns `None` if:
    /// - `FillStretch` mode is active (video fills available viewport).
    /// - `Original` mode is active but natural dimensions are non-positive (degenerate).
    pub fn target_ratio(&self, natural_width: u32, natural_height: u32) -> Option<f32> {
        match self {
            Self::Original => {
                if natural_width > 0 && natural_height > 0 {
                    Some(natural_width as f32 / natural_height as f32)
                } else {
                    None
                }
            }
            Self::Ratio16_9 => Some(16.0 / 9.0),
            Self::Ratio4_3 => Some(4.0 / 3.0),
            Self::Ratio21_9 => Some(21.0 / 9.0),
            Self::Ratio1_1 => Some(1.0),
            Self::FillStretch => None,
        }
    }

    /// Computes the target aspect ratio width-to-height ($W / H$) from `f32` inputs.
    pub fn target_ratio_f32(&self, natural_width: f32, natural_height: f32) -> Option<f32> {
        match self {
            Self::Original => {
                if natural_width > 0.0 && natural_height > 0.0 {
                    Some(natural_width / natural_height)
                } else {
                    None
                }
            }
            Self::Ratio16_9 => Some(16.0 / 9.0),
            Self::Ratio4_3 => Some(4.0 / 3.0),
            Self::Ratio21_9 => Some(21.0 / 9.0),
            Self::Ratio1_1 => Some(1.0),
            Self::FillStretch => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aspect_ratio_defaults_and_display_names() {
        assert_eq!(AspectRatioOverride::default(), AspectRatioOverride::Original);
        assert_eq!(AspectRatioOverride::Original.display_name(), "Original");
        assert_eq!(AspectRatioOverride::Ratio16_9.display_name(), "16:9");
        assert_eq!(AspectRatioOverride::Ratio4_3.display_name(), "4:3");
        assert_eq!(AspectRatioOverride::Ratio21_9.display_name(), "21:9");
        assert_eq!(AspectRatioOverride::Ratio1_1.display_name(), "1:1");
        assert_eq!(AspectRatioOverride::FillStretch.display_name(), "Fill / Stretch");
    }

    #[test]
    fn test_aspect_ratio_cycle_next() {
        let mut ar = AspectRatioOverride::Original;
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::Ratio16_9);
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::Ratio4_3);
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::Ratio21_9);
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::Ratio1_1);
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::FillStretch);
        ar = ar.cycle_next();
        assert_eq!(ar, AspectRatioOverride::Original);
    }

    #[test]
    fn test_aspect_ratio_target_ratio_values() {
        // Original with valid dimensions
        assert_eq!(
            AspectRatioOverride::Original.target_ratio(1920, 1080),
            Some(1920.0 / 1080.0)
        );
        // Original with degenerate dimensions
        assert_eq!(AspectRatioOverride::Original.target_ratio(0, 1080), None);
        assert_eq!(AspectRatioOverride::Original.target_ratio(1920, 0), None);

        // Fixed presets
        assert_eq!(
            AspectRatioOverride::Ratio16_9.target_ratio(640, 480),
            Some(16.0 / 9.0)
        );
        assert_eq!(
            AspectRatioOverride::Ratio4_3.target_ratio(1920, 1080),
            Some(4.0 / 3.0)
        );
        assert_eq!(
            AspectRatioOverride::Ratio21_9.target_ratio(1920, 1080),
            Some(21.0 / 9.0)
        );
        assert_eq!(
            AspectRatioOverride::Ratio1_1.target_ratio(1920, 1080),
            Some(1.0)
        );

        // FillStretch returns None
        assert_eq!(
            AspectRatioOverride::FillStretch.target_ratio(1920, 1080),
            None
        );
    }

    #[test]
    fn test_into_dimension_conversions() {
        assert_eq!(1920u32.into_dimension(), 1920.0f32);
        assert_eq!(1080i32.into_dimension(), 1080.0f32);
        assert_eq!(1920.0f32.into_dimension(), 1920.0f32);
        assert_eq!(1080.0f64.into_dimension(), 1080.0f32);
        assert_eq!(100usize.into_dimension(), 100.0f32);
    }
}
