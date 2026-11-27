//! Status-bar / tray icon and menu. Reference: `setupStatusItem`, `refreshMenu`
//! and `setIcon` in Sources/VoiceCore/app.swift.
//!
//! Menu: "<Voice — hold <label> to dictate>" (disabled), "<status text>"
//! (disabled), separator, "Open Voice", "Settings…", "Copy Last Dictation"
//! (disabled while history is empty), "Setup Assistant…", "Check for Updates…"
//! (release builds only; retitled by updates.rs), separator, "Quit Voice".
//!
//! `MenuItem::set_text` / `set_enabled` and `TrayIcon::set_icon` round-trip
//! to the main thread, so callers must not hold any `App` mutex (see the
//! lock rules in app.rs). `refresh` and `set_recording` read what they need
//! from `App` first and only then touch the menu.

use std::sync::{Arc, OnceLock};

use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::app::{App, TrayItems};
use crate::commands::effective_hotkey;
use crate::updates::{self, UpdateStatus};

pub const TRAY_ID: &str = "voice-tray";

const ID_TITLE: &str = "title";
const ID_STATUS: &str = "status";
const ID_OPEN: &str = "open";
const ID_SETTINGS: &str = "settings";
const ID_COPY_LAST: &str = "copy-last";
const ID_SETUP: &str = "setup";
const ID_UPDATE: &str = "update";
const ID_QUIT: &str = "quit";

// Every tray PNG is rendered from the SVG next to it by scripts/icons.sh.
const TEMPLATE_PNG: &[u8] = include_bytes!("../icons/tray.png");
const COLOR_PNG: &[u8] = include_bytes!("../icons/tray-color.png");
const RECORDING_PNG: &[u8] = include_bytes!("../icons/tray-recording.png");
/// macOS tints the black template glyph for the menu bar's appearance.
/// Windows and Linux have no template images and the taskbar can be light or
/// dark, where a black glyph would vanish, so they get the full-colour mark.
const IDLE_PNG: &[u8] = if cfg!(target_os = "macos") {
    TEMPLATE_PNG
} else {
    COLOR_PNG
};

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
    let settings = MenuItem::with_id(handle, ID_SETTINGS, "Settings…", true, None::<&str>)?;
    let copy_last = MenuItem::with_id(
        handle,
        ID_COPY_LAST,
        "Copy Last Dictation",
        false,
        None::<&str>,
    )?;
    let setup = MenuItem::with_id(handle, ID_SETUP, "Setup Assistant…", true, None::<&str>)?;
    let update = match updates::menu_item(&updates::status(handle)) {
        Some((text, enabled)) => Some(MenuItem::with_id(
            handle,
            ID_UPDATE,
            text,
            enabled,
            None::<&str>,
        )?),
        None => None,
    };
    let quit = MenuItem::with_id(handle, ID_QUIT, "Quit Voice", true, Some("CmdOrCtrl+Q"))?;
    let separator_top = PredefinedMenuItem::separator(handle)?;
    let separator_bottom = PredefinedMenuItem::separator(handle)?;
    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![
        &title,
        &status,
        &separator_top,
        &open,
        &settings,
        &copy_last,
        &setup,
    ];
    if let Some(update) = &update {
        items.push(update);
    }
    items.extend([&separator_bottom as &dyn IsMenuItem<tauri::Wry>, &quit]);
    let menu = Menu::with_items(handle, &items)?;

    *app.tray_items.lock().unwrap_or_else(|e| e.into_inner()) = Some(TrayItems {
        title,
        status,
        copy_last,
        update,
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
        ID_SETTINGS => menu_app.show_settings(),
        ID_COPY_LAST => menu_app.copy_last(),
        ID_SETUP => menu_app.show_onboarding(),
        ID_UPDATE => updates::menu_clicked(handle),
        ID_QUIT => handle.exit(0),
        _ => {}
    });
    builder.build(handle)?;

    refresh(&app);
    Ok(())
}

/// Updates the title/status items and the Copy Last enabled state.
pub fn refresh(app: &App) {
    let title = title_text(&effective_hotkey(&app.settings).label());
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

/// Retitles Check for Updates… for `status` (updates.rs). A no-op before the
/// tray exists and in builds that do not update. Only macOS ever updates.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn set_update_item(handle: &AppHandle, status: &UpdateStatus) {
    let Some(app) = handle.try_state::<Arc<App>>() else {
        return;
    };
    let item = {
        let guard = app.tray_items.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().and_then(|t| t.update.clone())
    };
    if let (Some(item), Some((text, enabled))) = (item, updates::menu_item(status)) {
        let _ = item.set_text(text);
        let _ = item.set_enabled(enabled);
    }
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
        // The other platform's idle glyph is compiled in too; check it here
        // so a macOS run also catches a broken Windows/Linux asset.
        for png in [TEMPLATE_PNG, COLOR_PNG] {
            assert!(Image::from_bytes(png).is_ok());
        }
    }

    #[test]
    fn tray_glyphs_are_square_with_alpha() {
        // A tray slot is square; a non-square or opaque PNG would be
        // squashed or drawn as a solid block by the menu bar / taskbar.
        for png in [TEMPLATE_PNG, COLOR_PNG, RECORDING_PNG] {
            let img = Image::from_bytes(png).expect("decodes");
            assert_eq!(img.width(), img.height());
            assert!(img.width() >= 44);
            assert!(img.rgba().chunks(4).any(|px| px[3] == 0));
        }
    }
}
