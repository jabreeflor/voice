//! Hotkey choices, model discovery and the downloadable model catalog.
//! Behavioural reference: `enum Hotkey`, `enum Config`, `ModelCatalog` in
//! Sources/VoiceCore/core.swift.

use std::path::{Path, PathBuf};

use crate::settings::Settings;

/// The hold-to-talk key. Raw values are the Swift `UserDefaults` strings so a
/// settings file written by the Swift app keeps its meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Hotkey {
    /// macOS Right ⌥ Option; Windows/Linux Right Alt (AltGr).
    RightOption,
    /// macOS Right ⌘ Command; Windows Right Win; Linux Right Super.
    RightCommand,
    /// macOS fn/🌐. Not deliverable to apps on Windows/Linux, so not offered there.
    Fn,
    /// Right Control. Offered on Windows/Linux only.
    RightControl,
}

impl Hotkey {
    pub const ALL: [Hotkey; 4] = [
        Hotkey::RightOption,
        Hotkey::RightCommand,
        Hotkey::Fn,
        Hotkey::RightControl,
    ];

    /// Hotkeys the current platform can actually hook, in the order the UI lists them.
    pub fn available() -> Vec<Hotkey> {
        if cfg!(target_os = "macos") {
            vec![Hotkey::RightOption, Hotkey::RightCommand, Hotkey::Fn]
        } else {
            vec![
                Hotkey::RightOption,
                Hotkey::RightControl,
                Hotkey::RightCommand,
            ]
        }
    }

