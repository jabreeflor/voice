//! On-device aggregates over the dictations still in history.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Days, TimeZone, Timelike};
use serde::Serialize;

use crate::store::{word_count, DictationEntry};

#[derive(Debug, Serialize)]
pub struct DailyActivity {
    pub date: String,
    pub dictations: usize,
}

#[derive(Debug, Serialize)]
pub struct RecurringWord {
    pub word: String,
    pub dictations: usize,
}

#[derive(Debug, Serialize)]
pub struct Insights {
    pub words: i64,
    pub dictations: usize,
    pub words_per_minute: Option<i64>,
    pub activity: Vec<DailyActivity>,
    pub current_streak: usize,
    pub longest_streak: usize,
    pub recurring_words: Vec<RecurringWord>,
    pub hours: [usize; 24],
}

/// The zone of `now` determines calendar dates and hours, including DST.
/// Usage covers today and the previous six dates. Streaks and voice patterns
/// use all retained entries through now, including legacy entries. A streak
/// ending yesterday stays current until today's date has passed.
pub fn summarize<Tz: TimeZone>(entries: &[DictationEntry], now: DateTime<Tz>) -> Insights {
    let today = now.date_naive();
    let zone = now.timezone();
    let week = today
        .checked_sub_days(Days::new(6))
        .unwrap_or(chrono::NaiveDate::MIN);
    let mut result = Insights {
        words: 0,
        dictations: 0,
        words_per_minute: None,
        activity: Vec::new(),
        current_streak: 0,
        longest_streak: 0,
        recurring_words: Vec::new(),
        hours: [0; 24],
    };
    let mut days = BTreeMap::new();
    let mut words = HashMap::new();
    let (mut timed_words, mut seconds) = (0, 0.0);
    for entry in entries.iter().filter(|entry| entry.date.is_finite()) {
        let date = DateTime::<chrono::Utc>::from(entry.system_time()).with_timezone(&zone);
        if date > now || entry.text.trim().is_empty() {
            continue;
        }
        *days.entry(date.date_naive()).or_insert(0) += 1;
        result.hours[date.hour() as usize] += 1;
        for word in distinct_words(&entry.text) {
            *words.entry(word).or_insert(0) += 1;
        }
        if date.date_naive() >= week {
            let count = word_count(&entry.text);
            result.dictations += 1;
            result.words += count;
            if entry.duration.is_finite() && entry.duration > 0.0 {
                timed_words += count;
                seconds += entry.duration;
            }
        }
    }
    if seconds.is_finite() && seconds > 0.0 {
        result.words_per_minute = Some((60.0 * timed_words as f64 / seconds).round() as i64);
    }
    result.activity = (0..7)
        .rev()
        .filter_map(|days| today.checked_sub_days(Days::new(days)))
        .map(|date| DailyActivity {
            date: date.to_string(),
            dictations: days.get(&date).copied().unwrap_or(0),
        })
        .collect();

    let mut end = None;
    let mut run = 0;
    for &day in days.keys() {
        run = if day.pred_opt() == end { run + 1 } else { 1 };
        result.longest_streak = result.longest_streak.max(run);
        end = Some(day);
    }
    if end == Some(today) || end == today.pred_opt() {
        result.current_streak = run;
    }

    result.recurring_words = words
        .into_iter()
        .filter(|&(_, dictations)| dictations >= 2)
        .map(|(word, dictations)| RecurringWord { word, dictations })
        .collect();
    result.recurring_words.sort_unstable_by(|a, b| {
        b.dictations
            .cmp(&a.dictations)
            .then_with(|| a.word.cmp(&b.word))
    });
    result.recurring_words.truncate(8);
    result
}

fn distinct_words(text: &str) -> HashSet<String> {
    const COMMON: &[&str] = &[
        "about", "after", "again", "also", "and", "are", "been", "before", "being", "but", "can",
        "could", "did", "does", "for", "from", "had", "has", "have", "how", "into", "its", "just",
        "like", "more", "most", "other", "our", "out", "said", "she", "should", "some", "than",
        "that", "the", "their", "them", "then", "there", "these", "they", "this", "those",
        "through", "too", "very", "was", "were", "what", "when", "where", "which", "who", "will",
        "with", "would", "you", "your",
    ];
    text.split(|c: char| !c.is_alphabetic() && c != '\'' && c != '’')
        .map(|word| word.trim_matches(['\'', '’']))
        .filter(|word| word.chars().count() >= 3)
        .map(|word| word.to_lowercase().replace('’', "'"))
        .filter(|word| !COMMON.contains(&word.as_str()))
        .collect()
}
