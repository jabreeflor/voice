//! Status-bar / tray icon and menu. Reference: `setupStatusItem`, `refreshMenu`
//! and `setIcon` in Sources/VoiceCore/app.swift.
//!
//! Menu: "<Voice — hold <label> to dictate>" (disabled), "<status text>"
//! (disabled), separator, "Open Voice", "Copy Last Dictation" (disabled while
//! history is empty), "Setup Assistant…", separator, "Quit Voice".
//!
//! `MenuItem::set_text` / `set_enabled` and `TrayIcon::set_icon` round-trip
//! to the main thread, so callers must not hold any `App` mutex (see the
//! lock rules in app.rs). `refresh` and `set_recording` read what they need
//! from `App` first and only then touch the menu.

#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;
use voice_core::Config;

use crate::app::{App, TrayItems};

pub const TRAY_ID: &str = "voice-tray";

const ID_TITLE: &str = "title";
const ID_STATUS: &str = "status";
const ID_OPEN: &str = "open";
const ID_COPY_LAST: &str = "copy-last";
const ID_SETUP: &str = "setup";
const ID_QUIT: &str = "quit";

const IDLE_PNG: &[u8] = include_bytes!("../icons/tray.png");
const RECORDING_PNG: &[u8] = include_bytes!("../icons/tray-recording.png");

/// Decoded once; `Image::from_bytes` re-runs the PNG decoder otherwise.
fn icon(recording: bool) -> Option<Image<'static>> {
    static IDLE: OnceLock<Option<Image<'static>>> = OnceLock::new();
    static RECORDING: OnceLock<Option<Image<'static>>> = OnceLock::new();
    let (cell, bytes) = if recording {
        (&RECORDING, RECORDING_PNG)
    } else {
        (&IDLE, IDLE_PNG)
    };
    cell.get_or_init(|| match Image::from_bytes(bytes) {
        Ok(img) => Some(img),
        Err(e) => {
            log::error!("tray icon: {e}");
            None
        }
    })
    .clone()
}

pub fn title_text(hotkey_label: &str) -> String {
    format!("Voice — hold {hotkey_label} to dictate")
}

/// Builds the tray once during setup. `icons/tray.png` is a template image on
/// macOS; `icons/tray-recording.png` is the red glyph shown while recording.
pub fn build(handle: &AppHandle, app: Arc<App>) -> tauri::Result<()> {
    let title = MenuItem::with_id(handle, ID_TITLE, "Voice", false, None::<&str>)?;
    let status = MenuItem::with_id(handle, ID_STATUS, "", false, None::<&str>)?;
    let open = MenuItem::with_id(handle, ID_OPEN, "Open Voice", true, None::<&str>)?;
    let copy_last = MenuItem::with_id(
        handle,
        ID_COPY_LAST,
        "Copy Last Dictation",
        false,
        None::<&str>,
    )?;
    let setup = MenuItem::with_id(handle, ID_SETUP, "Setup Assistant…", true, None::<&str>)?;
    let quit = MenuItem::with_id(handle, ID_QUIT, "Quit Voice", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        handle,
        &[
            &title,
            &status,
            &PredefinedMenuItem::separator(handle)?,
            &open,
            &copy_last,
            &setup,
            &PredefinedMenuItem::separator(handle)?,
            &quit,
        ],
    )?;

    *app.tray_items.lock().unwrap_or_else(|e| e.into_inner()) = Some(TrayItems {
        title,
        status,
        copy_last,
    });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Voice")
        .icon_as_template(true);
    if let Some(img) = icon(false) {
        builder = builder.icon(img);
    }
    let menu_app = app.clone();
    builder = builder.on_menu_event(move |handle, event| match event.id.as_ref() {
        ID_OPEN => menu_app.show_main_window(),
        ID_COPY_LAST => menu_app.copy_last(),
        ID_SETUP => menu_app.show_onboarding(),
        ID_QUIT => handle.exit(0),
        _ => {}
    });
    builder.build(handle)?;

    refresh(handle, &app);
    Ok(())
}

/// Updates the title/status items and the Copy Last enabled state.
pub fn refresh(handle: &AppHandle, app: &App) {
    let _ = handle;
    let title = title_text(&Config::hotkey(&app.settings).label());
    let status = app.status_info().text;
    let has_history = !app
        .history
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries()
        .is_empty();
    // Items are cloned out so the lock is not held across the main-thread
    // round trips below (`MenuItem` is an `Arc` handle).
    let items = {
        let guard = app.tray_items.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .map(|t| (t.title.clone(), t.status.clone(), t.copy_last.clone()))
    };
    let Some((title_item, status_item, copy_item)) = items else {
        return;
    };
    let _ = title_item.set_text(title);
    let _ = status_item.set_text(status);
    let _ = copy_item.set_enabled(has_history);
}

pub fn set_recording(handle: &AppHandle, recording: bool) {
    let Some(tray) = handle.tray_by_id(TRAY_ID) else {
        return;
    };
    // The idle glyph is a template (tinted by the menu bar); the recording
    // one carries its own red and must not be.
    if let Err(e) = tray.set_icon_with_as_template(icon(recording), !recording) {
        log::warn!("tray icon: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_names_the_talk_key() {
        assert_eq!(
            title_text("Right ⌥ Option"),
            "Voice — hold Right ⌥ Option to dictate"
        );
    }

    #[test]
    fn bundled_tray_icons_decode() {
        // Both PNGs are compiled in; a corrupt asset would otherwise only
        // show up as a blank tray at runtime.
        assert!(icon(false).is_some());
        assert!(icon(true).is_some());
    }
}
