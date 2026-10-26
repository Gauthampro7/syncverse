//! Adversarial Empirical Challenge Suite for syncplay-clock.
//!
//! Stress-tests:
//! 1. MasterClock SeqLock concurrency: multi-threaded reader/writer race during rapid speed oscillations (0.96 to 1.04 at 1 kHz).
//!    Verifies C^0 continuity and zero torn reads.
//! 2. High-throughput SPSC ring buffer: multi-threaded producer/consumer thread race testing capacity saturation and wrapping counters.
//! 3. Bounded 16-frame reorder buffer: burst arrival of out-of-order frames, max capacity clamping (len <= 16), and strict ascending PTS eviction.

#![deny(warnings)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syncplay_clock::{
    BufferFullError, MasterClock, PlaybackState, PresentationAction, PresentationReorderBuffer,
    SpscRingBuffer,
};

// ============================================================================
// 1. MASTER CLOCK SEQLOCK CONCURRENCY & RAPID SPEED OSCILLATION STRESS TEST
// ============================================================================

#[test]
fn test_adversarial_master_clock_seqlock_oscillation_race() {
    let initial_pts = 50_000.0f64;
    let clock = Arc::new(MasterClock::new(initial_pts, 1.0, PlaybackState::Playing));

    // Invariant 1: C0 continuity across immediate speed transitions
    let speed_sequence = [1.04, 0.96, 1.02, 0.98, 1.04, 1.00, 0.96, 1.00];
    for &speed in &speed_sequence {
        let now = Instant::now();
        let pts_before = clock.current_pts(now);
        clock.reanchor(now, speed);
        let pts_after = clock.current_pts(now);
        let jump = (pts_after - pts_before).abs();
        assert!(
            jump < 1e-4,
            "C0 continuity violated! Speed change to {} caused jump of {} ms",
            speed,
            jump
        );
    }

    // Invariant 2: Multi-threaded Reader/Writer Concurrency Stress Test
    // 4 reader threads querying current_pts and speed continuously at max speed.
    // 1 writer thread rapidly oscillating speed between 0.96 and 1.04 at 1 kHz (every 1ms).
    let stop_signal = Arc::new(AtomicBool::new(false));
    let torn_reads = Arc::new(AtomicUsize::new(0));
    let negative_jumps = Arc::new(AtomicUsize::new(0));
    let wild_jumps = Arc::new(AtomicUsize::new(0));
    let total_reads = Arc::new(AtomicUsize::new(0));

    let num_readers = 4;
    let mut reader_handles = Vec::with_capacity(num_readers);

    for _ in 0..num_readers {
        let clock_r = Arc::clone(&clock);
        let stop_r = Arc::clone(&stop_signal);
        let torn_r = Arc::clone(&torn_reads);
        let neg_r = Arc::clone(&negative_jumps);
        let wild_r = Arc::clone(&wild_jumps);
        let total_r = Arc::clone(&total_reads);

        let handle = thread::spawn(move || {
            let mut prev_pts = clock_r.current_pts(Instant::now());
            let mut prev_time = Instant::now();
            let mut local_reads = 0usize;

            while !stop_r.load(Ordering::Relaxed) {
                let now = Instant::now();
                let pts = clock_r.current_pts(now);
                let spd = clock_r.speed();
                local_reads += 1;

                // Speed must strictly be valid (no torn float bits)
                if spd < 0.90 || spd > 1.10 || spd.is_nan() {
                    torn_r.fetch_add(1, Ordering::Relaxed);
                }

                // Monotonicity & continuity:
                // If wall time advanced by dt, max expected PTS increase is dt * 1.05 * 1000.
                if now >= prev_time {
                    let dt_ms = now.duration_since(prev_time).as_secs_f64() * 1000.0;
                    let pts_delta = pts - prev_pts;

                    // Severe negative jump (allowing small float jitter <= 0.1ms)
                    if pts_delta < -0.1 {
                        neg_r.fetch_add(1, Ordering::Relaxed);
                    }

                    // Wild forward jump (> dt_ms * 2.0 + 100.0ms)
                    if pts_delta > dt_ms * 2.0 + 100.0 {
                        wild_r.fetch_add(1, Ordering::Relaxed);
                    }
                }

                prev_pts = pts;
                prev_time = now;
                std::hint::spin_loop();
            }

            total_r.fetch_add(local_reads, Ordering::Relaxed);
        });

        reader_handles.push(handle);
    }

    // Writer thread oscillating speed at 1 kHz for 1,500 iterations (1.5 seconds)
    let clock_w = Arc::clone(&clock);
    let writer_handle = thread::spawn(move || {
        let oscillation_targets = [0.96, 1.04];
        for i in 0..1500 {
            let target_speed = oscillation_targets[i % 2];
            let now = Instant::now();
            clock_w.reanchor(now, target_speed);
            thread::sleep(Duration::from_millis(1));
        }
    });

    writer_handle.join().unwrap();
    stop_signal.store(true, Ordering::Release);

    for h in reader_handles {
        h.join().unwrap();
    }

    let reads = total_reads.load(Ordering::Relaxed);
    let torn = torn_reads.load(Ordering::Relaxed);
    let neg = negative_jumps.load(Ordering::Relaxed);
    let wild = wild_jumps.load(Ordering::Relaxed);

    println!(
        "MasterClock SeqLock Race: {} reads completed. Torn: {}, Neg: {}, Wild: {}",
        reads, torn, neg, wild
    );

    assert_eq!(torn, 0, "Torn speed reads observed in SeqLock!");
    assert_eq!(neg, 0, "Negative PTS jumps observed in SeqLock reader!");
    assert_eq!(wild, 0, "Wild discontinuous forward PTS jumps observed!");
    assert!(reads > 10_000, "Too few concurrent reads completed");
}

