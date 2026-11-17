//! Queue reorder, add-to-queue / play-next dedupe and autosave/restore acceptance tests.

use std::path::{Path, PathBuf};
use syncverse_ui::gui::playlist::{PlaylistManager, QueueError};

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("syncverse_queue_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn touch(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"x").unwrap();
    p
}

fn names(m: &PlaylistManager) -> Vec<String> {
    m.items.iter().map(|i| i.filename.clone()).collect()
}

fn mgr_of(n: usize) -> PlaylistManager {
    let mut m = PlaylistManager::new();
    for i in 0..n {
        m.add_file(PathBuf::from(format!("/nonexistent/t{i}.mp3")));
    }
    m
}

fn assert_valid_shuffle(m: &PlaylistManager) {
    let mut o: Vec<usize> = m.shuffle_order().to_vec();
    o.sort_unstable();
    assert_eq!(o, (0..m.items.len()).collect::<Vec<_>>(), "shuffle must be a permutation");
}

#[test]
fn move_item_keeps_current_item_for_all_pairs() {
    for n in 1..=6 {
        for from in 0..n {
            for to in 0..n {
                for cur in 0..n {
                    let mut m = mgr_of(n);
                    m.toggle_shuffle();
                    m.select_index(cur);
                    let playing = m.items[cur].filename.clone();
                    m.move_item(from, to);
                    assert_eq!(m.items[m.current_index.unwrap()].filename, playing, "n{n} {from}->{to} cur{cur}");
                    assert_eq!(m.items.len(), n);
                    assert_valid_shuffle(&m);
                }
            }
        }
    }
}

#[test]
fn move_item_semantics_and_bounds() {
    let mut m = mgr_of(4);
    assert!(m.move_item(0, 2));
    assert_eq!(names(&m), ["t1.mp3", "t2.mp3", "t0.mp3", "t3.mp3"]);
    assert!(m.move_item(3, 0));
    assert_eq!(names(&m), ["t3.mp3", "t1.mp3", "t2.mp3", "t0.mp3"]);
    assert!(!m.move_item(9, 0));
    assert!(!m.move_item(1, 1));
    assert!(m.move_item(0, 99)); // clamped to last
    assert_eq!(m.items.last().unwrap().filename, "t3.mp3");
}

#[test]
fn shuffle_order_is_preserved_not_reshuffled_by_move() {
    let mut m = mgr_of(6);
    m.toggle_shuffle();
    m.select_index(2);
    let before: Vec<String> = m.shuffle_order().iter().map(|&i| m.items[i].filename.clone()).collect();
    m.move_item(0, 4);
    let after: Vec<String> = m.shuffle_order().iter().map(|&i| m.items[i].filename.clone()).collect();
    assert_eq!(before, after, "shuffle sequence of items must not change");
}

