//! NTP 4-timestamp probe exchange and RFC 5905 sliding-window minimum RTT filter.
/// An individual 4-timestamp NTP probe measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeSample {
    /// Client transmit monotonic timestamp in nanoseconds.
    pub t1: u64,
    /// Server receive monotonic timestamp in nanoseconds.
    pub t2: u64,
    /// Server reply transmit monotonic timestamp in nanoseconds.
    pub t3: u64,
    /// Client reply receive monotonic timestamp in nanoseconds.
    pub t4: u64,
    /// Estimated clock offset theta in nanoseconds: `((t2 - t1) + (t3 - t4)) / 2`.
    pub offset: f64,
    /// Round-trip time in nanoseconds: `(t4 - t1) - (t3 - t2)`.
    pub rtt: f64,
}

impl ProbeSample {
    /// Constructs a `ProbeSample` from 4 monotonic nanosecond timestamps.
    pub fn new(t1: u64, t2: u64, t3: u64, t4: u64) -> Self {
        let offset = ((t2 as f64 - t1 as f64) + (t3 as f64 - t4 as f64)) / 2.0;
        let rtt = (t4 as f64 - t1 as f64) - (t3 as f64 - t2 as f64);
        Self {
            t1,
            t2,
            t3,
            t4,
            offset,
            rtt,
        }
    }
}

/// RFC 5905 8-sample sliding-window minimum RTT filter with effective dispersion floor and watchdog flush.
#[derive(Debug, Clone)]
pub struct Rfc5905Filter {
    window_size: usize,
    min_dispersion_floor_ns: f64,
    window: Vec<ProbeSample>,
    consecutive_rejections: usize,
    accepted_count: usize,
    rejected_count: usize,
}

impl Default for Rfc5905Filter {
    fn default() -> Self {
        Self::new(8, 5_000_000.0) // 8 samples, 5.0ms dispersion floor
    }
}

impl Rfc5905Filter {
    /// Constructs an `Rfc5905Filter` with specified window size and dispersion floor.
    pub fn new(window_size: usize, min_dispersion_floor_ns: f64) -> Self {
        Self {
            window_size: window_size.max(1),
            min_dispersion_floor_ns,
            window: Vec::with_capacity(window_size),
            consecutive_rejections: 0,
            accepted_count: 0,
            rejected_count: 0,
        }
    }

    /// Computes the RFC 5905 window dispersion $D = \sum_{i=0}^{W-1} \frac{|\theta_i - \theta_0|}{2^{i+1}}$.
    pub fn compute_dispersion(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        let mut sorted = self.window.clone();
        sorted.sort_by(|a, b| a.rtt.partial_cmp(&b.rtt).unwrap());
        let s0 = sorted[0];

        let mut dispersion = 0.0;
        for (i, s) in sorted.iter().enumerate() {
            let weight = 2.0f64.powi((i + 1) as i32);
            dispersion += (s.offset - s0.offset).abs() / weight;
        }
        dispersion
    }

    /// Returns the effective window dispersion: $\max(D, D_{\text{min}})$.
    #[inline]
    pub fn get_effective_dispersion(&self) -> f64 {
        self.compute_dispersion().max(self.min_dispersion_floor_ns)
    }

    /// Returns the filtered authoritative clock offset $\hat{\theta}$ from the minimum RTT sample.
    pub fn get_filtered_offset(&self) -> Option<f64> {
        if self.window.is_empty() {
            return None;
        }
        self.window
            .iter()
            .min_by(|a, b| a.rtt.partial_cmp(&b.rtt).unwrap())
            .map(|s| s.offset)
    }

    /// Returns the median RTT of all samples currently in the window.
    pub fn get_median_rtt(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        let mut rtts: Vec<f64> = self.window.iter().map(|s| s.rtt).collect();
        rtts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = rtts.len();
        if n % 2 == 1 {
            rtts[n / 2]
        } else {
            (rtts[n / 2 - 1] + rtts[n / 2]) / 2.0
        }
    }

    /// Evaluates a candidate probe sample against outlier criteria and inserts if accepted.
    ///
    /// Outlier criteria (when window is full):
    /// - `rtt > 2.0 * median(rtt)`
    /// - `|offset - theta_hat| > 3.0 * D_effective`
    ///
    /// Watchdog:
    /// - 8 consecutive rejections flushes stale window and seeds with candidate sample.
    pub fn process_sample(&mut self, sample: ProbeSample) -> bool {
        if self.window.len() >= self.window_size {
            let med_rtt = self.get_median_rtt();
            let dispersion_eff = self.get_effective_dispersion();
            let filtered_theta = self.get_filtered_offset().unwrap_or(0.0);

            let rtt_outlier = sample.rtt > 2.0 * med_rtt;
            let dispersion_outlier = (sample.offset - filtered_theta).abs() > 3.0 * dispersion_eff;

            if rtt_outlier || dispersion_outlier {
                self.consecutive_rejections += 1;
                if self.consecutive_rejections >= 8 {
                    // Watchdog trip: path reroute or persistent step shift
                    self.window.clear();
                    self.window.push(sample);
                    self.consecutive_rejections = 0;
                    self.accepted_count += 1;
                    return true;
                }
                self.rejected_count += 1;
                return false;
            }
        }

        // Accepted
        self.consecutive_rejections = 0;
        self.accepted_count += 1;
        if self.window.len() >= self.window_size {
            self.window.remove(0);
        }
        self.window.push(sample);
        true
    }

    /// Returns current number of samples in the window.
    pub fn sample_count(&self) -> usize {
        self.window.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ntp_filter_outlier_rejection_and_watchdog() {
        let mut filter = Rfc5905Filter::new(8, 5_000_000.0);

        // Seed with 8 stable samples: RTT = 250ms, offset = +50ms
        for i in 0..8 {
            let s = ProbeSample::new(
                i * 1_000_000_000,
                i * 1_000_000_000 + 175_000_000,
                i * 1_000_000_000 + 180_000_000,
                i * 1_000_000_000 + 255_000_000,
            );
            assert!(filter.process_sample(s));
        }

        assert_eq!(filter.sample_count(), 8);
        assert!((filter.get_filtered_offset().unwrap() - 50_000_000.0).abs() < 1e-3);

        // Asymmetric delay spike: offset error = +75ms -> rejected!
        let asym = ProbeSample::new(
            10_000_000_000,
            10_250_000_000,
            10_255_000_000,
            10_255_000_000,
        );
        assert!(!filter.process_sample(asym));

        // Legitimate +10ms step: delta = 10ms <= 3 * 5ms = 15ms -> accepted!
        let step = ProbeSample::new(
            11_000_000_000,
            11_185_000_000,
            11_190_000_000,
            11_245_000_000,
        );
        assert!(filter.process_sample(step));

        // Route change: RTT doubles to 210ms (> 2.0 * median)
        // Watchdog triggers within 8 rejections
        let mut rejected = 0;
        let mut accepted = 0;
        for k in 0..50 {
            let s = ProbeSample::new(
                (12 + k) * 1_000_000_000,
                (12 + k) * 1_000_000_000 + 105_000_000,
                (12 + k) * 1_000_000_000 + 110_000_000,
                (12 + k) * 1_000_000_000 + 650_000_000, // RTT > 2.0 * median
            );
            if filter.process_sample(s) {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        assert!(rejected <= 8, "Watchdog must trigger within 8 rejections");
        assert!(accepted >= 42);
    }
}
