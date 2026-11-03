//! Media and wall-clock timestamp primitives with nanosecond precision and rational conversions.

use std::fmt;
use std::ops::{Add, Sub};
use std::str::FromStr;
use std::time::Instant;

use crate::error::CommonError;

/// Media presentation timestamp (PTS) with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MediaTime(u64);

impl MediaTime {
    /// Sentinel value representing unanchored or invalid media presentation time.
    pub const UNANCHORED: Self = Self(u64::MAX);

    /// Zero presentation timestamp (0 nanoseconds).
    pub const ZERO: Self = Self(0);

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

    /// Formats the presentation timestamp as standard `HH:MM:SS` (without fractional seconds).
    pub fn to_hms(&self) -> String {
        if *self == Self::UNANCHORED {
            return "--:--:--".to_string();
        }
        let total_secs = self.as_millis() / 1000;
        let secs = total_secs % 60;
        let mins = (total_secs / 60) % 60;
        let hours = total_secs / 3600;
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    }

    /// Formats the duration into a human-friendly string (e.g. `"1h 23m 45s"`, `"14m 20s"`, or `"5s"`).
    pub fn to_friendly_duration(&self) -> String {
        if *self == Self::UNANCHORED {
            return "unknown".to_string();
        }
        let total_secs = self.as_millis() / 1000;
        let secs = total_secs % 60;
        let mins = (total_secs / 60) % 60;
        let hours = total_secs / 3600;

        if hours > 0 {
            format!("{}h {}m {}s", hours, mins, secs)
        } else if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// Computes the percentage `[0.0, 100.0]` of this timestamp relative to `total`.
    ///
    /// Returns `0.0` if `total` is zero, unanchored, or if `self` is unanchored.
    pub fn percent_of(&self, total: MediaTime) -> f64 {
        if *self == Self::UNANCHORED || total == Self::UNANCHORED || total.as_nanos() == 0 {
            return 0.0;
        }
        let ratio = (self.as_nanos() as f64) / (total.as_nanos() as f64);
        (ratio * 100.0).clamp(0.0, 100.0)
    }

    /// Computes a `MediaTime` from a percentage `[0.0, 100.0]` of `total`.
    ///
    /// Clamps the input percentage to `[0.0, 100.0]` and guards against negative or non-finite inputs.
    pub fn from_percent(total: MediaTime, percent: f64) -> MediaTime {
        if total == Self::UNANCHORED || total.as_nanos() == 0 || !percent.is_finite() || percent <= 0.0 {
            return MediaTime::ZERO;
        }
        let clamped = percent.clamp(0.0, 100.0);
        let nanos = ((total.as_nanos() as f64) * (clamped / 100.0)).round() as u64;
        MediaTime::from_nanos(nanos)
    }

    /// Parses a timecode string ("HH:MM:SS", "HH:MM:SS.mmm", "MM:SS", "MM:SS.mmm", or "SS.mmm") into `MediaTime`.
    ///
    /// Also recognizes the unanchored sentinel timecode `"--:--:--.---"`.
    pub fn from_timecode_str(s: &str) -> Result<Self, CommonError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(CommonError::InvalidData("empty timecode string".to_string()));
        }
        if trimmed == "--:--:--.---" {
            return Ok(Self::UNANCHORED);
        }

        if trimmed.contains(':') {
            let parts: Vec<&str> = trimmed.split(':').collect();
            match parts.len() {
                2 => {
                    // MM:SS or MM:SS.mmm
                    let mins: u64 = parts[0].parse().map_err(|_| {
                        CommonError::InvalidData(format!("invalid minutes in timecode: {}", s))
                    })?;
                    let sec_nanos = parse_seconds_nanos(parts[1], s)?;
                    if sec_nanos >= 60_000_000_000 {
                        return Err(CommonError::InvalidData(format!(
                            "seconds component must be less than 60 in timecode: {}",
                            s
                        )));
                    }
                    let total_nanos = mins
                        .checked_mul(60 * 1_000_000_000)
                        .and_then(|n| n.checked_add(sec_nanos))
                        .ok_or_else(|| {
                            CommonError::InvalidData(format!("timecode overflow: {}", s))
                        })?;
                    Ok(Self(total_nanos))
                }
                3 => {
                    // HH:MM:SS or HH:MM:SS.mmm
                    let hours: u64 = parts[0].parse().map_err(|_| {
                        CommonError::InvalidData(format!("invalid hours in timecode: {}", s))
                    })?;
                    let mins: u64 = parts[1].parse().map_err(|_| {
                        CommonError::InvalidData(format!("invalid minutes in timecode: {}", s))
                    })?;
                    if mins >= 60 {
                        return Err(CommonError::InvalidData(format!(
                            "minutes component must be less than 60 in timecode: {}",
                            s
                        )));
                    }
                    let sec_nanos = parse_seconds_nanos(parts[2], s)?;
                    if sec_nanos >= 60_000_000_000 {
                        return Err(CommonError::InvalidData(format!(
                            "seconds component must be less than 60 in timecode: {}",
                            s
                        )));
                    }
                    let total_nanos = hours
                        .checked_mul(3600 * 1_000_000_000)
                        .and_then(|h| {
                            mins.checked_mul(60 * 1_000_000_000).and_then(|m| h.checked_add(m))
                        })
                        .and_then(|hm| hm.checked_add(sec_nanos))
                        .ok_or_else(|| {
                            CommonError::InvalidData(format!("timecode overflow: {}", s))
                        })?;
                    Ok(Self(total_nanos))
                }
                _ => Err(CommonError::InvalidData(format!(
                    "malformed timecode (expected 1 or 2 colons): {}",
                    s
                ))),
            }
        } else {
            // Raw seconds (e.g. "45", "12.345", "10s")
            let stripped = trimmed.trim_end_matches(['s', 'S']);
            let sec_nanos = parse_seconds_nanos(stripped, s)?;
            Ok(Self(sec_nanos))
        }
    }
}