    pub fn raw_value(self) -> &'static str {
        match self {
            Hotkey::RightOption => "rightOption",
            Hotkey::RightCommand => "rightCommand",
            Hotkey::Fn => "fn",
            Hotkey::RightControl => "rightControl",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Hotkey> {
        Hotkey::ALL.into_iter().find(|hk| hk.raw_value() == raw)
    }

    /// macOS virtual key code (61 / 54 / 63 / 62). Kept for parity and tests.
    pub fn key_code(self) -> i64 {
        match self {
            Hotkey::RightOption => 61,
            Hotkey::RightCommand => 54,
            Hotkey::Fn => 63,
            Hotkey::RightControl => 62,
        }
    }

    pub fn label(self) -> String {
        let s = if cfg!(target_os = "macos") {
            match self {
                Hotkey::RightOption => "Right ⌥ Option",
                Hotkey::RightCommand => "Right ⌘ Command",
                Hotkey::Fn => "fn",
                Hotkey::RightControl => "Right ⌃ Control",
            }
        } else {
            match self {
                Hotkey::RightOption => "Right Alt",
                Hotkey::RightCommand if cfg!(target_os = "windows") => "Right Win",
                Hotkey::RightCommand => "Right Super",
                Hotkey::Fn => "fn",
                Hotkey::RightControl => "Right Ctrl",
            }
        };
        s.to_string()
    }

    pub fn short_label(self) -> String {
        if cfg!(target_os = "macos") {
            match self {
                Hotkey::RightOption => "Right ⌥",
                Hotkey::RightCommand => "Right ⌘",
                Hotkey::Fn => "fn",
                Hotkey::RightControl => "Right ⌃",
            }
            .to_string()
        } else {
            // The Windows/Linux names have no glyph form to shorten to.
            self.label()
        }
    }
}

/// Expands a leading `~` or `~/` to the user's home directory. A bare `~` is
/// returned as-is when no home directory can be resolved.
///
/// Deviation from Swift: `NSString.expandingTildeInPath` also resolves
/// `~alice/…` through the passwd database. This port leaves such paths
/// literal, so a hand-set `VOICE_MODEL=~alice/ggml-base.en.bin` fails the
/// existence check and falls through to the normal search. The app itself
/// only ever writes `~/…`, and resolving other users' homes would need a
/// per-platform passwd/NSS lookup that has no Windows equivalent, so this is
/// a conscious product decision rather than an oversight (pinned by
/// `expand_tilde_handles_bare_and_absent_tilde`).
pub fn expand_tilde(path: &str) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return PathBuf::from(path);
    };
    if path == "~" {
        home
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

pub struct Config;

impl Config {
    /// Production whisper-server port. Tests use 18178 so a running app is not disturbed.
    pub const SERVER_PORT: u16 = 8178;

    // Preference order tuned for dictation latency: base.en transcribes a
    // sentence in about a second even under load, which is what makes
    // hold-speak-release feel instant. Bigger models are more accurate but add
    // seconds per utterance. tiny is last: fast but noticeably less accurate.
    pub const PREFERRED_MODELS: [&'static str; 9] = [
        "ggml-base.en.bin",
        "ggml-base.bin",
        "ggml-small.en.bin",
        "ggml-small.bin",
        "ggml-medium.en.bin",
        "ggml-medium.bin",
        "ggml-large-v3-turbo.bin",
        "ggml-tiny.en.bin",
        "ggml-tiny.bin",
    ];

    /// Default `RightOption`; an unknown raw value (e.g. from a newer or older
    /// build) also falls back rather than failing to start.
    pub fn hotkey(settings: &Settings) -> Hotkey {
        settings
            .get_string("hotkey")
            .and_then(|raw| Hotkey::from_raw(&raw))
            .unwrap_or(Hotkey::RightOption)
    }

    pub fn set_hotkey(settings: &Settings, hotkey: Hotkey) {
        settings.set("hotkey", hotkey.raw_value());
    }

    pub fn sounds_enabled(settings: &Settings) -> bool {
        settings.get_bool("sounds").unwrap_or(true)
    }

    pub fn set_sounds_enabled(settings: &Settings, enabled: bool) {
        settings.set("sounds", enabled);
    }

    pub fn trailing_space(settings: &Settings) -> bool {
        settings.get_bool("trailingSpace").unwrap_or(true)
    }

    /// `~/voice/models` then `~/.voice/models`.
    pub fn models_dirs() -> Vec<PathBuf> {
        // A process with no resolvable home (no $HOME / passwd entry, which
        // no real user session has) degrades to cwd-relative `voice/models`
        // and `.voice/models` rather than an empty list: the downloader
        // writes to `models_dirs()[0]` (see download.rs) and callers rely on
        // there always being a first directory. Scanning and downloading
        // into the cwd is the least surprising non-crashing behaviour there.
        let home = dirs::home_dir().unwrap_or_default();
        vec![
            home.join("voice").join("models"),
            home.join(".voice").join("models"),
        ]
    }

    pub fn selected_model_file(settings: &Settings) -> Option<String> {
        settings.get_string("modelFile")
    }

    /// `None` removes the key, matching Swift's `UserDefaults.set(nil)`.
    pub fn set_selected_model_file(settings: &Settings, file: Option<&str>) {
        match file {
            Some(file) => settings.set("modelFile", file),
            None => settings.remove("modelFile"),
        }
    }

    /// `VOICE_MODEL` override → selected model → preferred list → any `ggml*.bin`.
    pub fn find_model(settings: &Settings) -> Option<PathBuf> {
        Config::find_model_with_env(settings, std::env::var("VOICE_MODEL").ok().as_deref())
    }

    /// Same as `find_model` with the environment override passed explicitly (testable).
    pub fn find_model_with_env(settings: &Settings, env_override: Option<&str>) -> Option<PathBuf> {
        Config::find_model_in(settings, env_override, &Config::models_dirs())
    }

    /// The whole search chain against explicit directories. `find_model_with_env`
    /// passes `models_dirs()`; the unit tests pass temp dirs so the scan is
    /// exercised without touching `$HOME` or the process environment.
    fn find_model_in(
        settings: &Settings,
        env_override: Option<&str>,
        dirs: &[PathBuf],
    ) -> Option<PathBuf> {
        // An empty override would otherwise resolve to "" — the cwd — and
        // pass the existence check.
        if let Some(env) = env_override.filter(|e| !e.is_empty()) {
            let path = expand_tilde(env);
            if path.exists() {
                return Some(path);
            }
        }
        if let Some(found) = Config::selected_model_file(settings)
            .and_then(|f| ModelCatalog::installed_path_in(&f, dirs))
        {
            return Some(found);
        }
        // Directory-major, like Swift: every preference (and then the
        // fallback) is tried in the first directory before the second one is
        // consulted at all, so a tiny model in ~/voice/models beats base.en
        // in ~/.voice/models.
        for dir in dirs {
            for name in Config::PREFERRED_MODELS {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            if let Some(any) = any_ggml_model(dir) {
                return Some(any);
            }
        }
        None
    }
}

/// First `ggml*.bin` in `dir` by name. Directory listing order is filesystem
/// dependent, so sort to keep `find_model` stable across calls.
fn any_ggml_model(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("ggml") && Path::new(n).extension().is_some_and(|x| x == "bin"))
        .collect();
    names.sort();
    names.first().map(|n| dir.join(n))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModelSpec {
    pub file: &'static str,
    pub label: &'static str,
}

pub struct ModelCatalog;

impl ModelCatalog {
    pub const ALL: [ModelSpec; 5] = [
        ModelSpec {
            file: "ggml-tiny.en.bin",
            label: "Tiny — fastest (75 MB)",
        },
        ModelSpec {
            file: "ggml-base.en.bin",
            label: "Base — fast (142 MB)",
        },
        ModelSpec {
            file: "ggml-small.en.bin",
            label: "Small — balanced (466 MB)",
        },
        ModelSpec {
            file: "ggml-medium.en.bin",
            label: "Medium — accurate (1.5 GB)",
        },
        ModelSpec {
            file: "ggml-large-v3-turbo.bin",
            label: "Large v3 Turbo — best (1.6 GB)",
        },
    ];

    /// Auto-setup default: base.en — small download, ~1 s per utterance.
    pub fn default_spec() -> &'static ModelSpec {
        // The catalog is a compile-time const, so a missing entry is a
        // programming error (Swift: fatalError), not a runtime condition.
        ModelCatalog::ALL
            .iter()
            .find(|s| s.file == "ggml-base.en.bin")
            .expect("ModelCatalog missing ggml-base.en.bin")
    }

    pub fn download_url(spec: &ModelSpec) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            spec.file
        )
    }

    pub fn installed_path(file: &str) -> Option<PathBuf> {
        ModelCatalog::installed_path_in(file, &Config::models_dirs())
    }

    fn installed_path_in(file: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
        dirs.iter().map(|dir| dir.join(file)).find(|p| p.exists())
    }
}

