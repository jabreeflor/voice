//! `Settings` replaces the handful of `UserDefaults` keys the Swift app kept.
//! Everything here uses a throwaway temp dir: the real settings file belongs to
//! whoever is running the app, never to a test.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};
use voice_core::Settings;

fn fresh() -> (TempDir, Settings) {
    let dir = tempdir().expect("temp dir");
    let settings = Settings::in_dir(dir.path());
    (dir, settings)
}

/// Directory listing minus `settings.json`, so a leftover temp file shows up.
fn stray_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name != "settings.json")
        .collect()
}

// Loading

#[test]
fn in_dir_points_at_settings_json() {
    let (dir, s) = fresh();
    assert_eq!(s.path(), dir.path().join("settings.json"));
}

/// First launch: no file yet. Every getter answers `None` so callers fall
/// back to their documented defaults instead of failing.
#[test]
fn missing_file_loads_as_empty() {
    let (dir, s) = fresh();
    assert!(!dir.path().join("settings.json").exists());
    assert_eq!(s.get_string("hotkey"), None);
    assert_eq!(s.get_bool("sounds"), None);
    assert_eq!(s.get_f64("lastAXRelaunch"), None);
    assert_eq!(s.get_i64("wordsTotal"), None);
    assert_eq!(s.get_string_array("history"), None);
}

/// A half-written or hand-edited file must not take the app down; the user
/// loses their preferences, not their dictation.
#[test]
fn corrupt_file_loads_as_empty() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("settings.json");
    fs::write(&path, b"{ this is not json").expect("write");
    let s = Settings::open(path);
    assert_eq!(s.get_string("hotkey"), None);
    assert_eq!(s.get_bool("onboarded"), None);
}

/// Valid JSON that is not an object has no keys to look up, so it counts as
/// empty rather than being mistaken for settings.
#[test]
fn non_object_json_loads_as_empty() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("settings.json");
    fs::write(&path, b"[1, 2, 3]").expect("write");
    let s = Settings::open(path);
    assert_eq!(s.get_i64("wordsTotal"), None);
}

// Getters

#[test]
fn get_string_returns_only_strings() {
    let (_dir, s) = fresh();
    s.set("hotkey", "rightCommand");
    s.set("wordsTotal", 12);
    assert_eq!(s.get_string("hotkey").as_deref(), Some("rightCommand"));
    assert_eq!(s.get_string("wordsTotal"), None);
    assert_eq!(s.get_string("absent"), None);
}

#[test]
fn get_bool_returns_only_bools() {
    let (_dir, s) = fresh();
    s.set("sounds", false);
    s.set("onboarded", true);
    s.set("hotkey", "fn");
    assert_eq!(s.get_bool("sounds"), Some(false));
    assert_eq!(s.get_bool("onboarded"), Some(true));
    // A string is never coerced to a bool, unlike UserDefaults' lenient reads.
    assert_eq!(s.get_bool("hotkey"), None);
}

#[test]
fn get_f64_reads_floats_and_integers() {
    let (_dir, s) = fresh();
    s.set("lastAXRelaunch", 1_700_000_000.5);
    s.set("wordsTotal", 42);
    assert_eq!(s.get_f64("lastAXRelaunch"), Some(1_700_000_000.5));
    assert_eq!(s.get_f64("wordsTotal"), Some(42.0));
    assert_eq!(s.get_f64("absent"), None);
}

#[test]
fn get_i64_reads_integers() {
    let (_dir, s) = fresh();
    s.set("wordsTotal", 1234);
    s.set("negative", -7);
    assert_eq!(s.get_i64("wordsTotal"), Some(1234));
    assert_eq!(s.get_i64("negative"), Some(-7));
    assert_eq!(s.get_i64("absent"), None);
}

/// `wordsTotal` was an integer in UserDefaults, but another tool editing the
/// JSON by hand may well write `1234.0`. A whole float still counts.
#[test]
fn get_i64_accepts_whole_floats() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("settings.json");
    fs::write(&path, br#"{"wordsTotal": 1234.0, "fraction": 2.5}"#).expect("write");
    let s = Settings::open(path);
    assert_eq!(s.get_i64("wordsTotal"), Some(1234));
    // A fractional value is truncated toward zero, matching `as i64`.
    assert_eq!(s.get_i64("fraction"), Some(2));
}

#[test]
fn get_string_array_reads_string_items() {
    let (_dir, s) = fresh();
    s.set("history", vec!["first", "second"]);
    assert_eq!(
        s.get_string_array("history"),
        Some(vec!["first".to_string(), "second".to_string()])
    );
    assert_eq!(s.get_string_array("absent"), None);
}

/// The legacy `history` array only ever held strings; anything else mixed in
/// is skipped rather than aborting the migration.
#[test]
fn get_string_array_skips_non_string_items() {
    let (_dir, s) = fresh();
    s.set("history", json!(["keep", 3, null, "also keep"]));
    assert_eq!(
        s.get_string_array("history"),
        Some(vec!["keep".to_string(), "also keep".to_string()])
    );
    s.set("hotkey", "fn");
    assert_eq!(s.get_string_array("hotkey"), None);
}

// Persistence

