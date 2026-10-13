//! Dictation history and snippets. Reference: Sources/VoiceCore/store.swift.
//! Files stay byte-compatible with the Swift app (same directory, same JSON
//! shapes) so an existing history.json / snippets.json carries over.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

// MARK: - Persistence root

pub struct Store;

impl Store {
    /// `<platform data dir>/Voice`, created if missing. macOS:
    /// ~/Library/Application Support/Voice (identical to the Swift app).
    pub fn dir() -> PathBuf {
        let base = dirs::data_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Voice");
        let _ = fs::create_dir_all(&base);
        base
    }
}

/// Unix seconds at Apple's reference date, 2001-01-01T00:00:00Z.
const APPLE_EPOCH_OFFSET: f64 = 978_307_200.0;

/// Bound for a `date` read from user-editable `history.json`, chosen so that
/// `UNIX_EPOCH ± Duration::from_secs_f64(MAX_UNIX_SECS)` is representable on
/// every target: Windows `SystemTime` is a FILETIME (i64 100-ns ticks), which
/// overflows around 9.2e11 s, well before chrono's `NaiveDate` limit (~8.2e12 s,
/// about ±262_000 years). 8.0e11 s is roughly year 27_000. The conversion in
/// [`DictationEntry::system_time`] still uses checked arithmetic on top of the
/// clamp, so a platform with a tighter range degrades to `UNIX_EPOCH` instead
/// of panicking.
const MAX_UNIX_SECS: f64 = 8.0e11;

/// Atomic write: temp file next to the target, then rename, so a concurrent
/// reader (the app, voicectl, or another store on the same file) never sees
/// a half-written file. The temp name carries the pid *and* a process-wide
/// counter: two stores on the same file inside one process must not share a
/// temp path, or one's `fs::write` truncates the file the other is about to
/// rename over. Failures are swallowed; the in-memory list stays
/// authoritative for this process.
fn write_atomically(path: &Path, bytes: &[u8]) {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let Some(dir) = path.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{}-{}-{}.tmp", name, std::process::id(), n));
    // Whether the write or the rename fails, remove the temp file so a full
    // disk does not litter the data directory with one orphan per save.
    let ok = fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, path).is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
    }
}

/// (mtime, size) of a user-editable file, `None` when it does not exist.
/// Stores compare it against the value from their last load or save to
/// notice edits made by `voicectl` (or a text editor) without re-reading the
/// file every time.
type Fingerprint = Option<(Option<SystemTime>, u64)>;