// The integration suite in tests/config.rs can only reach the real
// `models_dirs()`, which are empty on CI, so the directory scan is pinned
// here against temp directories.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, [0u8]).expect("write fake model");
        path
    }

    fn settings_in(dir: &TempDir) -> Settings {
        Settings::in_dir(dir.path())
    }

    // MARK: - any_ggml_model

    /// Swift's `contentsOfDirectory` order is filesystem dependent; the port
    /// sorts so `find_model` is stable across calls on every filesystem.
    #[test]
    fn any_ggml_model_picks_the_lexically_first_candidate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let zzz = touch(dir.path(), "ggml-zzz.bin");
        let aaa = touch(dir.path(), "ggml-aaa.bin");
        assert_eq!(any_ggml_model(dir.path()), Some(aaa));
        assert_ne!(any_ggml_model(dir.path()), Some(zzz));
    }

    /// Swift only checks `hasPrefix("ggml") && pathExtension == "bin"`: no dash
    /// is required after the prefix.
    #[test]
    fn any_ggml_model_accepts_the_prefix_without_a_dash() {
        let dir = tempfile::tempdir().expect("temp dir");
        let odd = touch(dir.path(), "ggmlfoo.bin");
        assert_eq!(any_ggml_model(dir.path()), Some(odd));
    }

    #[test]
    fn any_ggml_model_ignores_other_prefixes_and_extensions() {
        let dir = tempfile::tempdir().expect("temp dir");
        touch(dir.path(), "whisper-base.bin");
        touch(dir.path(), "ggml-base.en.gguf");
        touch(dir.path(), "ggml-base.en.bin.part");
        touch(dir.path(), "notes.txt");
        assert_eq!(any_ggml_model(dir.path()), None);
    }

    #[test]
    fn any_ggml_model_is_none_for_a_missing_or_empty_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(any_ggml_model(dir.path()), None);
        assert_eq!(any_ggml_model(&dir.path().join("nope")), None);
    }

    // MARK: - find_model directory scan

    #[test]
    fn find_model_walks_the_preference_list_within_a_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&dir);
        touch(dir.path(), "ggml-tiny.en.bin");
        let small = touch(dir.path(), "ggml-small.en.bin");
        touch(dir.path(), "ggml-aaa.bin");
        assert_eq!(
            Config::find_model_in(&settings, None, &[dir.path().to_path_buf()]),
            Some(small),
            "small.en outranks tiny.en and the fallback never runs when a preferred model exists"
        );
    }

    /// Directory-major precedence: the fallback in the first directory wins
    /// over the top preference in the second, exactly as in Swift.
    #[test]
    fn find_model_exhausts_the_first_directory_before_the_second() {
        let first = tempfile::tempdir().expect("temp dir");
        let second = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&first);
        let tiny = touch(first.path(), "ggml-tiny.bin");
        let base = touch(second.path(), "ggml-base.en.bin");
        let dirs = [first.path().to_path_buf(), second.path().to_path_buf()];
        assert_eq!(Config::find_model_in(&settings, None, &dirs), Some(tiny));

        fs::remove_file(dirs[0].join("ggml-tiny.bin")).expect("remove");
        let custom = touch(first.path(), "ggml-custom.bin");
        assert_eq!(
            Config::find_model_in(&settings, None, &dirs),
            Some(custom),
            "an unlisted ggml*.bin in the first dir still beats base.en in the second"
        );

        fs::remove_file(dirs[0].join("ggml-custom.bin")).expect("remove");
        assert_eq!(Config::find_model_in(&settings, None, &dirs), Some(base));
    }

    #[test]
    fn find_model_falls_back_to_sorted_any_ggml_and_skips_missing_dirs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&dir);
        touch(dir.path(), "ggml-zzz.bin");
        let aaa = touch(dir.path(), "ggml-aaa.bin");
        let dirs = [dir.path().join("missing"), dir.path().to_path_buf()];
        assert_eq!(Config::find_model_in(&settings, None, &dirs), Some(aaa));
    }

    #[test]
    fn find_model_is_none_when_no_directory_has_a_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&dir);
        touch(dir.path(), "README.md");
        let dirs = [dir.path().to_path_buf(), dir.path().join("missing")];
        assert_eq!(Config::find_model_in(&settings, None, &dirs), None);
    }

    /// The selected model wins over the preference scan when it is installed
    /// in any directory, and is ignored (not returned dangling) otherwise.
    #[test]
    fn find_model_prefers_an_installed_selected_model_over_the_scan() {
        let first = tempfile::tempdir().expect("temp dir");
        let second = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&first);
        let base = touch(first.path(), "ggml-base.en.bin");
        let tiny = touch(second.path(), "ggml-tiny.en.bin");
        let dirs = [first.path().to_path_buf(), second.path().to_path_buf()];

        Config::set_selected_model_file(&settings, Some("ggml-tiny.en.bin"));
        assert_eq!(Config::find_model_in(&settings, None, &dirs), Some(tiny));

        Config::set_selected_model_file(&settings, Some("ggml-ghost.bin"));
        assert_eq!(Config::find_model_in(&settings, None, &dirs), Some(base));
    }

    #[test]
    fn find_model_env_override_beats_a_selected_and_installed_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = settings_in(&dir);
        touch(dir.path(), "ggml-base.en.bin");
        let custom = touch(dir.path(), "weights.gguf");
        Config::set_selected_model_file(&settings, Some("ggml-base.en.bin"));
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(
            Config::find_model_in(&settings, custom.to_str(), &dirs),
            Some(custom.clone())
        );
        let dangling = dir.path().join("gone.bin");
        assert_eq!(
            Config::find_model_in(&settings, dangling.to_str(), &dirs),
            Some(dir.path().join("ggml-base.en.bin"))
        );
    }

    #[test]
    fn installed_path_in_returns_the_first_directory_that_has_the_file() {
        let first = tempfile::tempdir().expect("temp dir");
        let second = tempfile::tempdir().expect("temp dir");
        let in_second = touch(second.path(), "ggml-base.en.bin");
        let dirs = [first.path().to_path_buf(), second.path().to_path_buf()];
        assert_eq!(
            ModelCatalog::installed_path_in("ggml-base.en.bin", &dirs),
            Some(in_second)
        );
        let in_first = touch(first.path(), "ggml-base.en.bin");
        assert_eq!(
            ModelCatalog::installed_path_in("ggml-base.en.bin", &dirs),
            Some(in_first)
        );
        assert_eq!(
            ModelCatalog::installed_path_in("ggml-nope.bin", &dirs),
            None
        );
    }
}
