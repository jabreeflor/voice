//! Status-bar / tray icon and menu. Reference: `setupStatusItem`, `refreshMenu`
//! and `setIcon` in Sources/VoiceCore/app.swift.
//!
//! Menu: "<Voice — hold <label> to dictate>" (disabled), "<status text>"
//! (disabled), separator, "Open Voice", "Copy Last Dictation" (disabled while
//! history is empty), "Setup Assistant…", separator, "Quit Voice".

#![allow(dead_code)]

use std::sync::Arc;

use tauri::AppHandle;

use crate::app::App;

pub const TRAY_ID: &str = "voice-tray";

/// Builds the tray once during setup. `icons/tray.png` is a template image on
/// macOS; `icons/tray-recording.png` is the red glyph shown while recording.
pub fn build(handle: &AppHandle, app: Arc<App>) -> tauri::Result<()> {
    let _ = (handle, app);
    todo!()
}

/// Updates the title/status items and the Copy Last enabled state.
pub fn refresh(handle: &AppHandle, app: &App) {
    let _ = (handle, app);
    todo!()
}

pub fn set_recording(handle: &AppHandle, recording: bool) {
    let _ = (handle, recording);
    todo!()
}
