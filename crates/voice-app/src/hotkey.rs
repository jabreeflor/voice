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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rdev::{EventType, Key};
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

/// How long `start` waits for the hook thread to report a failure before
/// assuming the hook is up. rdev's grab/listen only return on failure (the
/// success path blocks in the OS run loop), so "no answer yet" is the success
/// signal. A failure that takes longer than this is still caught: the receiver
/// is kept and polled by `tap_running`, which flips back to false.
const START_GRACE: Duration = Duration::from_millis(300);

/// Mutable part of the hook: whether the hotkey is currently held. Kept
/// separate from the controller so the event rules can be unit-tested without
/// installing a real hook.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookState {
    pub held: bool,
}

/// Applies one raw key event to the state machine and returns the event to
/// deliver (if any) and whether the OS event must be swallowed.
///
/// Rules mirror the Swift tap callback:
/// - hotkey press while not held → `Down`; further presses while held are OS
///   key repeats and are ignored;
/// - hotkey release while held → `Up`;
/// - while a recording is active, any other key press → `Cancel`; Escape is
///   swallowed, everything else is passed through (it is a shortcut the user
///   is typing with the modifier). `held` is deliberately left set so the
///   eventual hotkey release still produces `Up` (a no-op for an idle app).
/// - key releases of other keys and mouse events are ignored.
pub fn handle(
    state: &mut HookState,
    event: &EventType,
    hotkey: Hotkey,
    active: bool,
) -> (Option<HotkeyEvent>, bool) {
    let hk = HotkeyController::rdev_key(hotkey);
    match event {
        EventType::KeyPress(key) if *key == hk => {
            if state.held {
                (None, false)
            } else {
                state.held = true;
                (Some(HotkeyEvent::Down), false)
            }
        }
        EventType::KeyRelease(key) if *key == hk => {
            if state.held {
                state.held = false;
                (Some(HotkeyEvent::Up), false)
            } else {
                (None, false)
            }
        }
        EventType::KeyPress(Key::Escape) if active => (Some(HotkeyEvent::Cancel), true),
        EventType::KeyPress(_) if active => (Some(HotkeyEvent::Cancel), false),
        _ => (None, false),
    }
}

struct Inner {
    current_hotkey: HotkeyFn,
    is_active: ActiveFn,
    on_event: Mutex<Option<EventFn>>,
    state: Mutex<HookState>,
    running: AtomicBool,
    /// Report channel of the hook thread that `start` considered successful.
    /// A message (or disconnect) on it means the hook has died.
    watch: Mutex<Option<Receiver<Result<(), String>>>>,
}

impl Inner {
    /// Hook-thread entry for one raw event. Returns whether the OS event
    /// should be swallowed. Never blocks: the mutexes are only held by this
    /// thread for a few instructions and the callback is documented as
    /// non-blocking.
    fn on_raw(&self, event: &EventType) -> bool {
        let hotkey = (self.current_hotkey)();
        let active = (self.is_active)();
        let (out, swallow) = match self.state.lock() {
            Ok(mut state) => handle(&mut state, event, hotkey, active),
            Err(_) => (None, false),
        };
        if let Some(ev) = out {
            let cb = self.on_event.lock().ok().and_then(|g| g.clone());
            if let Some(cb) = cb {
                cb(ev);
            }
        }
        swallow
    }
}

pub struct HotkeyController {
    inner: Arc<Inner>,
}

impl HotkeyController {
    /// `current_hotkey` is consulted on every event so changing the setting
    /// takes effect without restarting the hook. `is_active` reports whether a
    /// recording is in progress (drives the cancel-on-other-key rule).
    pub fn new(current_hotkey: HotkeyFn, is_active: ActiveFn) -> HotkeyController {
        HotkeyController {
            inner: Arc::new(Inner {
                current_hotkey,
                is_active,
                on_event: Mutex::new(None),
                state: Mutex::new(HookState::default()),
                running: AtomicBool::new(false),
                watch: Mutex::new(None),
            }),
        }
    }

    pub fn set_on_event(&self, f: EventFn) {
        if let Ok(mut g) = self.inner.on_event.lock() {
            *g = Some(f);
        }
    }