fn file_fingerprint(path: &Path) -> Fingerprint {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

/// Swift's `split(separator: " ")` drops empty pieces, so runs of spaces and
/// an empty string both count as zero extra words.
fn word_count(text: &str) -> i64 {
    text.split(' ').filter(|w| !w.is_empty()).count() as i64
}

// MARK: - Dictation history

/// One dictation. `date` is serialized as seconds since 2001-01-01 (Apple's
/// reference date) because that is how Swift's `Codable` encoded `Date`, and
/// history.json written by the Swift app must still load. Use
/// [`DictationEntry::new`] / [`DictationEntry::system_time`] rather than
/// touching the raw field.
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
        let unix = match date.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs_f64(),
            Err(e) => -e.duration().as_secs_f64(),
        };
        DictationEntry {
            text: text.into(),
            date: unix - APPLE_EPOCH_OFFSET,
            duration,
            latency,
        }
    }

    /// Total for every `f64`: history.json is user-editable, and
    /// `Duration::from_secs_f64` (NaN, huge), `SystemTime` arithmetic (Windows
    /// FILETIME range) and chrono's `DateTime` (outside roughly ±262_000 years)
    /// would otherwise panic on a garbage `date`. Swift produced a harmless
    /// nonsense label for such files; we clamp, and fall back to `UNIX_EPOCH`
    /// if the clamped value still does not fit the platform's `SystemTime`.
    pub fn system_time(&self) -> SystemTime {
        let unix = self.date + APPLE_EPOCH_OFFSET;
        let unix = if unix.is_nan() {
            0.0
        } else {
            unix.clamp(-MAX_UNIX_SECS, MAX_UNIX_SECS)
        };
        let d = Duration::from_secs_f64(unix.abs());
        let t = if unix >= 0.0 {
            UNIX_EPOCH.checked_add(d)
        } else {
            UNIX_EPOCH.checked_sub(d)
        };
        t.unwrap_or(UNIX_EPOCH)
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
        let _ = fs::create_dir_all(&directory);
        let mut store = HistoryStore {
            entries: Vec::new(),
            directory,
            settings,
            stamp: 0,
        };
        store.load();
        store.migrate_legacy();
        store
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
        self.settings.get_i64("wordsTotal").unwrap_or(0)
    }

    fn set_total_words(&self, value: i64) {
        self.settings.set("wordsTotal", value);
    }

    pub fn average_wpm(&self) -> i64 {
        let timed = self.entries.iter().filter(|e| e.duration > 0.5);
        let (words, secs) = timed.fold((0i64, 0.0f64), |(w, s), e| {
            (w + word_count(&e.text), s + e.duration)
        });
        if secs <= 1.0 {
            return 0;
        }
        (words as f64 / secs * 60.0).round() as i64
    }

    pub fn average_latency(&self) -> f64 {
        let timed: Vec<f64> = self
            .entries
            .iter()
            .filter(|e| e.latency > 0.0)
            .take(50)
            .map(|e| e.latency)
            .collect();
        if timed.is_empty() {
            return 0.0;
        }
        timed.iter().sum::<f64>() / timed.len() as f64
    }

    pub fn add(&mut self, entry: DictationEntry) {
        let words = word_count(&entry.text);
        self.entries.insert(0, entry);
        self.entries.truncate(Self::LIMIT);
        // Saturate: `wordsTotal` is user-editable JSON and must never trap.
        self.set_total_words(self.total_words().saturating_add(words));
        self.stamp += 1;
        self.save();
    }

    /// Entries grouped by day, newest first, titles like "Today" / "Yesterday" /
    /// "Monday, Sep 15". Only adjacent entries merge.
    pub fn grouped(&self, max_entries: usize) -> Vec<(String, Vec<DictationEntry>)> {
        let today = Local::now().date_naive();
        let yesterday = today.pred_opt();
        let mut groups: Vec<(String, Vec<DictationEntry>)> = Vec::new();
        for e in self.entries.iter().take(max_entries) {
            let day = DateTime::<Local>::from(e.system_time()).date_naive();
            let title = if day == today {
                "Today".to_string()
            } else if Some(day) == yesterday {
                "Yesterday".to_string()
            } else {
                // Swift "EEEE, MMM d": full weekday, short month, unpadded day.
                day.format("%A, %b %-d").to_string()
            };
            match groups.last_mut() {
                Some((t, list)) if *t == title => list.push(e.clone()),
                _ => groups.push((title, vec![e.clone()])),
            }
        }
        groups
    }

    fn load(&mut self) {
        let Ok(bytes) = fs::read(self.file_path()) else {
            return;
        };
        if let Ok(list) = serde_json::from_slice::<Vec<DictationEntry>>(&bytes) {
            self.entries = list;
        }
    }

    fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec(&self.entries) {
            write_atomically(&self.file_path(), &bytes);
        }
    }

    /// Import the plain-string history from earlier builds, once.
    fn migrate_legacy(&mut self) {
        let Some(old) = self.settings.get_string_array("history") else {
            return;
        };
        if old.is_empty() {
            return;
        }
        let now = SystemTime::now();
        let mut words = self.total_words();
        for (i, text) in old.into_iter().enumerate() {
            let date = now - Duration::from_secs(60 * i as u64);
            words = words.saturating_add(word_count(&text));
            self.entries.push(DictationEntry::new(text, date, 0.0, 0.0));
        }
        self.set_total_words(words);
        self.settings.remove("history");
        self.stamp += 1;
        self.save();
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
    /// snippets.json as of the last load or save (see [`Fingerprint`]).
    fingerprint: Fingerprint,
}