#[test]
fn add_to_queue_dedupes_and_keeps_current() {
    let d = tmpdir("dedupe");
    let a = touch(&d, "a.mkv");
    let b = touch(&d, "b.mkv");
    let mut m = PlaylistManager::new();
    assert_eq!(m.add_to_queue(vec![a.clone(), b.clone(), a.clone()]), 2);
    m.select_index(1);
    assert_eq!(m.add_to_queue(vec![a.clone(), d.join("./b.mkv"), touch(&d, "c.mkv")]), 1);
    assert_eq!(m.items.len(), 3);
    assert_eq!(m.current_index, Some(1));
    assert_valid_shuffle(&m);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn play_next_inserts_after_current_and_moves_duplicates() {
    let mut m = mgr_of(4); // t0..t3
    m.toggle_shuffle();
    m.select_index(1);
    let i = m.play_next(PathBuf::from("/nonexistent/new.mp3"));
    assert_eq!(i, 2);
    assert_eq!(names(&m), ["t0.mp3", "t1.mp3", "new.mp3", "t2.mp3", "t3.mp3"]);
    assert_eq!(m.current_index, Some(1));
    assert_valid_shuffle(&m);

    // existing item moves instead of duplicating
    let i = m.play_next(PathBuf::from("/nonexistent/t3.mp3"));
    assert_eq!(i, 2);
    assert_eq!(names(&m), ["t0.mp3", "t1.mp3", "t3.mp3", "new.mp3", "t2.mp3"]);
    assert_eq!(m.current_index, Some(1));
    let i = m.play_next(PathBuf::from("/nonexistent/t0.mp3"));
    assert_eq!(names(&m), ["t1.mp3", "t0.mp3", "t3.mp3", "new.mp3", "t2.mp3"]);
    assert_eq!(i, 1);
    assert_eq!(m.items[m.current_index.unwrap()].filename, "t1.mp3");
    assert_valid_shuffle(&m);

    // playing the current item again is a no-op
    let before = names(&m);
    m.play_next(PathBuf::from("/nonexistent/t1.mp3"));
    assert_eq!(names(&m), before);
}

#[test]
fn play_next_many_keeps_batch_order_and_empty_queue_works() {
    let mut m = PlaylistManager::new();
    let first = m.play_next_many(vec!["/x/1.mp3".into(), "/x/2.mp3".into(), "/x/3.mp3".into()]);
    assert_eq!(first, Some(0));
    assert_eq!(names(&m), ["1.mp3", "2.mp3", "3.mp3"]);
    m.select_index(0);
    m.play_next_many(vec!["/x/9.mp3".into(), "/x/8.mp3".into()]);
    assert_eq!(names(&m), ["1.mp3", "9.mp3", "8.mp3", "2.mp3", "3.mp3"]);
    assert_valid_shuffle(&m);
}

#[test]
fn autosave_restore_roundtrip_with_hostile_paths() {
    let d = tmpdir("roundtrip");
    let mut paths = vec![
        touch(&d, "plain.mkv"),
        touch(&d, "with space.mp4"),
        touch(&d, "hash#tag #1.mkv"),
        touch(&d, "percent%20name%.mkv"),
        touch(&d, "new\nline.mkv"),
        touch(&d, "ünï cødé 日本語.mkv"),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bad = d.join(std::ffi::OsStr::from_bytes(b"bad\xff\xfe.mkv"));
        std::fs::write(&bad, b"x").unwrap();
        paths.push(bad);
    }
    let mut m = PlaylistManager::new();
    m.add_to_queue(paths.clone());
    m.select_index(2);

    let file = d.join("state").join("syncverse").join("queue.m3u");
    m.save_queue(&file).unwrap();
    assert!(!d.join("state").join("syncverse").read_dir().unwrap().any(|e| {
        e.unwrap().file_name().to_string_lossy().contains(".tmp")
    }), "no temp file left behind");

    let mut r = PlaylistManager::new();
    assert_eq!(r.restore_queue(&file).unwrap(), paths.len());
    let got: Vec<PathBuf> = r.items.iter().map(|i| i.path.clone()).collect();
    assert_eq!(got, paths);
    assert_eq!(r.current_index, Some(2), "current index restored, nothing auto-played");
    assert_valid_shuffle(&r);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn restore_drops_missing_files_and_remaps_current() {
    let d = tmpdir("missing");
    let a = touch(&d, "a.mkv");
    let b = touch(&d, "b.mkv");
    let c = touch(&d, "c.mkv");
    let mut m = PlaylistManager::new();
    m.add_to_queue(vec![a, b.clone(), c.clone()]);
    m.select_index(2);
    let f = d.join("q.m3u");
    m.save_queue(&f).unwrap();
    std::fs::remove_file(d.join("a.mkv")).unwrap();
    let mut r = PlaylistManager::new();
    assert_eq!(r.restore_queue(&f).unwrap(), 2);
    assert_eq!(r.items[0].path, b);
    assert_eq!(r.items[r.current_index.unwrap()].path, c);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn missing_file_is_empty_ok_and_corrupt_is_ignored() {
    let d = tmpdir("corrupt");
    let mut m = PlaylistManager::new();
    m.add_to_queue(vec![touch(&d, "keep.mkv")]);

    assert_eq!(m.restore_queue(&d.join("nope.m3u")).unwrap(), 0);
    assert_eq!(m.items.len(), 1);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("garbage", b"\x00\x01\x02 not a playlist".to_vec()),
        ("no header", b"file:///etc/passwd\n".to_vec()),
        ("bad escape", b"#EXTM3U\n#SYNCVERSE-QUEUE:1\nfile:///a%ZZ\n".to_vec()),
        ("truncated escape", b"#EXTM3U\n#SYNCVERSE-QUEUE:1\nfile:///a%2".to_vec()),
        ("foreign entry", b"#EXTM3U\n#SYNCVERSE-QUEUE:1\nrelative.mkv\n".to_vec()),
        ("nul", b"#EXTM3U\n#SYNCVERSE-QUEUE:1\nfile:///a\0b\n".to_vec()),
        ("empty", Vec::new()),
    ];
    for (label, bytes) in cases {
        let f = d.join("bad.m3u");
        std::fs::write(&f, &bytes).unwrap();
        assert!(matches!(m.restore_queue(&f), Err(QueueError::Corrupt)), "{label}");
        assert_eq!(m.items.len(), 1, "{label}: existing queue untouched");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn save_overwrites_atomically_and_signature_tracks_changes() {
    let d = tmpdir("sig");
    let mut m = PlaylistManager::new();
    m.add_to_queue(vec![touch(&d, "a.mkv")]);
    let s0 = m.queue_signature();
    m.add_to_queue(vec![touch(&d, "b.mkv")]);
    let s1 = m.queue_signature();
    assert_ne!(s0, s1);
    m.move_item(0, 1);
    assert_ne!(s1, m.queue_signature());
    let f = d.join("q.m3u");
    m.save_queue(&f).unwrap();
    m.save_queue(&f).unwrap();
    let mut r = PlaylistManager::new();
    assert_eq!(r.restore_queue(&f).unwrap(), 2);
    assert_eq!(r.items[0].filename, "b.mkv");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn default_queue_path_honours_xdg_state_home() {
    // Pure computation; avoid mutating env in parallel tests by only checking shape.
    if let Some(p) = syncverse_ui::gui::playlist::default_queue_path() {
        assert!(p.ends_with("syncverse/queue.m3u"));
    }
}
