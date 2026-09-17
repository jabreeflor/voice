//! `#[tauri::command]` handlers — the whole surface the HTML UI talks to.
//! Keep these thin: they read/write `App` state and return plain DTOs.
//!
//! Events emitted by the backend (listen with `window.__TAURI__.event.listen`):
//! - `status-changed`   (no payload)         status line / tray text changed
//! - `history-changed`  (no payload)         a dictation was added
//! - `snippets-changed` (no payload)         snippets.json changed (app or voicectl)
//! - `dictation-landed` (no payload)         a dictation was pasted (onboarding step 4)
//! - `window-visible`   (bool, per window)    main/onboarding shown or hidden: start/stop UI timers
//! - `overlay`, `level`, `sound`             see overlay.rs
//!
//! Sync commands run on the main thread. Each one takes at most one `App`
//! mutex and releases it before calling anything that could wait on the
//! main thread (see the lock rules in app.rs).

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use serde::Serialize;
use tauri::{Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use voice_core::{Config, HistoryStore, Hotkey, StatusColor};

use crate::app::App;
use crate::platform;

/// Ledger rows shown on the Dictations tab (Swift `grouped(max: 40)`).
const LEDGER_ENTRIES: usize = 40;

#[derive(Serialize, Clone, Debug)]
pub struct StatusDto {
    pub text: String,
    /// "red" | "orange" | "green"
    pub color: String,
    pub needs_accessibility: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct DictationDto {
    /// "h:mm a" local time, e.g. "3:07 PM"
    pub time: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct DictationGroupDto {
    pub title: String,
    pub entries: Vec<DictationDto>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DictationsDto {
    pub total_words: i64,
    pub average_wpm: i64,
    pub average_latency: f64,
    pub hotkey_short_label: String,
    pub groups: Vec<DictationGroupDto>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SnippetDto {
    pub trigger: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct HotkeyOptionDto {
    /// raw value, e.g. "rightOption"
    pub id: String,
    pub label: String,
    pub short_label: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SettingsDto {
    pub hotkey: String,
    pub hotkeys: Vec<HotkeyOptionDto>,
    pub sounds: bool,
    pub start_at_login: bool,
    pub trailing_space: bool,
    /// "macos" | "windows" | "linux"
    pub platform: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct PermissionsDto {
    /// "granted" | "denied" | "undetermined" | "not_applicable"
    pub mic: String,
    pub hotkeys_running: bool,
    pub accessibility_trusted: bool,
}

pub fn color_name(color: StatusColor) -> &'static str {
    match color {
        StatusColor::Red => "red",
        StatusColor::Orange => "orange",
        StatusColor::Green => "green",
    }
}

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// Swift `DateFormatter` "h:mm a": 12-hour clock without a leading zero.
pub fn format_time<Tz: TimeZone>(dt: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    dt.format("%-I:%M %p").to_string()
}

/// Builds the Dictations tab payload from a store (split out so it can be
/// exercised against a throwaway `HistoryStore`).
pub fn dictations_dto(history: &HistoryStore, hotkey: Hotkey) -> DictationsDto {
    let groups = history
        .grouped(LEDGER_ENTRIES)
        .into_iter()
        .map(|(title, entries)| DictationGroupDto {
            title,
            entries: entries
                .into_iter()
                .map(|e| DictationDto {
                    time: format_time(&DateTime::<Local>::from(e.system_time())),
                    text: e.text,
                })
                .collect(),
        })
        .collect();
    DictationsDto {
        total_words: history.total_words(),
        average_wpm: history.average_wpm(),
        average_latency: history.average_latency(),
        hotkey_short_label: hotkey.short_label(),
        groups,
    }
}

#[tauri::command]
pub fn get_status(app: State<'_, Arc<App>>) -> StatusDto {
    let info = app.status_info();
    StatusDto {
        text: info.text,
        color: color_name(info.color).to_string(),
        needs_accessibility: info.needs_accessibility,
    }
}

#[tauri::command]
pub fn get_dictations(app: State<'_, Arc<App>>) -> DictationsDto {
    let hotkey = Config::hotkey(&app.settings);
    let history = app.history.lock().unwrap_or_else(|e| e.into_inner());
    dictations_dto(&history, hotkey)
}

#[tauri::command]
pub fn get_snippets(app: State<'_, Arc<App>>) -> Vec<SnippetDto> {
    // voicectl (or a text editor) may have rewritten snippets.json since the
    // last read; the file is the source of truth.
    let list = {
        let mut snippets = app.snippets.lock().unwrap_or_else(|e| e.into_inner());
        snippets.reload_if_changed();
        snippets
            .snippets()
            .iter()
            .map(|s| SnippetDto {
                trigger: s.trigger.clone(),
                text: s.text.clone(),
            })
            .collect()
    };
    list
}

#[tauri::command]
pub fn add_snippet(app: State<'_, Arc<App>>, trigger: String, text: String) -> bool {
    let added = app
        .snippets
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .add(&trigger, &text);
    if added {
        let _ = app.handle.emit("snippets-changed", ());
    }
    added
}

#[tauri::command]
pub fn remove_snippet(app: State<'_, Arc<App>>, index: usize) {
    app.snippets
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove_at(index);
    let _ = app.handle.emit("snippets-changed", ());
}

#[tauri::command]
pub fn get_settings(app: State<'_, Arc<App>>) -> SettingsDto {
    let start_at_login = app.handle.autolaunch().is_enabled().unwrap_or_else(|e| {
        log::warn!("autostart query: {e}");
        false
    });
    SettingsDto {
        hotkey: Config::hotkey(&app.settings).raw_value().to_string(),
        hotkeys: Hotkey::available()
            .into_iter()
            .map(|hk| HotkeyOptionDto {
                id: hk.raw_value().to_string(),
                label: hk.label(),
                short_label: hk.short_label(),
            })
            .collect(),
        sounds: Config::sounds_enabled(&app.settings),
        start_at_login,
        trailing_space: Config::trailing_space(&app.settings),
        platform: platform_name().to_string(),
    }
}

#[tauri::command]
pub fn set_hotkey(app: State<'_, Arc<App>>, id: String) {
    match Hotkey::from_raw(&id) {
        Some(hk) => {
            Config::set_hotkey(&app.settings, hk);
            app.refresh_ui();
        }
        None => log::warn!("set_hotkey: unknown id {id:?}"),
    }
}

#[tauri::command]
pub fn set_sounds(app: State<'_, Arc<App>>, enabled: bool) {
    Config::set_sounds_enabled(&app.settings, enabled);
}

#[tauri::command]
pub fn set_start_at_login(app: State<'_, Arc<App>>, enabled: bool) -> Result<(), String> {
    let manager = app.handle.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_mic(app: State<'_, Arc<App>>) {
    app.inner().preview_mic();
}

#[tauri::command]
pub fn permission_state(app: State<'_, Arc<App>>) -> PermissionsDto {
    PermissionsDto {
        mic: platform::mic_status().as_str().to_string(),
        hotkeys_running: app.hotkeys_running(),
        accessibility_trusted: platform::accessibility_trusted(),
    }
}

#[tauri::command]
pub fn request_mic(app: State<'_, Arc<App>>) {
    let _ = app;
    platform::request_mic();
}

#[tauri::command]
pub fn request_accessibility(app: State<'_, Arc<App>>) {
    app.request_accessibility();
}

#[tauri::command]
pub fn open_accessibility_settings() {
    platform::open_accessibility_settings();
}

#[tauri::command]
pub fn finish_onboarding(app: State<'_, Arc<App>>) {
    app.finish_onboarding();
}

/// Backend half of the ledger row. The row's contract (pinned by
/// Tests/VoiceCoreTests/LedgerRowTests.swift for the AppKit `LedgerRow`) is
/// implemented by `ledgerRow` in ui/app.js: the row itself is a `<button>`
/// labelled "Copy dictation" with no nested Copy button, and pressing it
/// invokes this command with the entry's `text`. `dictations_dto_entries_
/// carry_exactly_time_and_text` below pins the payload it consumes.
#[tauri::command]
pub fn copy_text(text: String) {
    crate::paste::copy_text(&text);
}

/// Hands every command to the builder in one place.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_status,
        get_dictations,
        get_snippets,
        add_snippet,
        remove_snippet,
        get_settings,
        set_hotkey,
        set_sounds,
        set_start_at_login,
        preview_mic,
        permission_state,
        request_mic,
        request_accessibility,
        open_accessibility_settings,
        finish_onboarding,
        copy_text
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};
    use std::sync::Arc;
    use std::time::SystemTime;
    use voice_core::{DictationEntry, Settings};

    #[test]
    fn time_is_12_hour_without_leading_zero() {
        let dt = Utc.with_ymd_and_hms(2025, 9, 15, 15, 7, 0).unwrap();
        assert_eq!(format_time(&dt), "3:07 PM");
        let dt = Utc.with_ymd_and_hms(2025, 9, 15, 0, 5, 0).unwrap();
        assert_eq!(format_time(&dt), "12:05 AM");
        let dt = Utc.with_ymd_and_hms(2025, 9, 15, 12, 0, 0).unwrap();
        assert_eq!(format_time(&dt), "12:00 PM");
        // Any zone works; only the wall-clock part is printed.
        let tz = FixedOffset::east_opt(5 * 3600).unwrap();
        let dt = tz.with_ymd_and_hms(2025, 9, 15, 9, 30, 0).unwrap();
        assert_eq!(format_time(&dt), "9:30 AM");
    }

    #[test]
    fn status_colors_match_the_css_classes() {
        assert_eq!(color_name(StatusColor::Red), "red");
        assert_eq!(color_name(StatusColor::Orange), "orange");
        assert_eq!(color_name(StatusColor::Green), "green");
    }

    #[test]
    fn dictations_dto_groups_today_and_reports_raw_latency() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings = Arc::new(Settings::in_dir(dir.path()));
        let mut history = HistoryStore::new(dir.path().to_path_buf(), settings);
        let now = SystemTime::now();
        history.add(DictationEntry::new("first one", now, 2.0, 0.5));
        // Same timestamp on purpose: `now + 1 s` could cross midnight and
        // split the group.
        history.add(DictationEntry::new("second", now, 3.0, 1.5));
        let dto = dictations_dto(&history, Hotkey::RightOption);
        assert_eq!(dto.total_words, 3);
        assert_eq!(dto.hotkey_short_label, Hotkey::RightOption.short_label());
        // Raw seconds; the UI formats "1.0s" itself.
        assert!((dto.average_latency - 1.0).abs() < 1e-9);
        assert_eq!(dto.groups.len(), 1);
        assert_eq!(dto.groups[0].title, "Today");
        // Newest first, time in "h:mm a".
        assert_eq!(dto.groups[0].entries[0].text, "second");
        assert!(dto.groups[0].entries[0].time.ends_with('M'));
    }

    /// The ledger row (ui/app.js `ledgerRow`) reads `time` and `text` and
    /// hands `text` back to `copy_text`; renaming either breaks the copy
    /// silently, so the wire shape is pinned here (the DOM half lives in
    /// LedgerRowTests.swift for the AppKit app).
    #[test]
    fn dictations_dto_entries_carry_exactly_time_and_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings = Arc::new(Settings::in_dir(dir.path()));
        let mut history = HistoryStore::new(dir.path().to_path_buf(), settings);
        history.add(DictationEntry::new("copy me", SystemTime::now(), 1.0, 0.5));
        let dto = dictations_dto(&history, Hotkey::RightOption);
        let json = serde_json::to_value(&dto.groups[0].entries[0]).expect("serialise entry");
        let object = json.as_object().expect("entry is an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["text", "time"]);
        assert_eq!(object["text"], "copy me");
    }
}