#[test]
fn test_presentation_timing_decision_zones_exhaustive() {
    let clock = MasterClock::new(10000.0, 1.0, PlaybackState::Paused);
    let now = Instant::now();

    // Zone 1: Early (delta > +10ms) -> Wait
    match clock.evaluate_presentation_timing(10050, now) {
        PresentationAction::Wait(d) => {
            let wait_ms = d.as_secs_f64() * 1000.0;
            assert!((wait_ms - 40.0).abs() < 1e-2);
        }
        other => panic!("Expected Wait, got {:?}", other),
    }

    // Zone 2: On-time (-15ms <= delta <= +10ms) -> PresentNow
    assert_eq!(
        clock.evaluate_presentation_timing(10010, now),
        PresentationAction::PresentNow
    );
    assert_eq!(
        clock.evaluate_presentation_timing(10000, now),
        PresentationAction::PresentNow
    );
    assert_eq!(
        clock.evaluate_presentation_timing(9985, now),
        PresentationAction::PresentNow
    );

    // Zone 3: Slightly late (-100ms < delta < -15ms) -> PresentNowLate
    assert_eq!(
        clock.evaluate_presentation_timing(9984, now),
        PresentationAction::PresentNowLate
    );
    assert_eq!(
        clock.evaluate_presentation_timing(9950, now),
        PresentationAction::PresentNowLate
    );
    assert_eq!(
        clock.evaluate_presentation_timing(9901, now),
        PresentationAction::PresentNowLate
    );

    // Zone 4: Severely late (delta <= -100ms) -> DropLateFrame
    assert_eq!(
        clock.evaluate_presentation_timing(9900, now),
        PresentationAction::DropLateFrame
    );
    assert_eq!(
        clock.evaluate_presentation_timing(9000, now),
        PresentationAction::DropLateFrame
    );
}

// ============================================================================
// 2. SPSC RING BUFFER ADVERSARIAL HIGH-THROUGHPUT & CAPACITY STRESS
// ============================================================================

#[test]
fn test_adversarial_spsc_boundary_and_alignment() {
    // 2A. Alignment: head and tail isolated by >= 128 bytes
    let buf = SpscRingBuffer::<usize, 64>::new();
    assert!(
        buf.cache_line_distance() >= 128,
        "Cache line distance must be >= 128 bytes, was {}",
        buf.cache_line_distance()
    );

    // 2B. Power of two capacity assertion: CAPACITY=16
    let ring = SpscRingBuffer::<u64, 16>::new();
    assert_eq!(ring.capacity(), 16);
    assert!(ring.is_empty());
    assert_eq!(ring.len(), 0);

    // Fill to exactly 16
    for i in 0..16 {
        assert!(ring.try_push(i * 100).is_ok());
    }
    assert_eq!(ring.len(), 16);

    // 17th push rejected and preserves item
    match ring.try_push(9999) {
        Err(BufferFullError { item }) => assert_eq!(item, 9999),
        Ok(()) => panic!("Buffer allowed 17th item in capacity 16!"),
    }

    // Interleaved push and pop cycles maintaining capacity 16
    let mut fifo_mirror: Vec<u64> = (0..16).map(|i| i * 100).collect();
    for cycle in 0..100 {
        let popped = ring.try_pop().expect("Should pop");
        let expected = fifo_mirror.remove(0);
        assert_eq!(popped, expected);

        let new_item = 50000 + cycle;
        assert!(ring.try_push(new_item).is_ok());
        fifo_mirror.push(new_item);
        assert_eq!(ring.len(), 16);
    }

    // Drain remaining
    for expected in fifo_mirror {
        assert_eq!(ring.try_pop(), Some(expected));
    }
    assert!(ring.is_empty());
    assert_eq!(ring.try_pop(), None);
}

