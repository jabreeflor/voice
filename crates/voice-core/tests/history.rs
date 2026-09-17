//! Port of Tests/VoiceCoreTests/HistoryTests.swift.
//!
//! Every test here runs against a throwaway directory and a throwaway
//! `Settings` file inside it. Nothing touches `Store::dir()` or the user's real
//! settings, both of which hold the user's real dictation history.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Days, Local};
use tempfile::TempDir;
use voice_core::{DictationEntry, HistoryStore, Settings};

struct Fixture {
    dir: TempDir,
    settings: Arc<Settings>,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = Arc::new(Settings::in_dir(dir.path()));
        Fixture { dir, settings }
    }

    fn make_store(&self) -> HistoryStore {
        HistoryStore::new(self.dir.path().to_path_buf(), Arc::clone(&self.settings))
    }

    fn history_file(&self) -> std::path::PathBuf {
        self.dir.path().join("history.json")
    }
}

fn entry(text: &str) -> DictationEntry {
    DictationEntry::new(text, SystemTime::now(), 0.0, 0.0)
}

fn timed(text: &str, duration: f64) -> DictationEntry {
    DictationEntry::new(text, SystemTime::now(), duration, 0.0)
}

fn lagged(text: &str, latency: f64) -> DictationEntry {
    DictationEntry::new(text, SystemTime::now(), 0.0, latency)
}

fn dated(text: &str, date: SystemTime) -> DictationEntry {
    DictationEntry::new(text, date, 0.0, 0.0)
}

/// Calendar days, not 24-hour blocks, so the label test is stable across DST.
fn days_ago(n: u64) -> SystemTime {
    Local::now()
        .checked_sub_days(Days::new(n))
        .expect("date in range")
        .into()
}

fn texts(entries: &[DictationEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.text.as_str()).collect()
}

fn count_entries(groups: &[(String, Vec<DictationEntry>)]) -> usize {
    groups.iter().map(|g| g.1.len()).sum()
}

fn seconds_between(a: &DictationEntry, b: &DictationEntry) -> f64 {
    a.date - b.date
}

// MARK: - add

#[test]
fn new_entries_go_to_the_front() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(entry("first"));
    store.add(entry("second"));
    store.add(entry("third"));
    assert_eq!(texts(store.entries()), ["third", "second", "first"]);
}

#[test]
fn stamp_advances_on_each_add() {
    let f = Fixture::new();
    let mut store = f.make_store();
    let start = store.stamp();
    store.add(entry("one"));
    store.add(entry("two"));
    assert_eq!(store.stamp(), start + 2);
}

#[test]
fn history_is_capped_at_three_hundred_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for i in 0..305 {
        store.add(entry(&format!("entry {i}")));
    }
    assert_eq!(store.entries().len(), 300);
    assert_eq!(
        store.entries().first().map(|e| e.text.as_str()),
        Some("entry 304"),
        "newest entry should survive"
    );
    assert_eq!(
        store.entries().last().map(|e| e.text.as_str()),
        Some("entry 5"),
        "oldest entries are dropped"
    );
}

// MARK: - total_words

#[test]
fn total_words_accumulates_across_adds() {
    let f = Fixture::new();
    let mut store = f.make_store();
    assert_eq!(store.total_words(), 0);
    store.add(entry("one two three"));
    assert_eq!(store.total_words(), 3);
    store.add(entry("four five"));
    assert_eq!(store.total_words(), 5);
}

#[test]
fn total_words_ignores_repeated_spaces_and_empty_text() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(entry("one   two"));
    assert_eq!(store.total_words(), 2);
    store.add(entry(""));
    assert_eq!(store.total_words(), 2);
}

/// total_words is a lifetime counter kept in settings, so trimming the entry
/// list at 300 must not roll it back.
#[test]
fn total_words_is_not_reduced_when_old_entries_are_trimmed() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for _ in 0..305 {
        store.add(entry("two words"));
    }
    assert_eq!(store.entries().len(), 300);
    assert_eq!(store.total_words(), 610);
}

#[test]
fn total_words_survives_a_reload() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(entry("one two three"));
    assert_eq!(f.make_store().total_words(), 3);
}

