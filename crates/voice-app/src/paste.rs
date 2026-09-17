//! Paste into the frontmost app. Reference: `pasteText` in
//! Sources/VoiceCore/core.swift.
//!
//! Puts `text` on the clipboard, synthesizes ⌘V (macOS) / Ctrl+V (Windows,
//! Linux), then restores the previous clipboard text 600 ms later — long
//! enough for the target app to have consumed the paste.

#![allow(dead_code)]

pub fn paste_text(text: &str) {
    let _ = text;
    todo!()
}

/// Plain clipboard write (menu "Copy Last Dictation", ledger row click).
pub fn copy_text(text: &str) {
    let _ = text;
    todo!()
}
