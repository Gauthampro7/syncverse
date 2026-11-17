//! Acceptance tests for the userdata module.
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use syncverse_common::userdata::*;

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sv_ud_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn settings_round_trip_and_unknown_keys() {
    let d = tmpdir("settings");
    let p = d.join("s.conf");
    let mut s = Settings::default();
    s.set(keys::VOLUME, "0.5");
    s.set_bool(keys::MUTED, true);
    s.set("future_key", "keep me = yes");
    s.save(&p).unwrap();
    let mut l = Settings::load(&p);
    assert_eq!(l.get_f32(keys::VOLUME), Some(0.5));
    assert_eq!(l.get_bool(keys::MUTED), Some(true));
    l.set(keys::VOLUME, "0.7");
    l.save(&p).unwrap();
    let l2 = Settings::load(&p);
    assert_eq!(l2.get("future_key"), Some("keep me = yes"));
    assert_eq!(l2.get_f32(keys::VOLUME), Some(0.7));
    assert_eq!(l2.version(), SETTINGS_VERSION);
}

#[test]
fn settings_tolerates_bom_crlf_and_garbage() {
    let text = "\u{feff}settings_version=1\r\nvolume=0.25\r\nthis line is junk\r\n=novalue\r\n\x01bad=1\r\n# c\r\n\r\nusername = bob \r\nvolume=0.3\r\nmuted=maybe\r\n";
    let s = Settings::parse(text);
    assert_eq!(s.get_f32("volume"), Some(0.3));
    assert_eq!(s.get("username"), Some("bob"));
    assert_eq!(s.get_bool("muted"), None);
    assert_eq!(s.iter().count(), 3);
    // raw bytes of non-UTF8 garbage never panic
    let d = tmpdir("garbage");
    let p = d.join("s.conf");
    fs::write(&p, [0xff, 0xfe, 0x00, b'=', 0x80, b'\n', b'a', b'=', b'1']).unwrap();
    assert_eq!(Settings::load(&p).get("a"), Some("1"));
    assert_eq!(Settings::load(&d.join("missing")), Settings::default());
}

#[test]
fn atomic_write_crash_simulation() {
    let d = tmpdir("atomic");
    let p = d.join("sub").join("f.txt");
    atomic_write(&p, b"original").unwrap();
    // crash after staging, before rename: original intact, readers ignore .tmp
    let tmp = stage_tmp(&p, b"half-new").unwrap();
    assert!(tmp.exists());
    assert_eq!(fs::read(&p).unwrap(), b"original");
    // crash mid-write: truncated tmp garbage must not affect settings/resume load
    let sp = d.join("settings.conf");
    let mut s = Settings::default();
    s.set("a", "1");
    s.save(&sp).unwrap();
    fs::File::create(d.join("settings.conf.tmp"))
        .unwrap()
        .write_all(b"a=BROKEN")
        .unwrap();
    assert_eq!(Settings::load(&sp).get("a"), Some("1"));
    // next successful write overwrites the stale tmp and commits
    atomic_write(&p, b"new").unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"new");
    assert!(!tmp.exists());
    // commit without staged tmp reports a typed error, original untouched
    assert!(matches!(commit_tmp(&p), Err(UserDataError::Io { .. })));
    assert_eq!(fs::read(&p).unwrap(), b"new");
}

fn key(n: u64) -> FileKey {
    FileKey { size: n, head_crc: n as u32, tail_crc: 7 }
}

#[test]
fn resume_store_round_trip_and_lru() {
    let mut st = ResumeStore::new();
    for i in 0..501u64 {
        let mut e = ResumeEntry::new(key(i));
        e.pos_ms = i * 1000;
        e.aspect = "16:9".into();
        st.put(e);
    }
    assert_eq!(st.len(), RESUME_CAPACITY);
    assert!(st.get(&key(0)).is_none(), "oldest evicted at 501");
    assert!(st.get(&key(1)).is_some());
    // touching key(1) makes it most recent; next insert evicts key(2)
    let e = st.get(&key(1)).cloned().unwrap();
    st.put(e);
    st.put(ResumeEntry::new(key(9999)));
    assert!(st.get(&key(1)).is_some());
    assert!(st.get(&key(2)).is_none());
    let d = tmpdir("store");
    let p = d.join("resume.tsv");
    st.save(&p).unwrap();
    let back = ResumeStore::load(&p);
    assert_eq!(back, st);
    // CRLF + junk lines tolerated
    let text = st.to_text().replace('\n', "\r\n") + "junk\r\n1-2-3\tx\r\n";
    assert_eq!(ResumeStore::parse(&format!("\u{feff}{text}")), st);
}

#[test]
fn file_key_rename_copy_and_mutation() {
    let d = tmpdir("key");
    let a = d.join("a.bin");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i * 31 % 251) as u8).collect();
    fs::write(&a, &data).unwrap();
    let k = FileKey::from_path(&a).unwrap();
    let b = d.join("renamed.mkv");
    fs::rename(&a, &b).unwrap();
    assert_eq!(FileKey::from_path(&b).unwrap(), k);
    let c = d.join("copy.bin");
    fs::copy(&b, &c).unwrap();
    assert_eq!(FileKey::from_path(&c).unwrap(), k);
    assert_eq!(FileKey::parse(&k.to_text()), Some(k));
    let mut m = data.clone();
    m[10] ^= 1;
    fs::write(&c, &m).unwrap();
    assert_ne!(FileKey::from_path(&c).unwrap(), k, "first 64 KiB change");
    let mut m = data.clone();
    *m.last_mut().unwrap() ^= 1;
    fs::write(&c, &m).unwrap();
    assert_ne!(FileKey::from_path(&c).unwrap(), k, "last 64 KiB change");
    // small + empty files
    fs::write(&c, b"hi").unwrap();
    assert!(FileKey::from_path(&c).is_ok());
    fs::write(&c, b"").unwrap();
    assert_eq!(FileKey::from_path(&c).unwrap().size, 0);
    assert!(FileKey::from_path(&d.join("nope")).is_err());
}

#[test]
fn sparse_8gib_file_keys_under_50ms() {
    let d = tmpdir("sparse");
    let p = d.join("big.mkv");
    let f = fs::File::create(&p).unwrap();
    f.set_len(8 * 1024 * 1024 * 1024).unwrap();
    drop(f);
    let _ = FileKey::from_path(&p).unwrap(); // warm
    let t = Instant::now();
    let k = FileKey::from_path(&p).unwrap();
    let el = t.elapsed();
    assert_eq!(k.size, 8 * 1024 * 1024 * 1024);
    assert!(el.as_millis() < 50, "keying took {el:?}");
    let _ = fs::remove_file(&p);
}
