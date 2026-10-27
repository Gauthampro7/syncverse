//! Media and wall-clock timestamp primitives with nanosecond precision and rational conversions.

use std::fmt;
use std::ops::{Add, Sub};
use std::time::Instant;

/// Media presentation timestamp (PTS) with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MediaTime(u64);

impl MediaTime {
    /// Sentinel value representing unanchored or invalid media presentation time.
    pub const UNANCHORED: Self = Self(u64::MAX);

    /// Creates a `MediaTime` from milliseconds.
    #[inline]
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms.saturating_mul(1_000_000))
    }

    /// Creates a `MediaTime` from nanoseconds.
    #[inline]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Creates a `MediaTime` from a rational fraction of seconds: `num / den`.
    pub fn from_rational(num: u64, den: u64) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let nanos = (num as u128)
            .saturating_mul(1_000_000_000)
            .checked_div(den as u128)?;
        if nanos > u64::MAX as u128 {
            Some(Self(u64::MAX))
        } else {
            Some(Self(nanos as u64))
        }
    }

    /// Returns presentation time in whole milliseconds.
    #[inline]
    pub const fn as_millis(&self) -> u64 {
        self.0 / 1_000_000
    }

    /// Returns presentation time in total nanoseconds.
    #[inline]
    pub const fn as_nanos(&self) -> u64 {
        self.0
    }

    /// Returns presentation time in fractional seconds (`f64`).
    #[inline]
    pub fn as_secs_f64(&self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }

    /// Formats the presentation timestamp as `HH:MM:SS.mmm`.
    pub fn to_timecode_string(&self) -> String {
        if *self == Self::UNANCHORED {
            return "--:--:--.---".to_string();
        }
        let total_ms = self.as_millis();
        let ms = total_ms % 1000;
        let total_secs = total_ms / 1000;
        let secs = total_secs % 60;
        let total_mins = total_secs / 60;
        let mins = total_mins % 60;
        let hours = total_mins / 60;
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, ms)
    }
}

impl fmt::Display for MediaTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_timecode_string())
    }
}

impl Add for MediaTime {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sub for MediaTime {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

/// Wall-clock timestamp backed by both physical UTC epoch milliseconds and monotonic hardware `Instant`.
#[derive(Debug, Clone, Copy)]
pub struct WallTime {
    /// UTC epoch timestamp in milliseconds.
    pub unix_ms: u64,
    /// Monotonic hardware instant.
    pub instant: Instant,
}

impl WallTime {
    /// Captures the current wall-clock instant.
    pub fn now() -> Self {
        let unix_ms = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_millis() as u64,
            Err(_) => 0,
        };
        Self {
            unix_ms,
            instant: Instant::now(),
        }
    }

    /// Creates a `WallTime` from UTC epoch milliseconds using the current monotonic instant.
    pub fn from_unix_millis(unix_ms: u64) -> Self {
        Self {
            unix_ms,
            instant: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_time_conversions() {
        let mt = MediaTime::from_millis(1250);
        assert_eq!(mt.as_millis(), 1250);
        assert_eq!(mt.as_nanos(), 1_250_000_000);
        assert_eq!(mt.to_timecode_string(), "00:00:01.250");

        let mt_rat = MediaTime::from_rational(24, 1).unwrap();
        assert_eq!(mt_rat.as_millis(), 24000);

        let hours = MediaTime::from_millis(3600 * 1000 + 120 * 1000 + 500);
        assert_eq!(hours.to_timecode_string(), "01:02:00.500");
    }
}