    /// Starts the hook thread. Returns true when the hook is (or already was)
    /// running; false when the OS refused (macOS without Accessibility, Linux
    /// without X11). Safe to call repeatedly — the app retries once a second.
    ///
    /// rdev's grab/listen block the calling thread inside the OS run loop for
    /// as long as the hook lives, and only return (with `Err`) when the hook
    /// could not be installed. So the thread reports its result over a channel
    /// and we wait `START_GRACE` for it: an early `Err` means failure, silence
    /// means the hook is up. The race we accept is a failure slower than the
    /// grace period being reported as success for a moment; `tap_running`
    /// keeps polling the channel and corrects that on the next status refresh,
    /// and the app's retry timer then calls `start` again.
    pub fn start(&self) -> bool {
        if self.tap_running() {
            return true;
        }
        if let Err(reason) = crate::platform::global_hotkeys_supported() {
            log::warn!("hotkey hook unavailable: {reason}");
            return false;
        }

        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let inner = Arc::clone(&self.inner);
        // A dedicated thread: on macOS rdev adds the tap to *this* thread's
        // run loop and calls CFRunLoopRun, on Windows it pumps GetMessage, on
        // Linux it sits in XRecordEnableContext. None of that may touch the
        // Tauri main thread.
        thread::Builder::new()
            .name("voice-hotkey-hook".into())
            .spawn(move || {
                let result = run_hook(inner);
                // The receiver may be gone if the controller was dropped.
                let _ = tx.send(result);
            })
            .ok();

        match rx.recv_timeout(START_GRACE) {
            Err(RecvTimeoutError::Timeout) => {
                self.inner.running.store(true, Ordering::SeqCst);
                if let Ok(mut w) = self.inner.watch.lock() {
                    *w = Some(rx);
                }
                true
            }
            Ok(Err(reason)) => {
                log::warn!("hotkey hook failed to start: {reason}");
                false
            }
            // The hook returned Ok, i.e. its run loop ended straight away, or
            // the thread panicked before reporting. Either way nothing is
            // listening.
            Ok(Ok(())) | Err(RecvTimeoutError::Disconnected) => false,
        }
    }

    pub fn tap_running(&self) -> bool {
        if !self.inner.running.load(Ordering::SeqCst) {
            return false;
        }
        let died = match self.inner.watch.lock() {
            Ok(mut w) => match w.as_ref().map(Receiver::try_recv) {
                Some(Err(TryRecvError::Empty)) => false,
                Some(Ok(Err(reason))) => {
                    log::warn!("hotkey hook stopped: {reason}");
                    *w = None;
                    true
                }
                Some(Ok(Ok(()))) | Some(Err(TryRecvError::Disconnected)) => {
                    *w = None;
                    true
                }
                None => true,
            },
            Err(_) => false,
        };
        if died {
            self.inner.running.store(false, Ordering::SeqCst);
            if let Ok(mut s) = self.inner.state.lock() {
                *s = HookState::default();
            }
        }
        !died
    }

    /// Maps a hotkey to the rdev key it is delivered as.
    pub fn rdev_key(hotkey: Hotkey) -> rdev::Key {
        match hotkey {
            Hotkey::RightOption => Key::AltGr,
            Hotkey::RightCommand => Key::MetaRight,
            Hotkey::Fn => Key::Function,
            Hotkey::RightControl => Key::ControlRight,
        }
    }
}

/// Blocks for the lifetime of the hook; returns only when it could not be
/// installed (or, in theory, when the OS run loop ends).
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_hook(inner: Arc<Inner>) -> Result<(), String> {
    // Returning None from the grab callback drops the event (macOS: the tap
    // rewrites it to a Null event; Windows: the hook returns 1).
    rdev::grab(move |event: rdev::Event| {
        if inner.on_raw(&event.event_type) {
            None
        } else {
            Some(event)
        }
    })
    .map_err(|e| format!("{e:?}"))
}

