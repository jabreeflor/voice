//! Hotkey choices, model discovery and the downloadable model catalog.
//! Behavioural reference: `enum Hotkey`, `enum Config`, `ModelCatalog` in
//! Sources/VoiceCore/core.swift.

use std::path::{Path, PathBuf};

use crate::polish::{DEFAULT_POLISH_MODEL, DEFAULT_POLISH_URL};
use crate::settings::Settings;

/// One row of the talk-key table behind `Hotkey`.
struct Row {
    /// The Swift `UserDefaults` string for the four legacy keys; the W3C
    /// `KeyboardEvent.code` for every other key.
    raw: &'static str,
    /// W3C `KeyboardEvent.code` as the webview reports it. `None` for fn,
    /// which never reaches a webview.
    code: Option<&'static str>,
    /// macOS virtual keycode (`kVK_*`, as in rdev's macOS table).
    key_code: i64,
    class: Class,
    /// Label on macOS, and the short form of it.
    mac: &'static str,
    mac_short: &'static str,
    /// Label on Windows and Linux. The Command key reads "Win" here and is
    /// renamed to "Super" on Linux by `pc_name`.
    pc: &'static str,
}

/// What a key does to the rest of the desktop, which decides whether the
/// picker warns about it and which platforms can offer it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Option, Command, Control, Shift (left and right) and fn. These only
    /// change modifier state, so they are safe to hold.
    Modifier,
    /// F1 to F12.
    Function,
    /// Keys that type a character or move the caret.
    Text,
}

