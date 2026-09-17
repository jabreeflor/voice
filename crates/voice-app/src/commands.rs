//! `#[tauri::command]` handlers — the whole surface the HTML UI talks to.
//! Keep these thin: they read/write `App` state and return plain DTOs.
//!
//! Events emitted by the backend (listen with `window.__TAURI__.event.listen`):
//! - `status-changed`   (no payload)         status line / tray text changed
//! - `history-changed`  (no payload)         a dictation was added
//! - `snippets-changed` (no payload)         snippets.json changed (app or voicectl)
//! - `dictation-landed` (no payload)         a dictation was pasted (onboarding step 4)
//! - `overlay`, `level`, `sound`             see overlay.rs

#![allow(dead_code)]

use serde::Serialize;
use tauri::State;

use crate::app::App;

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

#[tauri::command]
pub fn get_status(app: State<'_, std::sync::Arc<App>>) -> StatusDto {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn get_dictations(app: State<'_, std::sync::Arc<App>>) -> DictationsDto {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn get_snippets(app: State<'_, std::sync::Arc<App>>) -> Vec<SnippetDto> {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn add_snippet(app: State<'_, std::sync::Arc<App>>, trigger: String, text: String) -> bool {
    let _ = (app, trigger, text);
    todo!()
}

#[tauri::command]
pub fn remove_snippet(app: State<'_, std::sync::Arc<App>>, index: usize) {
    let _ = (app, index);
    todo!()
}

#[tauri::command]
pub fn get_settings(app: State<'_, std::sync::Arc<App>>) -> SettingsDto {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn set_hotkey(app: State<'_, std::sync::Arc<App>>, id: String) {
    let _ = (app, id);
    todo!()
}

#[tauri::command]
pub fn set_sounds(app: State<'_, std::sync::Arc<App>>, enabled: bool) {
    let _ = (app, enabled);
    todo!()
}

#[tauri::command]
pub fn set_start_at_login(
    app: State<'_, std::sync::Arc<App>>,
    enabled: bool,
) -> Result<(), String> {
    let _ = (app, enabled);
    todo!()
}

#[tauri::command]
pub fn preview_mic(app: State<'_, std::sync::Arc<App>>) {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn permission_state(app: State<'_, std::sync::Arc<App>>) -> PermissionsDto {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn request_mic(app: State<'_, std::sync::Arc<App>>) {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn request_accessibility(app: State<'_, std::sync::Arc<App>>) {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn open_accessibility_settings() {
    todo!()
}

#[tauri::command]
pub fn finish_onboarding(app: State<'_, std::sync::Arc<App>>) {
    let _ = app;
    todo!()
}

#[tauri::command]
pub fn copy_text(text: String) {
    let _ = text;
    todo!()
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
