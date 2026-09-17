//! JSON-backed key/value settings, the cross-platform replacement for the
//! `UserDefaults` keys the Swift app used (`hotkey`, `sounds`, `trailingSpace`,
//! `modelFile`, `onboarded`, `lastAXRelaunch`, `wordsTotal`, legacy `history`).
//!
//! One file: `<data dir>/settings.json`. Every mutation is written atomically
//! (temp file + rename) so a concurrent reader never sees a torn file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value};

/// Thread-safe settings handle. Cheap to share behind an `Arc`.
pub struct Settings {
    path: PathBuf,
    values: Mutex<Map<String, Value>>,
}

impl Settings {
    /// Loads `path` (missing or corrupt → empty settings).
    pub fn open(path: PathBuf) -> Settings {
        let values = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|v| match v {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        Settings {
            path,
            values: Mutex::new(values),
        }
    }

    /// `dir/settings.json`.
    pub fn in_dir(dir: &Path) -> Settings {
        Settings::open(dir.join("settings.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.lock().get(key).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Map<String, Value>> {
        // A poisoned lock only means another thread panicked mid-write; the
        // map itself is still consistent, so keep going rather than cascade.
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key)?.as_f64()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        let v = self.get(key)?;
        // A whole-number float (e.g. written by another tool) still counts.
        v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
    }

    pub fn get_string_array(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key)? {
            Value::Array(items) => Some(
                items
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::String(s) => Some(s),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// Sets `key` and persists immediately.
    pub fn set(&self, key: &str, value: impl Into<Value>) {
        let mut map = self.lock();
        map.insert(key.to_string(), value.into());
        self.save(&map);
    }

    /// Removes `key` (no-op if absent) and persists immediately.
    pub fn remove(&self, key: &str) {
        let mut map = self.lock();
        if map.remove(key).is_some() {
            self.save(&map);
        }
    }

    /// Atomic write: temp file in the same directory, then rename over the
    /// target. Failures are swallowed — settings are a convenience, and the
    /// in-memory copy stays authoritative for this process.
    fn save(&self, map: &Map<String, Value>) {
        let Some(dir) = self.path.parent() else {
            return;
        };
        let _ = fs::create_dir_all(dir);
        let Ok(bytes) = serde_json::to_vec_pretty(&Value::Object(map.clone())) else {
            return;
        };
        let tmp = dir.join(format!(".settings-{}.tmp", std::process::id()));
        if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, &self.path).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}
