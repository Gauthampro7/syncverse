//! Acceptance tests for ResumeTracker policy.
use syncverse_common::userdata::{FileKey, ResumeStore};
use syncverse_core::resume::*;

const MIN: u64 = 60_000;
fn key() -> FileKey {
    FileKey { size: 1, head_crc: 2, tail_crc: 3 }
}
fn mem() -> FileMemory {
    FileMemory::default()
}
const DUR: u64 = 100 * MIN;

fn tracker_with_pos(pos: u64, policy: ResumePolicy) -> ResumeTracker {
    let mut t = ResumeTracker::with_store(ResumeStore::new(), None, policy);
    t.on_open(key(), DUR, false, 0);
    t.on_stop(pos, &mem(), false, 1);
    t
}

fn decide(pos: u64, policy: ResumePolicy, party: bool) -> ResumeDecision {
    let mut t = tracker_with_pos(pos, policy);
    t.on_open(key(), DUR, party, 10).decision
}

#[test]
fn thresholds() {
    let a = ResumePolicy::Ask;
    assert_eq!(decide(29_999, a, false), ResumeDecision::None);
    assert_eq!(decide(30_000, a, false), ResumeDecision::Offer { pos_ms: 30_000 });
    assert_eq!(decide(31_000, a, false), ResumeDecision::Offer { pos_ms: 31_000 });
    let edge = DUR - 60_000;
    // positions past the end margin are cleared at save time
    assert_eq!(decide(edge, a, false), ResumeDecision::Offer { pos_ms: edge });
    assert_eq!(decide(DUR - 59_000, a, false), ResumeDecision::None);
    assert!(is_resumable(30_000, DUR) && !is_resumable(29_999, DUR));
    assert!(!is_resumable(40_000, 0) && !is_resumable(40_000, 80_000));
}

#[test]
fn eof_clears() {
    let mut t = tracker_with_pos(40 * MIN, ResumePolicy::Ask);
    t.on_open(key(), DUR, false, 5);
    t.on_stop(DUR, &mem(), true, 6);
    assert_eq!(t.on_open(key(), DUR, false, 7).decision, ResumeDecision::None);
    assert!(t.store().get(&key()).is_none());
}

#[test]
fn party_suppresses_offer_and_keeps_old_position() {
    let mut t = tracker_with_pos(40 * MIN, ResumePolicy::Always);
    assert_eq!(t.on_open(key(), DUR, true, 10).decision, ResumeDecision::None);
    t.on_stop(5 * MIN, &mem(), false, 11);
    assert_eq!(t.store().get(&key()).unwrap().pos_ms, 40 * MIN, "room timeline must not overwrite");
}

#[test]
fn policies() {
    assert_eq!(decide(40 * MIN, ResumePolicy::Never, false), ResumeDecision::None);
    assert_eq!(
        decide(40 * MIN, ResumePolicy::Always, false),
        ResumeDecision::AutoResume { pos_ms: 40 * MIN }
    );
    for p in [ResumePolicy::Ask, ResumePolicy::Always, ResumePolicy::Never] {
        assert_eq!(ResumePolicy::parse(p.as_str()), Some(p));
    }
    assert_eq!(ResumePolicy::parse("bogus"), None);
}

#[test]
fn save_throttling() {
    let mut t = ResumeTracker::with_store(ResumeStore::new(), None, ResumePolicy::Ask);
    t.on_open(key(), DUR, false, 0);
    let mut now = 0;
    // 20 s of ticks at 100 ms: at most 4 throttled writes
    for i in 0..200u64 {
        now = i * 100;
        t.on_tick(40 * MIN + now, true, &mem(), now);
    }
    assert!(t.writes() <= 4 && t.writes() >= 3, "writes = {}", t.writes());
    assert!(!t.on_tick(1, false, &mem(), now + 10_000), "paused ticks never write");
    let w = t.writes();
    assert!(t.on_pause(41 * MIN, &mem(), now + 10_001), "pause forces a save");
    assert_eq!(t.writes(), w + 1);
}

#[test]
fn per_file_memory_restored_and_persisted() {
    let dir = std::env::temp_dir().join(format!("sv_resume_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("resume.tsv");
    let m = FileMemory { audio: 2, sub: -2, audio_delay_ms: -150, sub_delay_ms: 400, aspect: "16:9".into() };
    {
        let mut t = ResumeTracker::load(Some(&path), ResumePolicy::Ask);
        assert!(t.on_open(key(), DUR, false, 0).memory.is_none());
        t.on_stop(10_000, &m, false, 1); // below threshold: tracks remembered, no offer
    }
    let mut t = ResumeTracker::load(Some(&path), ResumePolicy::Ask);
    let o = t.on_open(key(), DUR, false, 0);
    assert_eq!(o.decision, ResumeDecision::None);
    assert_eq!(o.memory, Some(m.clone()));
    // memory also restored inside a party
    assert_eq!(t.on_open(key(), DUR, true, 1).memory, Some(m.clone()));
    t.set_remember_per_file(false);
    assert!(t.on_open(key(), DUR, false, 2).memory.is_none());
    // open-other flushes previous
    t.set_remember_per_file(true);
    t.on_open(key(), DUR, false, 3);
    t.on_tick(50 * MIN, false, &m, 4);
    let k2 = FileKey { size: 9, head_crc: 9, tail_crc: 9 };
    t.on_open(k2, DUR, false, 5);
    assert_eq!(t.store().get(&key()).unwrap().pos_ms, 50 * MIN);
    // decline keeps tracks, drops position
    t.decline(&key(), 6);
    let e = t.store().get(&key()).unwrap();
    assert_eq!((e.pos_ms, e.audio), (0, 2));
    assert!(t.take_error().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