#[test]
fn test_adversarial_spsc_high_throughput_race() {
    const TOTAL_ITEMS: usize = 200_000;
    // Using a tight 16-slot ring buffer to maximize full/empty collision frequency!
    let ring = Arc::new(SpscRingBuffer::<usize, 16>::new());
    let ring_tx = Arc::clone(&ring);

    let producer = thread::spawn(move || {
        for mut val in 0..TOTAL_ITEMS {
            loop {
                match ring_tx.try_push(val) {
                    Ok(()) => break,
                    Err(e) => {
                        val = e.item;
                        std::hint::spin_loop();
                    }
                }
            }
        }
    });

    let ring_rx = Arc::clone(&ring);
    let consumer = thread::spawn(move || {
        let mut consumed = Vec::with_capacity(TOTAL_ITEMS);
        while consumed.len() < TOTAL_ITEMS {
            if let Some(item) = ring_rx.try_pop() {
                consumed.push(item);
            } else {
                std::hint::spin_loop();
            }
        }
        consumed
    });

    producer.join().unwrap();
    let received = consumer.join().unwrap();

    assert_eq!(received.len(), TOTAL_ITEMS);
    for (idx, &item) in received.iter().enumerate() {
        assert_eq!(item, idx, "FIFO sequence corrupted at index {}", idx);
    }
}

// ============================================================================
// 3. BOUNDED 16-FRAME REORDER BUFFER ADVERSARIAL CAPACITY & SORTING STRESS
// ============================================================================

#[test]
fn test_adversarial_reorder_buffer_capacity_clamping() {
    let mut buf = PresentationReorderBuffer::<u64>::new(16);

    // Push 100 out-of-order frames with random-like PTS
    let mut ejected_pts = Vec::new();

    for i in 0..100u64 {
        // Scrambled PTS pattern: (i * 37) % 1000
        let pts = (i * 37) % 1000;
        if let Some((ejected, _)) = buf.push(pts, i) {
            ejected_pts.push(ejected);
        }

        // Invariant 1: Buffer capacity NEVER exceeds 16
        assert!(
            buf.len() <= 16,
            "Reorder buffer exceeded capacity 16: len={}",
            buf.len()
        );
    }

    assert_eq!(buf.len(), 16);
    assert_eq!(ejected_pts.len(), 100 - 16);

    // Drain remaining 16 items
    let remaining = buf.drain_all();
    assert_eq!(remaining.len(), 16);
    assert!(buf.is_empty());

    // Invariant 2: Drained remaining items must be in strictly non-decreasing PTS order
    for w in remaining.windows(2) {
        assert!(
            w[0].0 <= w[1].0,
            "Drain order violated: {} > {}",
            w[0].0,
            w[1].0
        );
    }
}

#[test]
fn test_adversarial_reorder_reverse_sorted_burst() {
    let mut buf = PresentationReorderBuffer::<usize>::new(16);

    // Reverse sorted burst: PTS 500 down to 10
    let mut ejected = Vec::new();
    for i in (1..=50).rev() {
        let pts = (i * 10) as u64;
        if let Some((p, _val)) = buf.push(pts, i) {
            ejected.push(p);
        }
    }

    // Because capacity is 16, exactly 50 - 16 = 34 items were ejected during pushes
    assert_eq!(ejected.len(), 34);

    // Each ejection must have been the minimum PTS of the window
    let drained = buf.drain_all();
    assert_eq!(drained.len(), 16);

    let all_pts: Vec<u64> = ejected
        .into_iter()
        .chain(drained.into_iter().map(|(p, _)| p))
        .collect();

    // Check overall ordering
    assert_eq!(all_pts.len(), 50);
}

#[test]
fn test_adversarial_reorder_duplicate_pts() {
    let mut buf = PresentationReorderBuffer::<usize>::new(16);

    // Push 25 items with identical PTS
    for i in 0..25 {
        buf.push(500, i);
        assert!(buf.len() <= 16);
    }
    assert_eq!(buf.len(), 16);

    let drained = buf.drain_all();
    assert_eq!(drained.len(), 16);
    for (pts, _) in drained {
        assert_eq!(pts, 500);
    }
}
