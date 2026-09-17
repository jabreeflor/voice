//! Dictation history and snippets. Reference: Sources/VoiceCore/store.swift.
//! Files stay byte-compatible with the Swift app (same directory, same JSON
//! shapes) so an existing history.json / snippets.json carries over.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

// MARK: - Persistence root

pub struct Store;

impl Store {
    /// `<platform data dir>/Voice`, created if missing. macOS:
    /// ~/Library/Application Support/Voice (identical to the Swift app).
    pub fn dir() -> PathBuf {
        todo!()
    }
}

// MARK: - Dictation history

/// One dictation. `date` is serialized as seconds since 2001-01-01 (Apple's
/// reference date) because that is how Swift's `Codable` encoded `Date`, and
/// history.json written by the Swift app must still load.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DictationEntry {
    pub text: String,
    pub date: f64,
    /// seconds of speech
    pub duration: f64,
    /// seconds from key-release to paste
    pub latency: f64,
}

impl DictationEntry {
    pub fn new(text: impl Into<String>, date: SystemTime, duration: f64, latency: f64) -> Self {
        let _ = (text.into(), date, duration, latency);
        todo!()
    }

    pub fn system_time(&self) -> SystemTime {
        todo!()
    }
}

pub struct HistoryStore {
    entries: Vec<DictationEntry>,
    directory: PathBuf,
    settings: Arc<Settings>,
    stamp: u64,
}

impl HistoryStore {
    pub const LIMIT: usize = 300;

    pub fn new(directory: PathBuf, settings: Arc<Settings>) -> HistoryStore {
        let _ = (directory, settings);
        todo!()
    }

    pub fn entries(&self) -> &[DictationEntry] {
        &self.entries
    }

    /// Bumped on every mutation so views know when to rebuild.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn file_path(&self) -> PathBuf {
        self.directory.join("history.json")
    }

    /// Lifetime word counter kept in settings (`wordsTotal`); trimming old
    /// entries never rolls it back.
    pub fn total_words(&self) -> i64 {
        todo!()
    }

    pub fn average_wpm(&self) -> i64 {
        todo!()
    }

    pub fn average_latency(&self) -> f64 {
        todo!()
    }

    pub fn add(&mut self, entry: DictationEntry) {
        let _ = entry;
        todo!()
    }

    /// Entries grouped by day, newest first, titles like "Today" / "Yesterday" /
    /// "Monday, Sep 15". Only adjacent entries merge.
    pub fn grouped(&self, max_entries: usize) -> Vec<(String, Vec<DictationEntry>)> {
        let _ = max_entries;
        todo!()
    }
}

// MARK: - Snippets

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
}

pub struct SnippetStore {
    snippets: Vec<Snippet>,
    directory: PathBuf,
    stamp: u64,
    /// (mtime, size) of snippets.json as of the last load or save.
    fingerprint: Option<(Option<SystemTime>, u64)>,
}

impl SnippetStore {
    pub fn new(directory: PathBuf) -> SnippetStore {
        let _ = directory;
        todo!()
    }

    pub fn snippets(&self) -> &[Snippet] {
        &self.snippets
    }

    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn file_path(&self) -> PathBuf {
        self.directory.join("snippets.json")
    }

    /// The trigger as stored: surrounding quotes/spaces dropped, lowercased.
    pub fn normalize_trigger(trigger: &str) -> String {
        let _ = trigger;
        todo!()
    }

    pub fn snippet_for(&self, trigger: &str) -> Option<&Snippet> {
        let _ = trigger;
        todo!()
    }

    /// Adds or replaces. Returns false (and changes nothing) when the trigger
    /// or text is empty after normalization.
    pub fn add(&mut self, trigger: &str, text: &str) -> bool {
        let _ = (trigger, text);
        todo!()
    }

    pub fn remove_at(&mut self, index: usize) {
        let _ = index;
        todo!()
    }

    /// Returns false when there is no such snippet.
    pub fn remove_trigger(&mut self, trigger: &str) -> bool {
        let _ = trigger;
        todo!()
    }

    /// Replaces every snippet (normalized, empties dropped, later duplicates win).
    pub fn replace_all(&mut self, list: &[Snippet]) {
        let _ = list;
        todo!()
    }

    /// Re-reads snippets.json if another process wrote it since the last
    /// load or save. Returns true when the in-memory list actually changed.
    pub fn reload_if_changed(&mut self) -> bool {
        todo!()
    }

    /// Replace spoken triggers with their expansions (see SPEC / store.swift).
    pub fn expand(&mut self, transcript: &str) -> String {
        let _ = transcript;
        todo!()
    }
}

#[allow(dead_code)]
fn _path_marker(_: &Path) {}
