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
        todo!()
    }

    pub fn raw_value(self) -> &'static str {
        todo!()
    }

    pub fn from_raw(raw: &str) -> Option<Hotkey> {
        let _ = raw;
        todo!()
    }

    /// macOS virtual key code (61 / 54 / 63 / 62). Kept for parity and tests.
    pub fn key_code(self) -> i64 {
        todo!()
    }

    pub fn label(self) -> String {
        todo!()
    }

    pub fn short_label(self) -> String {
        todo!()
    }
}

/// Expands a leading `~` or `~/` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    let _ = path;
    todo!()
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

    pub fn hotkey(settings: &Settings) -> Hotkey {
        let _ = settings;
        todo!()
    }

    pub fn set_hotkey(settings: &Settings, hotkey: Hotkey) {
        let _ = (settings, hotkey);
        todo!()
    }

    pub fn sounds_enabled(settings: &Settings) -> bool {
        let _ = settings;
        todo!()
    }

    pub fn set_sounds_enabled(settings: &Settings, enabled: bool) {
        let _ = (settings, enabled);
        todo!()
    }

    pub fn trailing_space(settings: &Settings) -> bool {
        let _ = settings;
        todo!()
    }

    /// `~/voice/models` then `~/.voice/models`.
    pub fn models_dirs() -> Vec<PathBuf> {
        todo!()
    }

    pub fn selected_model_file(settings: &Settings) -> Option<String> {
        let _ = settings;
        todo!()
    }

    pub fn set_selected_model_file(settings: &Settings, file: Option<&str>) {
        let _ = (settings, file);
        todo!()
    }

    /// `VOICE_MODEL` override → selected model → preferred list → any `ggml*.bin`.
    pub fn find_model(settings: &Settings) -> Option<PathBuf> {
        Config::find_model_with_env(settings, std::env::var("VOICE_MODEL").ok().as_deref())
    }

    /// Same as `find_model` with the environment override passed explicitly (testable).
    pub fn find_model_with_env(settings: &Settings, env_override: Option<&str>) -> Option<PathBuf> {
        let _ = (settings, env_override);
        todo!()
    }
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
        todo!()
    }

    pub fn download_url(spec: &ModelSpec) -> String {
        let _ = spec;
        todo!()
    }

    pub fn installed_path(file: &str) -> Option<PathBuf> {
        let _ = file;
        todo!()
    }
}

#[allow(dead_code)]
fn _path_marker(_: &Path) {}