#[test]
fn set_persists_and_reloads() {
    let (dir, s) = fresh();
    s.set("hotkey", "rightCommand");
    s.set("sounds", false);
    s.set("lastAXRelaunch", 1.5);
    s.set("wordsTotal", 99);
    s.set("history", vec!["a"]);
    drop(s);

    let again = Settings::in_dir(dir.path());
    assert_eq!(again.get_string("hotkey").as_deref(), Some("rightCommand"));
    assert_eq!(again.get_bool("sounds"), Some(false));
    assert_eq!(again.get_f64("lastAXRelaunch"), Some(1.5));
    assert_eq!(again.get_i64("wordsTotal"), Some(99));
    assert_eq!(
        again.get_string_array("history"),
        Some(vec!["a".to_string()])
    );
}

#[test]
fn set_overwrites_existing_value() {
    let (dir, s) = fresh();
    s.set("wordsTotal", 1);
    s.set("wordsTotal", 2);
    assert_eq!(s.get_i64("wordsTotal"), Some(2));
    assert_eq!(Settings::in_dir(dir.path()).get_i64("wordsTotal"), Some(2));
}

/// The history migration removes the legacy key once it has been imported;
/// that removal has to survive a relaunch or the import would repeat.
#[test]
fn remove_persists() {
    let (dir, s) = fresh();
    s.set("history", vec!["legacy"]);
    s.set("wordsTotal", 5);
    s.remove("history");
    assert_eq!(s.get_string_array("history"), None);

    let again = Settings::in_dir(dir.path());
    assert_eq!(again.get_string_array("history"), None);
    assert_eq!(again.get_i64("wordsTotal"), Some(5), "other keys untouched");
}

#[test]
fn remove_of_absent_key_is_a_no_op() {
    let (dir, s) = fresh();
    s.remove("never-set");
    assert!(!dir.path().join("settings.json").exists());
    assert_eq!(s.get_string("never-set"), None);
}

/// The data dir may not exist yet on a fresh install; the first write must
/// create it rather than silently dropping the setting.
#[test]
fn set_creates_missing_parent_directory() {
    let dir = tempdir().expect("temp dir");
    let nested = dir.path().join("deeper").join("still");
    let s = Settings::in_dir(&nested);
    s.set("onboarded", true);
    assert_eq!(Settings::in_dir(&nested).get_bool("onboarded"), Some(true));
}

/// Writes go to a temp file that is renamed over `settings.json`. After the
/// rename nothing else may be left in the directory, or every launch would
/// accumulate junk next to the settings. (Whether a concurrent reader sees a
/// torn file is checked separately in `concurrent_writers_never_tear_the_file`.)
#[test]
fn atomic_write_leaves_no_temp_file_behind() {
    let (dir, s) = fresh();
    for i in 0..5 {
        s.set("wordsTotal", i);
    }
    s.remove("wordsTotal");
    assert!(dir.path().join("settings.json").exists());
    assert_eq!(stray_files(dir.path()), Vec::<String>::new());
}

#[test]
fn file_on_disk_is_a_json_object_with_the_swift_key_names() {
    let (dir, s) = fresh();
    s.set("trailingSpace", true);
    s.set("modelFile", "ggml-base.en.bin");
    let raw = fs::read(dir.path().join("settings.json")).expect("read");
    let v: Value = serde_json::from_slice(&raw).expect("valid json");
    assert_eq!(v["trailingSpace"], json!(true));
    assert_eq!(v["modelFile"], json!("ggml-base.en.bin"));
}

// Thread safety

/// The spec requires `Settings` to be `Send + Sync` so one handle can sit
/// behind an `Arc` shared by the hotkey thread, the engine and the UI.
#[test]
fn settings_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Settings>();
}

/// Several threads hammering one shared handle while another thread keeps
/// reading the file from disk: every read must parse as JSON (the temp file +
/// rename must never expose a half-written `settings.json`), and every key
/// written must survive a reload once the writers are done.
#[test]
fn concurrent_writers_never_tear_the_file() {
    const WRITERS: usize = 4;
    const ROUNDS: i64 = 50;

    let (dir, s) = fresh();
    let s = Arc::new(s);
    let path = dir.path().join("settings.json");
    let done = Arc::new(AtomicBool::new(false));

    let reader = {
        let path = path.clone();
        let done = Arc::clone(&done);
        thread::spawn(move || {
            let mut reads = 0usize;
            while !done.load(Ordering::Acquire) {
                // The file may not exist before the first rename lands; only
                // a file that exists but does not parse counts as torn.
                if let Ok(bytes) = fs::read(&path) {
                    serde_json::from_slice::<Value>(&bytes)
                        .unwrap_or_else(|e| panic!("torn settings.json: {e}"));
                    reads += 1;
                }
            }
            reads
        })
    };

    let writers: Vec<_> = (0..WRITERS)
        .map(|w| {
            let s = Arc::clone(&s);
            thread::spawn(move || {
                for i in 0..ROUNDS {
                    s.set(&format!("writer{w}"), i);
                }
            })
        })
        .collect();
    for w in writers {
        w.join().expect("writer thread");
    }
    done.store(true, Ordering::Release);
    let reads = reader.join().expect("reader thread");
    assert!(reads > 0, "reader never observed the file");

    let again = Settings::in_dir(dir.path());
    for w in 0..WRITERS {
        assert_eq!(
            again.get_i64(&format!("writer{w}")),
            Some(ROUNDS - 1),
            "writer{w}'s last value must survive a reload"
        );
    }
    assert_eq!(stray_files(dir.path()), Vec::<String>::new());
}