/// Declares `Hotkey` and its row table from one list, so a variant, its raw
/// value, keycode, class and labels are written once and cannot drift. Rows
/// are in declaration order, which is what `Hotkey::row` indexes by.
macro_rules! hotkeys {
    ($(#[$enum_doc:meta])* enum Hotkey; $(
        $(#[$doc:meta])*
        $variant:ident = ($raw:literal, $code:expr, $key_code:expr, $class:ident,
            [$mac:literal, $mac_short:literal, $pc:literal])
    ),* $(,)?) => {
        $(#[$enum_doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub enum Hotkey {
            $( $(#[$doc])* $variant, )*
        }

        const HOTKEY_COUNT: usize = [$(stringify!($variant)),*].len();

        impl Hotkey {
            /// Every key, in the declaration order of the enum.
            pub const ALL: [Hotkey; HOTKEY_COUNT] = [$(Hotkey::$variant),*];

            const ROWS: [Row; HOTKEY_COUNT] = [$(
                Row {
                    raw: $raw,
                    code: $code,
                    key_code: $key_code,
                    class: Class::$class,
                    mac: $mac,
                    mac_short: $mac_short,
                    pc: $pc,
                },
            )*];
        }
    };
}

hotkeys! {
    /// The hold-to-talk key: one key from the keyboard the talk-key picker
    /// draws (ui/keyboard.js). Escape (the cancel key) and Caps Lock (a toggle
    /// that cannot be swallowed cleanly) are deliberately not offered.
    ///
    /// Raw values are the Swift `UserDefaults` strings for the four keys the
    /// Swift app could record, so a migrated settings file keeps its meaning.
    /// Every other key's raw value is its W3C `KeyboardEvent.code`.
    enum Hotkey;

    /// macOS Right ⌥ Option; Windows/Linux Right Alt (AltGr).
    RightOption = ("rightOption", Some("AltRight"), 61, Modifier,
        ["Right ⌥ Option", "Right ⌥", "Right Alt"]),
    /// macOS Right ⌘ Command; Windows Right Win; Linux Right Super.
    RightCommand = ("rightCommand", Some("MetaRight"), 54, Modifier,
        ["Right ⌘ Command", "Right ⌘", "Right Win"]),
    /// macOS fn/🌐. Not deliverable to apps on Windows/Linux, so not offered there.
    Fn = ("fn", None, 63, Modifier, ["fn", "fn", "fn"]),
    /// Right Control. Offered on Windows/Linux only.
    RightControl = ("rightControl", Some("ControlRight"), 62, Modifier,
        ["Right ⌃ Control", "Right ⌃", "Right Ctrl"]),

    LeftOption = ("AltLeft", Some("AltLeft"), 58, Modifier,
        ["Left ⌥ Option", "Left ⌥", "Left Alt"]),
    LeftCommand = ("MetaLeft", Some("MetaLeft"), 55, Modifier,
        ["Left ⌘ Command", "Left ⌘", "Left Win"]),
    LeftControl = ("ControlLeft", Some("ControlLeft"), 59, Modifier,
        ["Left ⌃ Control", "Left ⌃", "Left Ctrl"]),
    LeftShift = ("ShiftLeft", Some("ShiftLeft"), 56, Modifier,
        ["Left ⇧ Shift", "Left ⇧", "Left Shift"]),
    RightShift = ("ShiftRight", Some("ShiftRight"), 60, Modifier,
        ["Right ⇧ Shift", "Right ⇧", "Right Shift"]),

    F1 = ("F1", Some("F1"), 122, Function, ["F1", "F1", "F1"]),
    F2 = ("F2", Some("F2"), 120, Function, ["F2", "F2", "F2"]),
    F3 = ("F3", Some("F3"), 99, Function, ["F3", "F3", "F3"]),
    F4 = ("F4", Some("F4"), 118, Function, ["F4", "F4", "F4"]),
    F5 = ("F5", Some("F5"), 96, Function, ["F5", "F5", "F5"]),
    F6 = ("F6", Some("F6"), 97, Function, ["F6", "F6", "F6"]),
    F7 = ("F7", Some("F7"), 98, Function, ["F7", "F7", "F7"]),
    F8 = ("F8", Some("F8"), 100, Function, ["F8", "F8", "F8"]),
    F9 = ("F9", Some("F9"), 101, Function, ["F9", "F9", "F9"]),
    F10 = ("F10", Some("F10"), 109, Function, ["F10", "F10", "F10"]),
    F11 = ("F11", Some("F11"), 103, Function, ["F11", "F11", "F11"]),
    F12 = ("F12", Some("F12"), 111, Function, ["F12", "F12", "F12"]),

    A = ("KeyA", Some("KeyA"), 0, Text, ["A", "A", "A"]),
    B = ("KeyB", Some("KeyB"), 11, Text, ["B", "B", "B"]),
    C = ("KeyC", Some("KeyC"), 8, Text, ["C", "C", "C"]),
    D = ("KeyD", Some("KeyD"), 2, Text, ["D", "D", "D"]),
    E = ("KeyE", Some("KeyE"), 14, Text, ["E", "E", "E"]),
    F = ("KeyF", Some("KeyF"), 3, Text, ["F", "F", "F"]),
    G = ("KeyG", Some("KeyG"), 5, Text, ["G", "G", "G"]),
    H = ("KeyH", Some("KeyH"), 4, Text, ["H", "H", "H"]),
    I = ("KeyI", Some("KeyI"), 34, Text, ["I", "I", "I"]),
    J = ("KeyJ", Some("KeyJ"), 38, Text, ["J", "J", "J"]),
    K = ("KeyK", Some("KeyK"), 40, Text, ["K", "K", "K"]),
    L = ("KeyL", Some("KeyL"), 37, Text, ["L", "L", "L"]),
    M = ("KeyM", Some("KeyM"), 46, Text, ["M", "M", "M"]),
    N = ("KeyN", Some("KeyN"), 45, Text, ["N", "N", "N"]),
    O = ("KeyO", Some("KeyO"), 31, Text, ["O", "O", "O"]),
    P = ("KeyP", Some("KeyP"), 35, Text, ["P", "P", "P"]),
    Q = ("KeyQ", Some("KeyQ"), 12, Text, ["Q", "Q", "Q"]),
    R = ("KeyR", Some("KeyR"), 15, Text, ["R", "R", "R"]),
    S = ("KeyS", Some("KeyS"), 1, Text, ["S", "S", "S"]),
    T = ("KeyT", Some("KeyT"), 17, Text, ["T", "T", "T"]),
    U = ("KeyU", Some("KeyU"), 32, Text, ["U", "U", "U"]),
    V = ("KeyV", Some("KeyV"), 9, Text, ["V", "V", "V"]),
    W = ("KeyW", Some("KeyW"), 13, Text, ["W", "W", "W"]),
    X = ("KeyX", Some("KeyX"), 7, Text, ["X", "X", "X"]),
    Y = ("KeyY", Some("KeyY"), 16, Text, ["Y", "Y", "Y"]),
    Z = ("KeyZ", Some("KeyZ"), 6, Text, ["Z", "Z", "Z"]),

    Digit0 = ("Digit0", Some("Digit0"), 29, Text, ["0", "0", "0"]),
    Digit1 = ("Digit1", Some("Digit1"), 18, Text, ["1", "1", "1"]),
    Digit2 = ("Digit2", Some("Digit2"), 19, Text, ["2", "2", "2"]),
    Digit3 = ("Digit3", Some("Digit3"), 20, Text, ["3", "3", "3"]),
    Digit4 = ("Digit4", Some("Digit4"), 21, Text, ["4", "4", "4"]),
    Digit5 = ("Digit5", Some("Digit5"), 23, Text, ["5", "5", "5"]),
    Digit6 = ("Digit6", Some("Digit6"), 22, Text, ["6", "6", "6"]),
    Digit7 = ("Digit7", Some("Digit7"), 26, Text, ["7", "7", "7"]),
    Digit8 = ("Digit8", Some("Digit8"), 28, Text, ["8", "8", "8"]),
    Digit9 = ("Digit9", Some("Digit9"), 25, Text, ["9", "9", "9"]),

    Backquote = ("Backquote", Some("Backquote"), 50, Text, ["`", "`", "`"]),
    Minus = ("Minus", Some("Minus"), 27, Text, ["-", "-", "-"]),
    Equal = ("Equal", Some("Equal"), 24, Text, ["=", "=", "="]),
    BracketLeft = ("BracketLeft", Some("BracketLeft"), 33, Text, ["[", "[", "["]),
    BracketRight = ("BracketRight", Some("BracketRight"), 30, Text, ["]", "]", "]"]),
    Backslash = ("Backslash", Some("Backslash"), 42, Text, ["\\", "\\", "\\"]),
    Semicolon = ("Semicolon", Some("Semicolon"), 41, Text, [";", ";", ";"]),
    Quote = ("Quote", Some("Quote"), 39, Text, ["'", "'", "'"]),
    Comma = ("Comma", Some("Comma"), 43, Text, [",", ",", ","]),
    Period = ("Period", Some("Period"), 47, Text, [".", ".", "."]),
    Slash = ("Slash", Some("Slash"), 44, Text, ["/", "/", "/"]),

    Space = ("Space", Some("Space"), 49, Text, ["Space", "Space", "Space"]),
    Tab = ("Tab", Some("Tab"), 48, Text, ["Tab", "Tab", "Tab"]),
    Enter = ("Enter", Some("Enter"), 36, Text, ["Return", "Return", "Enter"]),
    Backspace = ("Backspace", Some("Backspace"), 51, Text, ["Delete", "Delete", "Backspace"]),
    ArrowUp = ("ArrowUp", Some("ArrowUp"), 126, Text, ["↑", "↑", "↑"]),
    ArrowDown = ("ArrowDown", Some("ArrowDown"), 125, Text, ["↓", "↓", "↓"]),
    ArrowLeft = ("ArrowLeft", Some("ArrowLeft"), 123, Text, ["←", "←", "←"]),
    ArrowRight = ("ArrowRight", Some("ArrowRight"), 124, Text, ["→", "→", "→"]),
}

/// The Command key is the Windows key on Windows and Super on Linux.
fn pc_name(name: &'static str) -> String {
    if cfg!(target_os = "windows") {
        name.to_string()
    } else {
        name.replace("Win", "Super")
    }
}

impl Hotkey {
    fn row(self) -> &'static Row {
        // `ROWS` is generated from the same list as the enum, so the index of
        // a variant is its row.
        &Hotkey::ROWS[self as usize]
    }

    /// Whether this platform can hook the key. macOS offers every key but
    /// Right Control, which Apple keyboards do not have. Windows lacks fn. Linux offers only modifiers and F-keys:
    /// rdev's `listen` on X11 uses XRecord, which cannot swallow events, so
    /// a letter, digit or Space as the talk key would type into the focused
    /// app on every press.
    fn offered_here(self) -> bool {
        if cfg!(target_os = "macos") {
            self != Hotkey::RightControl
        } else if cfg!(target_os = "windows") {
            self != Hotkey::Fn
        } else {
            (self.is_modifier() && self != Hotkey::Fn) || self.row().class == Class::Function
        }
    }

    /// Hotkeys the current platform can actually hook, in the order the UI lists them.
    /// Index 0 is the platform default and the fallback everywhere else.
    pub fn available() -> Vec<Hotkey> {
        let leading = if cfg!(target_os = "macos") {
            [Hotkey::RightOption, Hotkey::RightCommand, Hotkey::Fn]
        } else {
            [
                Hotkey::RightOption,
                Hotkey::RightControl,
                Hotkey::RightCommand,
            ]
        };
        let mut out = leading.to_vec();
        for hk in Hotkey::ALL {
            if !out.contains(&hk) && hk.offered_here() {
                out.push(hk);
            }
        }
        out
    }

    /// The Swift `UserDefaults` string for the four legacy keys, the W3C
    /// `KeyboardEvent.code` for every other key.
    pub fn raw_value(self) -> &'static str {
        self.row().raw
    }

    /// Accepts every `raw_value`, and also the W3C code of a key: the codes
    /// `AltRight`, `MetaRight` and `ControlRight` therefore name the legacy
    /// right-hand keys, so a code sent from the picker normalises.
    pub fn from_raw(raw: &str) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|hk| hk.raw_value() == raw || hk.code() == Some(raw))
    }

    /// The W3C `KeyboardEvent.code` the webview reports for this key. `None`
    /// for fn, which never reaches a webview.
    pub fn code(self) -> Option<&'static str> {
        self.row().code
    }

    /// macOS virtual keycode (`kVK_*`). Modifiers arrive as `flagsChanged`
    /// with this code; other keys as `keyDown`/`keyUp`.
    pub fn key_code(self) -> i64 {
        self.row().key_code
    }

    /// Option, Command, Control, Shift (left and right) and fn. On macOS these
    /// arrive as `flagsChanged` and are never swallowed, so holding one as the
    /// talk key does not stop it working as a modifier in other apps. Every
    /// other talk key is swallowed while held.
    pub fn is_modifier(self) -> bool {
        self.row().class == Class::Modifier
    }

    /// True for keys that type a character or move the caret. Holding one as
    /// the talk key swallows it, so it no longer does its usual job while
    /// it is the talk key; the picker warns about that.
    pub fn types_text(self) -> bool {
        self.row().class == Class::Text
    }

    pub fn label(self) -> String {
        let row = self.row();
        if cfg!(target_os = "macos") {
            row.mac.to_string()
        } else {
            pc_name(row.pc)
        }
    }

    pub fn short_label(self) -> String {
        if cfg!(target_os = "macos") {
            self.row().mac_short.to_string()
        } else {
            // The Windows/Linux names have no glyph form to shorten to.
            self.label()
        }
    }
}

