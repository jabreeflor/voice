use chrono::{FixedOffset, TimeZone, Utc};
use voice_core::{insights::summarize, DictationEntry};

fn entry(day: u32, hour: u32, text: &str, seconds: f64) -> DictationEntry {
    let date = Utc.with_ymd_and_hms(2026, 11, day, hour, 0, 0).unwrap();
    DictationEntry::new(text, date.into(), seconds, 0.0)
}

#[test]
fn usage_fills_seven_local_dates_and_weights_pace_by_recording_time() {
    let zone = FixedOffset::west_opt(5 * 3600).unwrap();
    let now = zone.with_ymd_and_hms(2026, 11, 26, 0, 0, 0).unwrap();
    let entries = [
        entry(20, 4, "outside local week", 1.0),
        entry(20, 5, "two words", 2.0),
        entry(26, 4, "four words right here", 1.0),
        entry(26, 5, "legacy words", 0.0),
        entry(26, 6, "future words", 1.0),
        entry(26, 4, "missing time", f64::NAN),
        entry(26, 4, "   ", 2.0),
    ];
    let data = summarize(&entries, now);
    assert_eq!(data.dictations, 4);
    assert_eq!(data.words, 10);
    assert_eq!(data.words_per_minute, Some(120));
    assert_eq!(data.activity.first().unwrap().date, "2026-11-20");
    assert_eq!(data.activity.last().unwrap().date, "2026-11-26");
    let counts: Vec<_> = data.activity.iter().map(|day| day.dictations).collect();
    assert_eq!(counts, [1, 0, 0, 0, 0, 2, 1]);
    assert_eq!(data.hours[23], 3);
    assert_eq!(data.hours[0], 2);
}

#[test]
fn streaks_use_distinct_consecutive_dates_and_allow_yesterday() {
    let entries = [21, 24, 20, 19, 25, 25].map(|day| entry(day, 1, "test", 0.0));
    let now = Utc.with_ymd_and_hms(2026, 11, 26, 0, 0, 0).unwrap();
    let data = summarize(&entries, now);
    assert_eq!(data.current_streak, 2);
    assert_eq!(data.longest_streak, 3);
    let tomorrow = Utc.with_ymd_and_hms(2026, 11, 27, 0, 0, 0).unwrap();
    assert_eq!(summarize(&entries, tomorrow).current_streak, 0);
}

#[test]
fn words_recur_across_dictations_even_when_legacy_recording_times_are_missing() {
    let legacy: DictationEntry =
        serde_json::from_str(r#"{"text":"The RÉSUMÉ, résumé, release and unique","date":1}"#)
            .unwrap();
    let entries = [legacy, entry(25, 12, "the résumé RELEASE", 0.0)];
    let now = Utc.with_ymd_and_hms(2026, 11, 26, 0, 0, 0).unwrap();
    let data = summarize(&entries, now);
    assert_eq!(data.words_per_minute, None);
    let words: Vec<_> = data
        .recurring_words
        .iter()
        .map(|word| (word.word.as_str(), word.dictations))
        .collect();
    assert_eq!(words, [("release", 2), ("résumé", 2)]);
}

#[test]
fn empty_history_has_zero_activity_and_no_patterns_or_pace() {
    let data = summarize(&[], Utc::now());
    assert_eq!(data.dictations, 0);
    assert_eq!(data.words, 0);
    assert_eq!(data.words_per_minute, None);
    assert_eq!(data.activity.len(), 7);
    assert!(data.activity.iter().all(|day| day.dictations == 0));
    assert_eq!(data.current_streak, 0);
    assert_eq!(data.longest_streak, 0);
    assert!(data.recurring_words.is_empty());
    assert_eq!(data.hours, [0; 24]);
}