#[test]
fn total_words_is_read_from_the_injected_suite_only() {
    let f = Fixture::new();
    f.settings.set("wordsTotal", 42);
    assert_eq!(f.make_store().total_words(), 42);
}

// MARK: - Persistence

#[test]
fn entries_round_trip_through_json() {
    // 700_000_000 s after the Apple reference date, like the Swift test.
    let fixed = UNIX_EPOCH + Duration::from_secs(978_307_200 + 700_000_000);
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(DictationEntry::new("hello world", fixed, 1.25, 0.4));
    store.add(DictationEntry::new(
        "second one",
        fixed + Duration::from_secs(60),
        2.5,
        0.2,
    ));

    let reloaded = f.make_store();
    assert_eq!(reloaded.entries(), store.entries());
    let first = &reloaded.entries()[0];
    assert_eq!(first.text, "second one");
    assert_eq!(first.duration, 2.5);
    assert_eq!(first.latency, 0.2);
    assert_eq!(first.system_time(), fixed + Duration::from_secs(60));
}

/// The Swift app encoded `Date` as seconds since 2001-01-01, so a file it
/// wrote must decode to the same instant here. 700_000_000 s after the Apple
/// reference date is 2023-03-08T20:26:40Z, i.e. unix 1_678_307_200.
#[test]
fn date_field_is_seconds_since_apple_reference_date() {
    let f = Fixture::new();
    fs::write(
        f.history_file(),
        r#"[{"text":"from swift","date":700000000,"duration":1.5,"latency":0.25}]"#,
    )
    .expect("write history");
    let store = f.make_store();
    let e = &store.entries()[0];
    assert_eq!(e.text, "from swift");
    assert_eq!(e.date, 700_000_000.0);
    assert_eq!(
        e.system_time(),
        UNIX_EPOCH + Duration::from_secs(1_678_307_200)
    );

    // And the reverse: constructing from a SystemTime stores Apple seconds.
    let built = DictationEntry::new("x", UNIX_EPOCH + Duration::from_secs(978_307_200), 0.0, 0.0);
    assert_eq!(built.date, 0.0, "the reference date itself is zero");
    let json = serde_json::to_value(&built).expect("serialize");
    assert_eq!(json["date"], serde_json::json!(0.0));
}

#[test]
fn history_file_is_written_into_the_injected_directory() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(entry("hello"));
    let file = f.history_file();
    assert!(file.exists());
    let decoded: Vec<DictationEntry> =
        serde_json::from_slice(&fs::read(&file).expect("read")).expect("decode");
    assert_eq!(texts(&decoded), ["hello"]);
}

#[test]
fn fresh_directory_starts_empty() {
    let f = Fixture::new();
    assert!(f.make_store().entries().is_empty());
}

#[test]
fn corrupt_history_file_leaves_store_empty_rather_than_crashing() {
    let f = Fixture::new();
    fs::write(f.history_file(), "{{ not json").expect("write");
    let mut store = f.make_store();
    assert!(store.entries().is_empty());
    store.add(entry("recovered"));
    assert_eq!(texts(f.make_store().entries()), ["recovered"]);
}

// MARK: - grouped()

#[test]
fn grouped_on_empty_history_returns_no_groups() {
    let f = Fixture::new();
    assert!(f.make_store().grouped(40).is_empty());
}

#[test]
fn grouped_labels_today_yesterday_and_older_days() {
    let f = Fixture::new();
    let mut store = f.make_store();
    let old = days_ago(5);
    store.add(dated("old one", old));
    store.add(dated("yesterday one", days_ago(1)));
    store.add(dated("today one", SystemTime::now()));

    let groups = store.grouped(40);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].0, "Today");
    assert_eq!(groups[1].0, "Yesterday");

    // Same "EEEE, MMM d" rendering the store uses, e.g. "Monday, Sep 15".
    let expected = DateTime::<Local>::from(old)
        .format("%A, %b %-d")
        .to_string();
    assert_eq!(groups[2].0, expected);
    assert_eq!(texts(&groups[2].1), ["old one"]);
}

#[test]
fn consecutive_entries_from_the_same_day_share_one_group() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(dated("a", SystemTime::now()));
    store.add(dated("b", SystemTime::now()));
    store.add(dated("c", SystemTime::now()));

    let groups = store.grouped(40);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, "Today");
    assert_eq!(texts(&groups[0].1), ["c", "b", "a"]);
}