/// Window colour scheme. `System` follows the OS light/dark setting; the
/// other two pin it. Not a Swift setting (the AppKit app only followed the
/// system), so the raw values are new and lower-case like the CSS
/// `data-theme` attribute ui/app.css keys its dark tokens on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    /// In the order the Appearance select lists them.
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn raw_value(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Appearance> {
        Appearance::ALL.into_iter().find(|a| a.raw_value() == raw)
    }

    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "Match system",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
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

    /// Default `System`; an unknown raw value falls back the same way an
    /// unknown hotkey does.
    pub fn appearance(settings: &Settings) -> Appearance {
        settings
            .get_string("appearance")
            .and_then(|raw| Appearance::from_raw(&raw))
            .unwrap_or(Appearance::System)
    }

    pub fn set_appearance(settings: &Settings, appearance: Appearance) {
        settings.set("appearance", appearance.raw_value());
    }

    pub fn trailing_space(settings: &Settings) -> bool {
        settings.get_bool("trailingSpace").unwrap_or(true)
    }

    /// `contextPrompt` (Rust-only): send the vocabulary and snippet triggers
    /// to the local whisper-server as its prompt. Default on.
    pub fn context_prompt(settings: &Settings) -> bool {
        settings.get_bool("contextPrompt").unwrap_or(true)
    }

    pub fn set_context_prompt(settings: &Settings, enabled: bool) {
        settings.set("contextPrompt", enabled);
    }

    /// `removeFillers` (Rust-only): drop um/uh/erm before pasting
    /// (`transcript::remove_fillers`). Default on.
    pub fn remove_fillers(settings: &Settings) -> bool {
        settings.get_bool("removeFillers").unwrap_or(true)
    }

    pub fn set_remove_fillers(settings: &Settings, enabled: bool) {
        settings.set("removeFillers", enabled);
    }

    /// `llmPolish` (Rust-only): run the transcript through a local LLM
    /// (`polish::polish`). Default off: it needs a model server the user
    /// installs and runs themselves.
    pub fn llm_polish(settings: &Settings) -> bool {
        settings.get_bool("llmPolish").unwrap_or(false)
    }

    pub fn set_llm_polish(settings: &Settings, enabled: bool) {
        settings.set("llmPolish", enabled);
    }

    /// `llmPolishUrl`; a blank value means the default (Ollama on 11434).
    pub fn llm_polish_url(settings: &Settings) -> String {
        settings
            .get_string("llmPolishUrl")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_POLISH_URL.to_string())
    }

    /// Stored trimmed; blank removes the key so the default applies again.
    pub fn set_llm_polish_url(settings: &Settings, url: &str) {
        match url.trim() {
            "" => settings.remove("llmPolishUrl"),
            url => settings.set("llmPolishUrl", url),
        }
    }

    /// `llmPolishModel`; a blank value means the default.
    pub fn llm_polish_model(settings: &Settings) -> String {
        settings
            .get_string("llmPolishModel")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_POLISH_MODEL.to_string())
    }

    pub fn set_llm_polish_model(settings: &Settings, model: &str) {
        match model.trim() {
            "" => settings.remove("llmPolishModel"),
            model => settings.set("llmPolishModel", model),
        }
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
        // Swift's `URL(fileURLWithPath: "")` standardizes to the cwd and would
        // pass `fileExists`; Rust's `Path::new("")` never exists, so this guard
        // only makes the spec's "empty is ignored" rule explicit and immune to
        // a future canonicalizing change.
        if let Some(env) = env_override.filter(|e| !e.is_empty()) {
            let path = expand_tilde(env);
            if path.exists() {
                // `URL(fileURLWithPath:)` always yielded an absolute path, so a
                // relative override is anchored to the cwd here too. `absolute`
                // does not touch the filesystem or resolve symlinks, unlike
                // `canonicalize`.
                return Some(std::path::absolute(&path).unwrap_or(path));
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
    // Keep the `OsString`: Swift's `lastPathComponent` matched any byte
    // sequence, so a non-UTF-8 name must not silently vanish from the one
    // path that exists to rescue an oddly named model. `OsString` sorts by
    // bytes, which is the same order as `String` for valid UTF-8.
    let mut names: Vec<std::ffi::OsString> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| {
            n.to_string_lossy().starts_with("ggml")
                && Path::new(n).extension().is_some_and(|x| x == "bin")
        })
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
