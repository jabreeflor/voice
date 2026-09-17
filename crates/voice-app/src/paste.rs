//! Paste into the frontmost app. Reference: `pasteText` in
//! Sources/VoiceCore/core.swift.
//!
//! Puts `text` on the clipboard, synthesizes ⌘V (macOS) / Ctrl+V (Windows,
//! Linux), then restores the previous clipboard text 600 ms later — long
//! enough for the target app to have consumed the paste.
//!
//! On macOS the synthesized keystroke goes through CGEvent, which requires
//! the app to be trusted in System Settings → Privacy & Security →
//! Accessibility (the same grant the hotkey hook needs). Without it
//! `Enigo::new` fails and the text stays on the clipboard for a manual ⌘V.
//! Nothing here panics: every failure is logged and the function returns.

#![allow(dead_code)]

use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Swift restores after 0.6 s; shorter and slow apps paste the old contents.
const RESTORE_DELAY: Duration = Duration::from_millis(600);

pub fn paste_text(text: &str) {
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("clipboard unavailable: {e}");
            return;
        }
    };
    // Err means "nothing textual on the clipboard" (empty, image, ...);
    // Swift's `string(forType:)` returns nil there and skips the restore.
    let saved = clipboard.get_text().ok();
    if let Err(e) = clipboard.set_text(text) {
        log::error!("clipboard write failed: {e}");
        return;
    }
    send_paste_shortcut();

    if let Some(saved) = saved {
        // The same `Clipboard` handle is moved into the thread on purpose:
        // on X11 the process serving the selection is this handle, and
        // dropping it early can hand the contents to a clipboard manager
        // (or lose them) before the target app has read them.
        thread::spawn(move || {
            thread::sleep(RESTORE_DELAY);
            if let Err(e) = clipboard.set_text(saved) {
                log::warn!("clipboard restore failed: {e}");
            }
        });
    }
}

/// Plain clipboard write (menu "Copy Last Dictation", ledger row click).
pub fn copy_text(text: &str) {
    match Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(e) = clipboard.set_text(text) {
                log::error!("clipboard write failed: {e}");
            }
        }
        Err(e) => log::error!("clipboard unavailable: {e}"),
    }
}

/// ⌘V on macOS, Ctrl+V elsewhere, as an explicit down/up sequence so the
/// modifier is held while V is pressed and released.
fn send_paste_shortcut() {
    let settings = Settings {
        // The app drives the Accessibility prompt itself (onboarding /
        // status); enigo must not pop a second system dialog mid-paste.
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = match Enigo::new(&settings) {
        Ok(e) => e,
        Err(e) => {
            log::error!("cannot synthesize paste keystroke: {e}");
            return;
        }
    };
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    let sequence = [
        (modifier, Direction::Press),
        (Key::Unicode('v'), Direction::Press),
        (Key::Unicode('v'), Direction::Release),
        (modifier, Direction::Release),
    ];
    for (key, direction) in sequence {
        if let Err(e) = enigo.key(key, direction) {
            log::error!("paste keystroke failed at {key:?} {direction:?}: {e}");
            // `release_keys_when_dropped` (default true) lets go of any
            // modifier still held when `enigo` drops here.
            return;
        }
    }
}