impl SnippetStore {
    pub fn new(directory: PathBuf) -> SnippetStore {
        let _ = fs::create_dir_all(&directory);
        let mut store = SnippetStore {
            snippets: Vec::new(),
            directory,
            stamp: 0,
            fingerprint: None,
        };
        store.load();
        store
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
    /// `add`, `remove_trigger` and `snippet_for` all go through this so a CLI
    /// user can type `"My Email"` and still hit the `my email` snippet.
    pub fn normalize_trigger(trigger: &str) -> String {
        trigger
            .trim_matches(|c| c == '"' || c == ' ')
            .to_lowercase()
    }

    pub fn snippet_for(&self, trigger: &str) -> Option<&Snippet> {
        let t = Self::normalize_trigger(trigger);
        self.snippets.iter().find(|s| s.trigger == t)
    }

    /// Adds or replaces. Returns false (and changes nothing) when the trigger
    /// or text is empty after normalization.
    pub fn add(&mut self, trigger: &str, text: &str) -> bool {
        let t = Self::normalize_trigger(trigger);
        if t.is_empty() || text.is_empty() {
            return false;
        }
        // A stale in-memory list must not clobber another writer's snippets.
        self.reload_if_changed();
        self.snippets.retain(|s| s.trigger != t);
        self.snippets.push(Snippet {
            trigger: t,
            text: text.to_string(),
        });
        self.stamp += 1;
        self.save();
        true
    }

    pub fn remove_at(&mut self, index: usize) {
        if index >= self.snippets.len() {
            return;
        }
        self.reload_if_changed();
        if index >= self.snippets.len() {
            return;
        }
        self.snippets.remove(index);
        self.stamp += 1;
        self.save();
    }

    /// Returns false when there is no such snippet.
    pub fn remove_trigger(&mut self, trigger: &str) -> bool {
        self.reload_if_changed();
        let t = Self::normalize_trigger(trigger);
        let before = self.snippets.len();
        self.snippets.retain(|s| s.trigger != t);
        if self.snippets.len() == before {
            return false;
        }
        self.stamp += 1;
        self.save();
        true
    }

    /// Replaces every snippet (normalized, empties dropped, later duplicates win).
    pub fn replace_all(&mut self, list: &[Snippet]) {
        let mut out: Vec<Snippet> = Vec::new();
        for s in list {
            let t = Self::normalize_trigger(&s.trigger);
            if t.is_empty() || s.text.is_empty() {
                continue;
            }
            out.retain(|x| x.trigger != t);
            out.push(Snippet {
                trigger: t,
                text: s.text.clone(),
            });
        }
        self.snippets = out;
        self.stamp += 1;
        self.save();
    }

    /// Re-reads snippets.json if another process wrote it since the last
    /// load or save. Returns true when the in-memory list actually changed.
    pub fn reload_if_changed(&mut self) -> bool {
        if self.current_fingerprint() == self.fingerprint {
            return false;
        }
        let previous = std::mem::take(&mut self.snippets);
        self.load();
        if self.snippets != previous {
            self.stamp += 1;
            true
        } else {
            false
        }
    }

    /// Replace spoken triggers with their expansions. A trigger spoken as the
    /// entire utterance (ignoring case and trailing punctuation) becomes the
    /// snippet verbatim; triggers inside a sentence are swapped in place.
    pub fn expand(&mut self, transcript: &str) -> String {
        self.reload_if_changed();
        if self.snippets.is_empty() {
            return transcript.to_string();
        }
        let mut sorted: Vec<&Snippet> = self.snippets.iter().collect();
        // Longest trigger first so "my email" wins over its prefix "my".
        sorted.sort_by_key(|s| std::cmp::Reverse(s.trigger.chars().count()));
        let mut out = transcript.to_string();
        for s in sorted {
            let whole = out
                .trim()
                .trim_matches(|c| matches!(c, '.' | ',' | '!' | '?'));
            if whole.to_lowercase() == s.trigger.to_lowercase() {
                return s.text.clone();
            }
            out = replace_bounded(&out, &s.trigger, &s.text);
        }
        out
    }

    fn current_fingerprint(&self) -> Fingerprint {
        file_fingerprint(&self.file_path())
    }

    fn load(&mut self) {
        self.fingerprint = self.current_fingerprint();
        self.snippets = fs::read(self.file_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Snippet>>(&bytes).ok())
            .unwrap_or_default();
    }

    fn save(&mut self) {
        if let Ok(bytes) = serde_json::to_vec(&self.snippets) {
            write_atomically(&self.file_path(), &bytes);
        }
        // Record our own write so it is not later mistaken for an external edit.
        self.fingerprint = self.current_fingerprint();
    }
}

// MARK: - Vocabulary

/// Words and names whisper should spell the user's way (product names,
/// colleagues, jargon). Sent to whisper as its initial prompt; see
/// `engine::whisper_prompt`. Stored in `vocabulary.json` next to
/// snippets.json as a plain JSON array of strings, with the same
/// reload-on-external-edit rule so `voicectl vocab ...` reaches the running
/// app. Unlike snippet triggers the spelling is kept as typed: case is the
/// whole point ("Jabree", not "jabree").
pub struct VocabularyStore {
    words: Vec<String>,
    directory: PathBuf,
    stamp: u64,
    fingerprint: Fingerprint,
}

impl VocabularyStore {
    pub fn new(directory: PathBuf) -> VocabularyStore {
        let _ = fs::create_dir_all(&directory);
        let mut store = VocabularyStore {
            words: Vec::new(),
            directory,
            stamp: 0,
            fingerprint: None,
        };
        store.load();
        store
    }

    pub fn words(&self) -> &[String] {
        &self.words
    }

    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn file_path(&self) -> PathBuf {
        self.directory.join("vocabulary.json")
    }

