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

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Swift restores after 0.6 s; shorter and slow apps paste the old contents.
const RESTORE_DELAY: Duration = Duration::from_millis(600);

/// One process-lifetime clipboard handle, the way arboard is meant to be
/// used. On X11 the selection is served by a window owned by the handle:
/// dropping a per-call handle right after `set_text` tears that window down
/// (after a ≤100 ms handover that only works when a clipboard manager is
/// running), so the target app can find nothing to paste. macOS and Windows
/// copy the data into the OS on write, so a long-lived handle costs nothing
/// there. `None` until the first successful `Clipboard::new`, and again after
/// a failure, so a transient error (no display yet) is retried next time.
static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

/// Runs `f` on the shared handle, creating it on first use. `None` when the
/// clipboard cannot be opened at all (already logged).
fn with_clipboard<R>(f: impl FnOnce(&mut Clipboard) -> R) -> Option<R> {
    let mut guard = match CLIPBOARD.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        match Clipboard::new() {
            Ok(c) => *guard = Some(c),
            Err(e) => {
                log::error!("clipboard unavailable: {e}");
                return None;
            }
        }
    }
    guard.as_mut().map(f)
}

pub fn paste_text(text: &str) {
    // Err means "nothing textual on the clipboard" (empty, image, ...);
    // Swift's `string(forType:)` returns nil there and skips the restore.
    let written = with_clipboard(|clipboard| {
        let saved = clipboard.get_text().ok();
        clipboard.set_text(text).map(|()| saved)
    });
    let saved = match written {
        Some(Ok(saved)) => saved,
        Some(Err(e)) => {
            log::error!("clipboard write failed: {e}");
            return;
        }
        None => return,
    };
    send_paste_shortcut();

    // The restore runs on the shared handle, which stays alive for the whole
    // process, so the pasted text is still being served while the target app
    // reads it — whether or not there is anything to restore afterwards.
    if let Some(saved) = saved {
        thread::spawn(move || {
            thread::sleep(RESTORE_DELAY);
            if let Some(Err(e)) = with_clipboard(|clipboard| clipboard.set_text(saved)) {
                log::warn!("clipboard restore failed: {e}");
            }
        });
    }
}

/// Plain clipboard write (menu "Copy Last Dictation", ledger row click).
/// Uses the shared handle so the text keeps being served on X11 and the
/// caller (a Tauri IPC thread) never blocks on arboard's drop-time handover.
pub fn copy_text(text: &str) {
    if let Some(Err(e)) = with_clipboard(|clipboard| clipboard.set_text(text)) {
        log::error!("clipboard write failed: {e}");
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