/// Grouping only merges *adjacent* entries, so a day that reappears later in
/// the list opens a second group with the same title. Entries are normally
/// added in chronological order, so this only shows up with back-dated data.
#[test]
fn same_day_reappearing_later_opens_a_second_group() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(dated("early today", SystemTime::now()));
    store.add(dated("yesterday", days_ago(1)));
    store.add(dated("late today", SystemTime::now()));

    let titles: Vec<String> = store.grouped(40).into_iter().map(|g| g.0).collect();
    assert_eq!(titles, ["Today", "Yesterday", "Today"]);
}

#[test]
fn grouped_honors_the_max_entry_limit() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for i in 0..10 {
        store.add(dated(&format!("entry {i}"), SystemTime::now()));
    }
    let groups = store.grouped(4);
    assert_eq!(count_entries(&groups), 4);
    assert_eq!(
        texts(&groups[0].1),
        ["entry 9", "entry 8", "entry 7", "entry 6"]
    );
}

/// Rust has no default arguments; the app passes 40 explicitly, matching the
/// Swift `max: Int = 40` default.
#[test]
fn grouped_defaults_to_forty_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for i in 0..60 {
        store.add(dated(&format!("entry {i}"), SystemTime::now()));
    }
    assert_eq!(count_entries(&store.grouped(40)), 40);
}

// MARK: - average_wpm

#[test]
fn average_wpm_over_timed_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    // 10 words in 6 seconds = 100 wpm.
    store.add(timed(
        "one two three four five six seven eight nine ten",
        6.0,
    ));
    assert_eq!(store.average_wpm(), 100);
}

#[test]
fn average_wpm_pools_words_and_seconds_across_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(timed("one two three four five", 3.0));
    store.add(timed("six seven eight nine ten", 3.0));
    assert_eq!(store.average_wpm(), 100);
}

/// Entries shorter than half a second are keystroke-length noise; letting
/// them in would inflate the rate wildly.
#[test]
fn average_wpm_ignores_entries_under_half_a_second() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(timed(
        "one two three four five six seven eight nine ten",
        6.0,
    ));
    store.add(timed(&"word ".repeat(200), 0.3));
    assert_eq!(
        store.average_wpm(),
        100,
        "sub-0.5s entry should not affect the rate"
    );
}

#[test]
fn average_wpm_excludes_entries_at_exactly_half_a_second() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(timed(
        "one two three four five six seven eight nine ten",
        6.0,
    ));
    store.add(timed("a b c d e f g h i j k l", 0.5));
    assert_eq!(
        store.average_wpm(),
        100,
        "the threshold is strictly greater than 0.5"
    );
}

#[test]
fn average_wpm_is_zero_without_at_least_a_second_of_speech() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(timed("one two three", 0.6));
    assert_eq!(store.average_wpm(), 0);
}

#[test]
fn average_wpm_is_zero_when_no_entry_is_timed() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(timed("untimed legacy entry", 0.0));
    assert_eq!(store.average_wpm(), 0);
}

#[test]
fn average_wpm_is_zero_on_empty_history() {
    let f = Fixture::new();
    assert_eq!(f.make_store().average_wpm(), 0);
}

#[test]
fn average_wpm_rounds_to_nearest_whole_number() {
    let f = Fixture::new();
    let mut store = f.make_store();
    // 10 words in 7 seconds = 85.71… wpm.
    store.add(timed(
        "one two three four five six seven eight nine ten",
        7.0,
    ));
    assert_eq!(store.average_wpm(), 86);
}

// MARK: - average_latency

#[test]
fn average_latency_over_recent_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(lagged("a", 0.1));
    store.add(lagged("b", 0.2));
    store.add(lagged("c", 0.3));
    assert!((store.average_latency() - 0.2).abs() < 0.0001);
}

#[test]
fn average_latency_ignores_entries_without_a_timing() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(lagged("a", 0.2));
    store.add(lagged("b", 0.0));
    store.add(lagged("c", 0.4));
    assert!((store.average_latency() - 0.3).abs() < 0.0001);
}