    /// Surrounding quotes and whitespace dropped, inner whitespace collapsed
    /// to single spaces. Case is preserved.
    pub fn normalize_word(word: &str) -> String {
        word.trim_matches(|c: char| c == '"' || c.is_whitespace())
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The stored spelling of `word`, matched case-insensitively.
    pub fn find(&self, word: &str) -> Option<&str> {
        let key = Self::normalize_word(word).to_lowercase();
        self.words
            .iter()
            .find(|w| w.to_lowercase() == key)
            .map(String::as_str)
    }

    /// Adds `word`, or re-spells an existing case-insensitive match in place
    /// (so "jabree" → "Jabree" fixes the entry without moving it). Returns
    /// false (and changes nothing) when the word is empty after
    /// normalization.
    pub fn add(&mut self, word: &str) -> bool {
        let w = Self::normalize_word(word);
        if w.is_empty() {
            return false;
        }
        // A stale in-memory list must not clobber another writer's words.
        self.reload_if_changed();
        let key = w.to_lowercase();
        match self.words.iter_mut().find(|x| x.to_lowercase() == key) {
            Some(existing) if *existing == w => return true,
            Some(existing) => *existing = w,
            None => self.words.push(w),
        }
        self.stamp += 1;
        self.save();
        true
    }

    /// Case-insensitive. Returns false when there is no such word.
    pub fn remove(&mut self, word: &str) -> bool {
        self.reload_if_changed();
        let key = Self::normalize_word(word).to_lowercase();
        let before = self.words.len();
        self.words.retain(|w| w.to_lowercase() != key);
        if self.words.len() == before {
            return false;
        }
        self.stamp += 1;
        self.save();
        true
    }

    /// Replaces every word (normalized, empties dropped, the first spelling
    /// of a case-insensitive duplicate wins).
    pub fn replace_all(&mut self, list: &[String]) {
        let mut out: Vec<String> = Vec::new();
        for word in list {
            let w = Self::normalize_word(word);
            if w.is_empty() || out.iter().any(|x| x.to_lowercase() == w.to_lowercase()) {
                continue;
            }
            out.push(w);
        }
        self.words = out;
        self.stamp += 1;
        self.save();
    }

    /// Re-reads vocabulary.json if another process wrote it since the last
    /// load or save. Returns true when the in-memory list actually changed.
    pub fn reload_if_changed(&mut self) -> bool {
        if file_fingerprint(&self.file_path()) == self.fingerprint {
            return false;
        }
        let previous = std::mem::take(&mut self.words);
        self.load();
        if self.words != previous {
            self.stamp += 1;
            true
        } else {
            false
        }
    }

    fn load(&mut self) {
        self.fingerprint = file_fingerprint(&self.file_path());
        self.words = fs::read(self.file_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .unwrap_or_default();
    }

    fn save(&mut self) {
        if let Ok(bytes) = serde_json::to_vec(&self.words) {
            write_atomically(&self.file_path(), &bytes);
        }
        // Record our own write so it is not later mistaken for an external edit.
        self.fingerprint = file_fingerprint(&self.file_path());
    }
}

/// The spec's `\w`: Unicode alphanumeric or `_`. This only approximates ICU's
/// `\w` (what NSRegularExpression used), which also counts combining marks,
/// ZWJ/ZWNJ and connector punctuation as word characters and excludes the Nl/No
/// number categories, so decomposed accents (e + U+0301) and superscripts bound
/// a trigger differently here than in the Swift build.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Case-insensitive replacement of every `trigger` occurrence in `text` that
/// is bounded by non-word characters on both sides, i.e. the Swift
/// `(?<!\w)…(?!\w)` pattern. Lookarounds rather than `\b` so symbol-edged
/// triggers like "c++" still expand mid-sentence, and the `regex` crate has
/// no lookaround, so the neighbouring characters are checked by hand.
/// `replacement` is inserted literally ("$1" is not a capture reference).
fn replace_bounded(text: &str, trigger: &str, replacement: &str) -> String {
    let Ok(re) = regex::RegexBuilder::new(&regex::escape(trigger))
        .case_insensitive(true)
        .build()
    else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len());
    let mut copied = 0; // everything before this byte offset is already in `out`
    let mut pos = 0;
    while pos <= text.len() {
        let Some(m) = re.find_at(text, pos) else {
            break;
        };
        let before_ok = text[..m.start()]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after_ok = text[m.end()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            out.push_str(&text[copied..m.start()]);
            out.push_str(replacement);
            copied = m.end();
            pos = m.end();
            if m.start() == m.end() {
                // snippets.json is user-editable and load() does not
                // normalize, so an empty trigger can reach this; its
                // zero-width match would otherwise spin forever.
                break;
            }
        } else {
            // A rejected match may overlap a valid one starting inside it
            // (trigger "a-a" in "ba-a-a"), so advance one char, not one match.
            pos = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}
