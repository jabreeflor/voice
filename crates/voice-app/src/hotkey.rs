//! Hold-to-talk key hook. Reference: `HotkeyController` in
//! Sources/VoiceCore/core.swift (a CGEvent tap).
//!
//! Semantics: hotkey pressed → `Down`; released → `Up`. While a recording is
//! active, Escape cancels and is swallowed; any other key cancels and is passed
//! through so Option/Alt-based shortcuts keep working. Key repeats are ignored.
//!
//! Backend: `rdev::grab` on macOS and Windows (needs Accessibility on macOS),
//! `rdev::listen` on Linux/X11 (cannot swallow Escape; Wayland unsupported).
//! The hook runs on its own thread; events are delivered through the callback
//! from that thread — the receiver must be `Send + Sync` and must not block.

#![allow(dead_code)]

use std::sync::Arc;

use voice_core::Hotkey;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotkeyEvent {
    Down,
    Up,
    Cancel,
}

pub type EventFn = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;
pub type ActiveFn = Arc<dyn Fn() -> bool + Send + Sync>;
pub type HotkeyFn = Arc<dyn Fn() -> Hotkey + Send + Sync>;

pub struct HotkeyController {
    _private: (),
}

impl HotkeyController {
    /// `current_hotkey` is consulted on every event so changing the setting
    /// takes effect without restarting the hook. `is_active` reports whether a
    /// recording is in progress (drives the cancel-on-other-key rule).
    pub fn new(current_hotkey: HotkeyFn, is_active: ActiveFn) -> HotkeyController {
        let _ = (current_hotkey, is_active);
        todo!()
    }

    pub fn set_on_event(&self, f: EventFn) {
        let _ = f;
        todo!()
    }

    /// Starts the hook thread. Returns true when the hook is (or already was)
    /// running; false when the OS refused (macOS without Accessibility, Linux
    /// without X11). Safe to call repeatedly — the app retries once a second.
    pub fn start(&self) -> bool {
        todo!()
    }

    pub fn tap_running(&self) -> bool {
        todo!()
    }

    /// Maps a hotkey to the rdev key it is delivered as.
    pub fn rdev_key(hotkey: Hotkey) -> rdev::Key {
        let _ = hotkey;
        todo!()
    }
}
