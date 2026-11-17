//! Acceptance tests for the pure frame-step planner.

use syncverse_core::frame_step::*;

fn frames(fps: f64, n: usize) -> Vec<u64> {
    (0..n).map(|i| (i as f64 * 1000.0 / fps).round() as u64).collect()
}

/// Simulated player: `idx` into a frame table, history fed like the runtime would.
struct Sim {
    table: Vec<u64>,
    idx: usize,
    hist: PtsHistory,
    fps: f64,
}

impl Sim {
    fn new(fps: f64, table: Vec<u64>, idx: usize) -> Self {
        let mut hist = PtsHistory::new();
        hist.push(table[idx]);
        Self { table, idx, hist, fps }
    }
    fn step(&mut self, forward: bool) {
        let dur = *self.table.last().unwrap();
        match plan_step_for_role(StepRole::Solo, self.hist.as_slice(), self.fps, forward, Some(dur)).unwrap() {
            StepPlan::PresentNextDecoded { .. } => {
                if self.idx + 1 < self.table.len() {
                    self.idx += 1;
                }
            }
            StepPlan::SeekExact { target_ms } => {
                // Exact seek lands on the frame with this PTS.
                self.idx = self.table.iter().position(|p| *p >= target_ms).expect("target inside media");
            }
            StepPlan::NoOp => {}
        }
        self.hist.push(self.table[self.idx]);
    }
    fn pts(&self) -> u64 {
        self.table[self.idx]
    }
}

#[test]
fn table_fps_forward_and_back_roundtrip() {
    for fps in [24.0, 23.976, 25.0, 29.97, 30.0, 60.0] {
        let table = frames(fps, 400);
        let mut sim = Sim::new(fps, table.clone(), 100);
        let start = sim.pts();
        for k in 1..=10 {
            sim.step(true);
            assert_eq!(sim.pts(), table[100 + k], "fps {fps} forward {k}");
        }
        for _ in 0..10 {
            sim.step(false);
        }
        assert_eq!(sim.pts(), start, "fps {fps}: fwd x10 then back x10");
    }
}

#[test]
fn forward_fallback_is_one_frame_duration() {
    for (fps, ms) in [(24.0, 41), (25.0, 39), (30.0, 32), (60.0, 16)] {
        match plan_step(&[1000], fps, true) {
            StepPlan::PresentNextDecoded { fallback_target_ms } => assert_eq!(fallback_target_ms, 1000 + ms, "fps {fps}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn vfr_uses_real_previous_pts() {
    let hist = [0, 40, 80, 200, 233];
    assert_eq!(plan_step(&hist, 25.0, false), StepPlan::SeekExact { target_ms: 200 });
    let hist = [0, 40, 80, 200];
    assert_eq!(plan_step(&hist, 25.0, false), StepPlan::SeekExact { target_ms: 80 });
}

#[test]
fn backward_at_zero_is_noop() {
    assert_eq!(plan_step(&[0], 25.0, false), StepPlan::NoOp);
    assert_eq!(plan_step(&[], 25.0, false), StepPlan::NoOp);
}

#[test]
fn unknown_fps_falls_back_to_40ms() {
    for fps in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(plan_step(&[500], fps, true), StepPlan::PresentNextDecoded { fallback_target_ms: 539 });
        assert_eq!(plan_step(&[500], fps, false), StepPlan::SeekExact { target_ms: 459 });
    }
}

#[test]
fn backward_with_no_earlier_history_clamps_at_zero() {
    assert_eq!(plan_step(&[10], 25.0, false), StepPlan::SeekExact { target_ms: 0 });
}

#[test]
fn client_step_is_disabled_decision() {
    // DECISION: a party client never steps (PLL would fight a local seek; host step = broadcast seek).
    assert_eq!(
        plan_step_for_role(StepRole::Client, &[1000], 25.0, true, None),
        Err(StepError::DisabledInPartyClient)
    );
    assert!(plan_step_for_role(StepRole::Host, &[1000], 25.0, true, None).is_ok());
    assert!(plan_step_for_role(StepRole::Solo, &[1000], 25.0, false, None).is_ok());
    assert!(!CLIENT_STEP_DISABLED_HUD.is_empty());
}

#[test]
fn step_at_end_is_noop() {
    assert_eq!(
        plan_step_for_role(StepRole::Solo, &[10_000], 25.0, true, Some(10_000)).unwrap(),
        StepPlan::NoOp
    );
}

#[test]
fn history_is_bounded_and_ascending() {
    let mut h = PtsHistory::new();
    for i in 0..100u64 {
        h.push(i * 40);
    }
    assert_eq!(h.as_slice().len(), PTS_HISTORY_LEN);
    h.push(3000);
    assert_eq!(*h.as_slice().last().unwrap(), 3000);
    assert!(h.as_slice().windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn random_step_sequences_stay_in_range() {
    let mut seed: u64 = 0x1234_5678_9abc_def1;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let rates = [24.0, 23.976, 25.0, 29.97, 30.0, 60.0];
    for run in 0..1000 {
        let fps = rates[run % rates.len()];
        let table = frames(fps, 120);
        let duration = *table.last().unwrap();
        let mut sim = Sim::new(fps, table, (rnd() % 120) as usize);
        for _ in 0..200 {
            sim.step(rnd() % 2 == 0);
            assert!(sim.pts() <= duration, "pts {} beyond {duration}", sim.pts());
        }
    }
}
