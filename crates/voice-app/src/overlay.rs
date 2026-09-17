//! Floating pill overlay (Wisprflow-style). Reference: `BarsView` / `Overlay`
//! in Sources/VoiceCore/core.swift.
//!
//! The pill is the `overlay` Tauri window (192×44, transparent, always on top,
//! non-focusable, ignores the cursor). Rust only positions/shows/hides it and
//! emits events; ui/overlay.js draws the bars and text:
//!
//! - event `overlay`  payload `{ "mode": "listening" | "processing" | "flash" | "hidden",
//!                              "message": String, "duration_ms": u64 }`
//! - event `level`    payload `{ "value": f32 }` at ~30 Hz while listening
//! - event `sound`    payload `{ "name": "start" | "done" }` (played by the overlay webview)

#![allow(dead_code)]

use std::time::Duration;

use tauri::AppHandle;

pub struct Overlay {
    _private: (),
}

impl Overlay {
    pub fn new(handle: AppHandle) -> Overlay {
        let _ = handle;
        todo!()
    }

    /// Shows the bars driven by `level_provider` (polled ~30 Hz on a thread).
    pub fn show_listening(&self, level_provider: Box<dyn Fn() -> f32 + Send + Sync>) {
        let _ = level_provider;
        todo!()
    }

    pub fn show_processing(&self) {
        todo!()
    }

    /// Shows `message` then hides after `duration` (default 1.1 s).
    pub fn flash(&self, message: &str, duration: Duration) {
        let _ = (message, duration);
        todo!()
    }

    pub fn hide(&self) {
        todo!()
    }

    pub fn play_sound(&self, name: &str) {
        let _ = name;
        todo!()
    }
}