#[test]
fn average_latency_is_zero_when_nothing_is_timed() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(lagged("a", 0.0));
    assert_eq!(store.average_latency(), 0.0);
    assert_eq!(f.make_store().average_latency(), 0.0);
}

#[test]
fn average_latency_only_considers_the_fifty_newest_entries() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for _ in 0..10 {
        store.add(lagged("old", 10.0));
    }
    for _ in 0..50 {
        store.add(lagged("recent", 0.5));
    }
    assert_eq!(store.entries().len(), 60);
    assert!(
        (store.average_latency() - 0.5).abs() < 0.0001,
        "the 10 slow older entries sit outside the 50-entry window"
    );
}

/// Untimed entries must not shrink the sample — the window is the last 50
/// *timed* entries, so older timed ones are pulled in past untimed noise.
#[test]
fn untimed_recent_entries_do_not_crowd_out_timed_ones() {
    let f = Fixture::new();
    let mut store = f.make_store();
    for _ in 0..5 {
        store.add(lagged("timed", 0.4));
    }
    for _ in 0..50 {
        store.add(lagged("untimed", 0.0));
    }
    assert!((store.average_latency() - 0.4).abs() < 0.0001);
}

// MARK: - Legacy migration

fn set_legacy(f: &Fixture, items: &[&str]) {
    let values: Vec<serde_json::Value> = items
        .iter()
        .map(|s| serde_json::Value::String(s.to_string()))
        .collect();
    f.settings.set("history", serde_json::Value::Array(values));
}

#[test]
fn legacy_string_history_is_imported_on_first_load() {
    let f = Fixture::new();
    set_legacy(&f, &["hello world", "second entry"]);
    let store = f.make_store();
    assert_eq!(texts(store.entries()), ["hello world", "second entry"]);
    assert_eq!(store.total_words(), 4);
}

#[test]
fn migrated_entries_have_no_timings() {
    let f = Fixture::new();
    set_legacy(&f, &["hello world"]);
    let store = f.make_store();
    let migrated = &store.entries()[0];
    assert_eq!(migrated.duration, 0.0);
    assert_eq!(migrated.latency, 0.0);
}

#[test]
fn migrated_entries_are_back_dated_a_minute_apart() {
    let f = Fixture::new();
    set_legacy(&f, &["newest", "middle", "oldest"]);
    let store = f.make_store();
    let entries = store.entries();
    assert_eq!(entries.len(), 3);
    assert!((seconds_between(&entries[0], &entries[1]) - 60.0).abs() < 1.0);
    assert!((seconds_between(&entries[1], &entries[2]) - 60.0).abs() < 1.0);
}

#[test]
fn legacy_key_is_cleared_so_migration_runs_only_once() {
    let f = Fixture::new();
    set_legacy(&f, &["hello world"]);
    let _ = f.make_store();
    assert!(f.settings.get_string_array("history").is_none());
    assert_eq!(
        f.make_store().entries().len(),
        1,
        "a second load must not re-import"
    );
    assert_eq!(
        f.make_store().total_words(),
        2,
        "word count must not be double-charged"
    );
}

#[test]
fn migration_is_persisted_to_disk() {
    let f = Fixture::new();
    set_legacy(&f, &["hello world"]);
    let _ = f.make_store();
    let decoded: Vec<DictationEntry> =
        serde_json::from_slice(&fs::read(f.history_file()).expect("read")).expect("decode");
    assert_eq!(texts(&decoded), ["hello world"]);
}

#[test]
fn legacy_entries_are_appended_after_existing_history() {
    let f = Fixture::new();
    let mut store = f.make_store();
    store.add(entry("modern entry"));
    set_legacy(&f, &["legacy entry"]);
    assert_eq!(
        texts(f.make_store().entries()),
        ["modern entry", "legacy entry"]
    );
}

#[test]
fn empty_legacy_history_is_a_no_op() {
    let f = Fixture::new();
    set_legacy(&f, &[]);
    let store = f.make_store();
    assert!(store.entries().is_empty());
    assert_eq!(store.total_words(), 0);
}

#[test]
fn missing_legacy_history_is_a_no_op() {
    let f = Fixture::new();
    assert!(f.make_store().entries().is_empty());
}