/// Linux uses XRecord, which can observe but not swallow events, so Escape
/// still reaches the focused app while it cancels the dictation.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_hook(inner: Arc<Inner>) -> Result<(), String> {
    rdev::listen(move |event: rdev::Event| {
        inner.on_raw(&event.event_type);
    })
    .map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key) -> EventType {
        EventType::KeyPress(key)
    }
    fn release(key: Key) -> EventType {
        EventType::KeyRelease(key)
    }

    #[test]
    fn rdev_key_mapping_is_distinct_and_matches_spec() {
        assert_eq!(HotkeyController::rdev_key(Hotkey::RightOption), Key::AltGr);
        assert_eq!(
            HotkeyController::rdev_key(Hotkey::RightCommand),
            Key::MetaRight
        );
        assert_eq!(HotkeyController::rdev_key(Hotkey::Fn), Key::Function);
        assert_eq!(
            HotkeyController::rdev_key(Hotkey::RightControl),
            Key::ControlRight
        );
        let keys: Vec<Key> = Hotkey::ALL
            .iter()
            .map(|h| HotkeyController::rdev_key(*h))
            .collect();
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn press_then_release_yields_down_then_up() {
        let mut s = HookState::default();
        let hk = Hotkey::RightOption;
        assert_eq!(
            handle(&mut s, &press(Key::AltGr), hk, false),
            (Some(HotkeyEvent::Down), false)
        );
        assert!(s.held);
        assert_eq!(
            handle(&mut s, &release(Key::AltGr), hk, true),
            (Some(HotkeyEvent::Up), false)
        );
        assert!(!s.held);
    }

    // OS key repeat delivers KeyPress again while the key is held; a second
    // Down would restart the recording.
    #[test]
    fn key_repeats_are_ignored() {
        let mut s = HookState::default();
        let hk = Hotkey::RightCommand;
        assert_eq!(
            handle(&mut s, &press(Key::MetaRight), hk, false).0,
            Some(HotkeyEvent::Down)
        );
        assert_eq!(
            handle(&mut s, &press(Key::MetaRight), hk, true),
            (None, false)
        );
        assert_eq!(
            handle(&mut s, &press(Key::MetaRight), hk, true),
            (None, false)
        );
        assert_eq!(
            handle(&mut s, &release(Key::MetaRight), hk, true).0,
            Some(HotkeyEvent::Up)
        );
    }

    #[test]
    fn release_without_press_is_ignored() {
        let mut s = HookState::default();
        assert_eq!(
            handle(&mut s, &release(Key::AltGr), Hotkey::RightOption, false),
            (None, false)
        );
    }

    // Esc cancels dictation and must not reach the frontmost app.
    #[test]
    fn escape_while_active_cancels_and_is_swallowed() {
        let mut s = HookState { held: true };
        assert_eq!(
            handle(&mut s, &press(Key::Escape), Hotkey::RightOption, true),
            (Some(HotkeyEvent::Cancel), true)
        );
        // Still held: the later hotkey release yields Up.
        assert!(s.held);
        assert_eq!(
            handle(&mut s, &release(Key::AltGr), Hotkey::RightOption, false).0,
            Some(HotkeyEvent::Up)
        );
    }

    // Any other key while holding the hotkey means the user is typing a
    // shortcut: cancel, but let the shortcut through.
    #[test]
    fn other_key_while_active_cancels_and_passes_through() {
        let mut s = HookState { held: true };
        assert_eq!(
            handle(&mut s, &press(Key::KeyC), Hotkey::RightOption, true),
            (Some(HotkeyEvent::Cancel), false)
        );
    }

    #[test]
    fn keys_while_idle_are_passed_through_untouched() {
        let mut s = HookState::default();
        assert_eq!(
            handle(&mut s, &press(Key::Escape), Hotkey::RightOption, false),
            (None, false)
        );
        assert_eq!(
            handle(&mut s, &press(Key::KeyA), Hotkey::RightOption, false),
            (None, false)
        );
        assert_eq!(
            handle(&mut s, &release(Key::KeyA), Hotkey::RightOption, true),
            (None, false)
        );
        assert_eq!(
            handle(
                &mut s,
                &EventType::ButtonPress(rdev::Button::Left),
                Hotkey::RightOption,
                true
            ),
            (None, false)
        );
    }

    // The hotkey closure is read per event, so a settings change mid-hold must
    // not leave the old key "stuck": the new key is what counts from now on.
    #[test]
    fn hotkey_change_takes_effect_per_event() {
        let mut s = HookState::default();
        assert_eq!(
            handle(&mut s, &press(Key::AltGr), Hotkey::RightOption, false).0,
            Some(HotkeyEvent::Down)
        );
        assert_eq!(
            handle(
                &mut s,
                &press(Key::ControlRight),
                Hotkey::RightControl,
                false
            ),
            (None, false)
        );
        assert_eq!(
            handle(
                &mut s,
                &release(Key::ControlRight),
                Hotkey::RightControl,
                true
            )
            .0,
            Some(HotkeyEvent::Up)
        );
    }

    #[test]
    fn controller_reports_not_running_before_start() {
        let c = HotkeyController::new(Arc::new(|| Hotkey::RightOption), Arc::new(|| false));
        assert!(!c.tap_running());
    }
}