fn parse_seconds_nanos(sec_str: &str, original_input: &str) -> Result<u64, CommonError> {
    let sec_str = sec_str.trim();
    if sec_str.is_empty() {
        return Err(CommonError::InvalidData(format!(
            "empty seconds field in timecode: {}",
            original_input
        )));
    }
    let (whole_str, frac_str) = match sec_str.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (sec_str, None),
    };
    let whole_secs: u64 = whole_str.parse().map_err(|_| {
        CommonError::InvalidData(format!("invalid seconds component: {}", original_input))
    })?;
    let mut nanos = whole_secs.checked_mul(1_000_000_000).ok_or_else(|| {
        CommonError::InvalidData(format!("seconds overflow: {}", original_input))
    })?;
    if let Some(frac) = frac_str {
        if frac.is_empty() {
            return Err(CommonError::InvalidData(format!(
                "invalid fractional seconds in timecode: {}",
                original_input
            )));
        }
        if !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(CommonError::InvalidData(format!(
                "invalid digits in fractional seconds: {}",
                original_input
            )));
        }
        let truncated = if frac.len() > 9 { &frac[..9] } else { frac };
        let padded = format!("{:0<9}", truncated);
        let frac_nanos: u64 = padded.parse().map_err(|_| {
            CommonError::InvalidData(format!(
                "failed to parse fractional seconds: {}",
                original_input
            ))
        })?;
        nanos = nanos.checked_add(frac_nanos).ok_or_else(|| {
            CommonError::InvalidData(format!("timestamp overflow: {}", original_input))
        })?;
    }
    Ok(nanos)
}

impl FromStr for MediaTime {
    type Err = CommonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_timecode_str(s)
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

    #[test]
    fn test_media_time_from_timecode_str() {
        // HH:MM:SS and HH:MM:SS.mmm
        let t1 = MediaTime::from_timecode_str("01:02:03.500").unwrap();
        assert_eq!(t1.as_millis(), 3600 * 1000 + 2 * 60 * 1000 + 3500);
        assert_eq!(t1.to_timecode_string(), "01:02:03.500");

        let t2 = MediaTime::from_timecode_str("00:00:10").unwrap();
        assert_eq!(t2.as_millis(), 10000);

        // MM:SS and MM:SS.mmm
        let t3 = MediaTime::from_timecode_str("05:30.250").unwrap();
        assert_eq!(t3.as_millis(), 5 * 60 * 1000 + 30250);

        let t4 = MediaTime::from_timecode_str("00:45").unwrap();
        assert_eq!(t4.as_millis(), 45000);

        // Raw seconds
        let t5 = MediaTime::from_timecode_str("90").unwrap();
        assert_eq!(t5.as_millis(), 90000);

        let t6 = MediaTime::from_timecode_str("12.5s").unwrap();
        assert_eq!(t6.as_millis(), 12500);

        // FromStr trait
        let t7: MediaTime = "02:15:00.000".parse().unwrap();
        assert_eq!(t7.as_millis(), 2 * 3600 * 1000 + 15 * 60 * 1000);

        // Sentinel
        let unanchored = MediaTime::from_timecode_str("--:--:--.---").unwrap();
        assert_eq!(unanchored, MediaTime::UNANCHORED);

        // Errors
        assert!(MediaTime::from_timecode_str("").is_err());
        assert!(MediaTime::from_timecode_str("invalid").is_err());
        assert!(MediaTime::from_timecode_str("01:65:00").is_err()); // minutes >= 60
        assert!(MediaTime::from_timecode_str("01:20:65").is_err()); // seconds >= 60
        assert!(MediaTime::from_timecode_str("10:70").is_err());    // seconds >= 60
        assert!(MediaTime::from_timecode_str("01:02:03:04").is_err()); // too many colons
    }

    #[test]
    fn test_media_time_extended_helpers() {
        let t1 = MediaTime::from_millis(3600 * 1000 + 120 * 1000 + 45 * 1000 + 500);
        assert_eq!(t1.to_hms(), "01:02:45");
        assert_eq!(t1.to_friendly_duration(), "1h 2m 45s");

        let t2 = MediaTime::from_millis(5 * 60 * 1000 + 30 * 1000);
        assert_eq!(t2.to_hms(), "00:05:30");
        assert_eq!(t2.to_friendly_duration(), "5m 30s");

        let t3 = MediaTime::from_millis(15 * 1000);
        assert_eq!(t3.to_hms(), "00:00:15");
        assert_eq!(t3.to_friendly_duration(), "15s");

        assert_eq!(MediaTime::UNANCHORED.to_hms(), "--:--:--");
        assert_eq!(MediaTime::UNANCHORED.to_friendly_duration(), "unknown");

        let total = MediaTime::from_millis(100_000);
        let half = MediaTime::from_millis(50_000);
        assert!((half.percent_of(total) - 50.0).abs() < 1e-6);
        assert_eq!(MediaTime::from_percent(total, 50.0).as_millis(), 50_000);

        // Edge cases
        assert_eq!(half.percent_of(MediaTime::ZERO), 0.0);
        assert_eq!(half.percent_of(MediaTime::UNANCHORED), 0.0);
        assert_eq!(MediaTime::UNANCHORED.percent_of(total), 0.0);
        assert_eq!(MediaTime::from_percent(total, -10.0), MediaTime::ZERO);
        assert_eq!(MediaTime::from_percent(total, 150.0), total);
        assert_eq!(MediaTime::from_percent(MediaTime::ZERO, 50.0), MediaTime::ZERO);
    }
}
