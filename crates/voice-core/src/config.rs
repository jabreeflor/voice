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

/// Expands a leading `~` or `~/` to the user's home directory. Any other path
/// (including `~user` forms, which the app never writes) is returned as-is,
/// as is a bare `~` when no home directory can be resolved.
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
        // Without a resolvable home there is nowhere to look; relative paths
        // would silently scan the process cwd instead.
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
        // An empty override would otherwise resolve to "" — the cwd — and
        // pass the existence check.
        if let Some(env) = env_override.filter(|e| !e.is_empty()) {
            let path = expand_tilde(env);
            if path.exists() {
                return Some(path);
            }
        }
        if let Some(found) =
            Config::selected_model_file(settings).and_then(|f| ModelCatalog::installed_path(&f))
        {
            return Some(found);
        }
        for dir in Config::models_dirs() {
            for name in Config::PREFERRED_MODELS {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            if let Some(any) = any_ggml_model(&dir) {
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
        Config::models_dirs()
            .into_iter()
            .map(|dir| dir.join(file))
            .find(|p| p.exists())
    }
}
