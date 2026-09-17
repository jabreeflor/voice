//! Hold-to-talk key hook. Reference: `HotkeyController` in
//! Sources/VoiceCore/core.swift (a CGEvent tap).
//!
//! Semantics: hotkey pressed → `Down`; released → `Up`. While a recording is
//! active, Escape cancels and is swallowed; any other non-modifier key cancels
//! and is passed through so Option/Alt-based shortcuts keep working. Key
//! repeats are ignored.
//!
//! Backends:
//! - macOS: a CGEvent tap installed directly through `core-graphics`, the
//!   same shape as the Swift tap (session tap, `flagsChanged | keyDown` mask,
//!   re-armed on `tapDisabledByTimeout` / `tapDisabledByUserInput`). rdev's
//!   grab is not used here: it subscribes to every event type, cannot
//!   re-enable a tap the WindowServer switched off, and folds modifier
//!   changes into `KeyPress`, which would cancel dictation on Shift.
//! - Windows: `rdev::grab` (low-level keyboard + mouse hooks).
//! - Linux/X11: `rdev::listen` (XRecord; cannot swallow Escape; Wayland
//!   unsupported).
//!
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
/// assuming the hook is up. The hook entry points only return on failure (the
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

/// Modifier keys as rdev reports them. The Swift tap only cancelled on
/// `.keyDown`; modifiers arrive as `.flagsChanged` there and never cancel.
/// rdev (Windows/Linux) reports both as `KeyPress`, so the distinction has
/// to be made here or pressing Shift while talking would kill the dictation.
fn is_modifier(key: &Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
            | Key::Function
            | Key::CapsLock
    )
}

/// Applies one raw key event to the state machine and returns the event to
/// deliver (if any) and whether the OS event must be swallowed.
///
/// Rules mirror the Swift tap callback:
/// - hotkey press while not held → `Down`; further presses while held are OS
///   key repeats and are ignored;
/// - hotkey release while held → `Up`;
/// - while a recording is active, any other non-modifier key press →
///   `Cancel`; Escape is swallowed, everything else is passed through (it is
///   a shortcut the user is typing with the modifier). `held` is deliberately
///   left set so the eventual hotkey release still produces `Up` (a no-op for
///   an idle app).
/// - other modifiers, key releases of other keys and mouse events are ignored.
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
        EventType::KeyPress(key) if active && !is_modifier(key) => {
            (Some(HotkeyEvent::Cancel), false)
        }
        _ => (None, false),
    }
}

/// The two CGEvent kinds the macOS tap subscribes to, reduced to plain values
/// so the conversion rule can be unit-tested on any host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapEvent {
    /// `flagsChanged`: `keycode` is the modifier key that changed, `pressed`
    /// whether the *hotkey's* modifier bit is set in the event flags (the
    /// Swift tap reads `event.flags.contains(hk.flag)`).
    FlagsChanged { keycode: i64, pressed: bool },
    /// `keyDown` of a non-modifier key.
    KeyDown { keycode: i64 },
}

/// Virtual keycode of Escape on macOS.
const MAC_ESCAPE: i64 = 53;

/// Maps a macOS tap event onto the rdev vocabulary `handle` speaks, so both
/// backends share one state machine. Modifier changes other than the hotkey's
/// own key are dropped here, exactly as the Swift tap ignored them.
pub fn tap_to_event(ev: TapEvent, hotkey: Hotkey) -> Option<EventType> {
    match ev {
        TapEvent::FlagsChanged { keycode, pressed } if keycode == hotkey.key_code() => {
            let key = HotkeyController::rdev_key(hotkey);
            Some(if pressed {
                EventType::KeyPress(key)
            } else {
                EventType::KeyRelease(key)
            })
        }
        TapEvent::FlagsChanged { .. } => None,
        TapEvent::KeyDown { keycode } if keycode == MAC_ESCAPE => {
            Some(EventType::KeyPress(Key::Escape))
        }
        // Any other keyDown only matters as "some other key": the exact key
        // is irrelevant to `handle`, and `Unknown` can never collide with a
        // hotkey or a modifier.
        TapEvent::KeyDown { keycode } => Some(EventType::KeyPress(Key::Unknown(
            u32::try_from(keycode).unwrap_or(u32::MAX),
        ))),
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
    /// Serialises `start`: the running check and the thread spawn are not
    /// atomic on their own, and two hooks at once would fight over the OS
    /// hook slot (and the second one's report channel would never be polled).
    start_lock: Mutex<()>,
    /// Last failure reason that was logged. The app retries `start` once a
    /// second, so without this a Wayland session or a Mac without
    /// Accessibility would emit one warning per second for the whole run.
    last_failure: Mutex<Option<String>>,
}

impl Inner {
    /// Runs the state machine for one event and delivers the outcome. Returns
    /// whether the OS event should be swallowed. Never blocks: the mutexes
    /// are only held by the hook thread for a few instructions and the
    /// callback is documented as non-blocking.
    fn dispatch(&self, event: &EventType, hotkey: Hotkey) -> bool {
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

    /// Hook-thread entry for one rdev event (Windows/Linux).
    ///
    /// rdev's hooks also report pointer motion, scrolling and drags (its
    /// Windows grab installs a `WH_MOUSE_LL` hook next to the keyboard one),
    /// and a slow callback there is what trips the OS hook timeout. Nothing
    /// below `handle` cares about non-key events, so bail before touching
    /// the settings lock or the app's `is_active` callback.
    fn on_raw(&self, event: &EventType) -> bool {
        if !matches!(event, EventType::KeyPress(_) | EventType::KeyRelease(_)) {
            return false;
        }
        let hotkey = (self.current_hotkey)();
        self.dispatch(event, hotkey)
    }

    /// Hook-thread entry for one CGEvent tap event (macOS). The hotkey is
    /// passed in because the caller already needed it to read the flag bit.
    fn on_tap(&self, ev: TapEvent, hotkey: Hotkey) -> bool {
        match tap_to_event(ev, hotkey) {
            Some(event) => self.dispatch(&event, hotkey),
            None => false,
        }
    }

    /// Logs `reason` unless it is the same failure as last time.
    fn warn_once(&self, what: &str, reason: &str) {
        let Ok(mut last) = self.last_failure.lock() else {
            return;
        };
        if last.as_deref() != Some(reason) {
            log::warn!("{what}: {reason}");
            *last = Some(reason.to_string());
        }
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
                start_lock: Mutex::new(()),
                last_failure: Mutex::new(None),
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
    /// without X11). Safe to call repeatedly and concurrently — the app
    /// retries once a second, and a second caller waits for the first and
    /// then sees the hook it installed.
    ///
    /// The hook entry points block the calling thread inside the OS run loop
    /// for as long as the hook lives, and only return (with `Err`) when the
    /// hook could not be installed. So the thread reports its result over a
    /// channel and we wait `START_GRACE` for it: an early `Err` means failure,
    /// silence means the hook is up. The race we accept is a failure slower
    /// than the grace period being reported as success for a moment;
    /// `tap_running` keeps polling the channel and corrects that on the next
    /// status refresh, and the app's retry timer then calls `start` again.
    pub fn start(&self) -> bool {
        let _serial = self.inner.start_lock.lock();
        if self.tap_running() {
            return true;
        }
        if let Err(reason) = crate::platform::global_hotkeys_supported() {
            self.inner.warn_once("hotkey hook unavailable", &reason);
            return false;
        }

        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let inner = Arc::clone(&self.inner);
        // A dedicated thread: on macOS the tap is added to *this* thread's
        // run loop and CFRunLoopRun blocks, on Windows rdev pumps GetMessage,
        // on Linux it sits in XRecordEnableContext. None of that may touch
        // the Tauri main thread.
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
                if let Ok(mut last) = self.inner.last_failure.lock() {
                    *last = None;
                }
                true
            }
            Ok(Err(reason)) => {
                self.inner.warn_once("hotkey hook failed to start", &reason);
                false
            }
            // The hook returned Ok, i.e. its run loop ended straight away, or
            // the thread panicked before reporting. Either way nothing is
            // listening.
            Ok(Ok(())) | Err(RecvTimeoutError::Disconnected) => {
                self.inner
                    .warn_once("hotkey hook failed to start", "hook thread exited");
                false
            }
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
    ///
    /// rdev's Windows and Linux keycode tables only name the *left* Win/Super
    /// key (`MetaLeft` = VK_LWIN 91 / X11 Super_L 133); the right-hand key
    /// falls through to `Key::Unknown(<raw code>)`, so `MetaRight` would never
    /// match there and "Right Win"/"Right Super" would be dead.
    pub fn rdev_key(hotkey: Hotkey) -> rdev::Key {
        match hotkey {
            Hotkey::RightOption => Key::AltGr,
            #[cfg(target_os = "macos")]
            Hotkey::RightCommand => Key::MetaRight,
            // VK_RWIN.
            #[cfg(target_os = "windows")]
            Hotkey::RightCommand => Key::Unknown(92),
            // X11 Super_R.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            Hotkey::RightCommand => Key::Unknown(134),
            Hotkey::Fn => Key::Function,
            Hotkey::RightControl => Key::ControlRight,
        }
    }
}

/// The CGEventFlags bit the hotkey toggles, i.e. `Hotkey.flag` in Swift.
#[cfg(target_os = "macos")]
fn hotkey_flag(hotkey: Hotkey) -> core_graphics::event::CGEventFlags {
    use core_graphics::event::CGEventFlags;
    match hotkey {
        Hotkey::RightOption => CGEventFlags::CGEventFlagAlternate,
        Hotkey::RightCommand => CGEventFlags::CGEventFlagCommand,
        Hotkey::Fn => CGEventFlags::CGEventFlagSecondaryFn,
        Hotkey::RightControl => CGEventFlags::CGEventFlagControl,
    }
}

/// Blocks for the lifetime of the tap; returns `Err` when the tap could not
/// be created (no Accessibility grant) and `Ok` only if the run loop ends.
///
/// Mirrors `HotkeyController.startTap` / `handle` in core.swift: session
/// tap, head insert, `flagsChanged | keyDown` only, and re-enable on the two
/// tap-disabled notifications — the WindowServer switches a tap off when its
/// callback is slow or on user input, and a tap left that way is dead for
/// the rest of the process while nothing else notices.
#[cfg(target_os = "macos")]
fn run_hook(inner: Arc<Inner>) -> Result<(), String> {
    use std::ffi::c_void;
    use std::sync::atomic::AtomicPtr;

    use core_foundation::base::TCFType;
    use core_foundation::mach_port::CFMachPortRef;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        CallbackResult, EventField,
    };

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    // The callback must re-enable the tap it belongs to, but the tap (and
    // its mach port) only exist once the callback has been handed over, so
    // the port is filled in afterwards. The callback only runs from this
    // thread's run loop, after `store` below, so a relaxed-free atomic is
    // plenty.
    let port: Arc<AtomicPtr<c_void>> = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
    let port_in_cb = Arc::clone(&port);

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, kind, event| {
            match kind {
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    let p = port_in_cb.load(Ordering::SeqCst);
                    if !p.is_null() {
                        // SAFETY: `p` is the tap's own CFMachPort, which
                        // outlives the callback (the tap is dropped only after
                        // the run loop returns on this same thread).
                        unsafe { CGEventTapEnable(p.cast(), true) };
                    }
                    return CallbackResult::Keep;
                }
                CGEventType::FlagsChanged | CGEventType::KeyDown => {}
                _ => return CallbackResult::Keep,
            }
            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            let hotkey = (inner.current_hotkey)();
            let ev = match kind {
                CGEventType::FlagsChanged => TapEvent::FlagsChanged {
                    keycode,
                    pressed: event.get_flags().contains(hotkey_flag(hotkey)),
                },
                _ => TapEvent::KeyDown { keycode },
            };
            // Returning Drop hands a null event back to the WindowServer,
            // which is how a tap swallows an event (Swift returned nil).
            if inner.on_tap(ev, hotkey) {
                CallbackResult::Drop
            } else {
                CallbackResult::Keep
            }
        },
    )
    .map_err(|()| "CGEventTapCreate failed (Accessibility not granted?)".to_string())?;

    port.store(
        tap.mach_port().as_concrete_TypeRef().cast(),
        Ordering::SeqCst,
    );
    let source = tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|()| "CFMachPortCreateRunLoopSource failed".to_string())?;
    // SAFETY: reading a framework-exported constant.
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    CFRunLoop::run_current();
    // Keep the tap (and thus the callback) alive until the run loop is done.
    drop(tap);
    Ok(())
}

/// Blocks for the lifetime of the hook; returns only when it could not be
/// installed (or, in theory, when the message loop ends).
#[cfg(target_os = "windows")]
fn run_hook(inner: Arc<Inner>) -> Result<(), String> {
    // Returning None from the grab callback drops the event (the hook
    // returns 1 instead of calling CallNextHookEx).
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
    fn cmd() -> Key {
        HotkeyController::rdev_key(Hotkey::RightCommand)
    }

    #[test]
    fn rdev_key_mapping_is_distinct_and_matches_spec() {
        assert_eq!(HotkeyController::rdev_key(Hotkey::RightOption), Key::AltGr);
        #[cfg(target_os = "macos")]
        assert_eq!(
            HotkeyController::rdev_key(Hotkey::RightCommand),
            Key::MetaRight
        );
        // rdev has no MetaRight off macOS; the raw right-Win/Super code is
        // what its tables fall through to.
        #[cfg(target_os = "windows")]
        assert_eq!(
            HotkeyController::rdev_key(Hotkey::RightCommand),
            Key::Unknown(92)
        );
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(
            HotkeyController::rdev_key(Hotkey::RightCommand),
            Key::Unknown(134)
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
            handle(&mut s, &press(cmd()), hk, false).0,
            Some(HotkeyEvent::Down)
        );
        assert_eq!(handle(&mut s, &press(cmd()), hk, true), (None, false));
        assert_eq!(handle(&mut s, &press(cmd()), hk, true), (None, false));
        assert_eq!(
            handle(&mut s, &release(cmd()), hk, true).0,
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

    // The Swift tap only cancelled on keyDown; modifiers arrive as
    // flagsChanged there and never cancel. Pressing Shift (or Caps Lock, or
    // the other Option) mid-sentence must not kill the dictation.
    #[test]
    fn modifier_press_while_active_does_not_cancel() {
        let mut s = HookState { held: true };
        for key in [
            Key::ShiftLeft,
            Key::ShiftRight,
            Key::ControlLeft,
            Key::Alt,
            Key::MetaLeft,
            Key::CapsLock,
        ] {
            assert_eq!(
                handle(&mut s, &press(key), Hotkey::RightOption, true),
                (None, false),
                "{key:?}"
            );
        }
        assert!(s.held);
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

    // macOS tap conversion: the hotkey's own flagsChanged becomes a press or
    // release by flag bit (keycode 61 = Right Option), other modifiers are
    // dropped, Esc (53) is Escape, any other keyDown is "some other key".
    #[test]
    fn tap_events_map_onto_handle_vocabulary() {
        let hk = Hotkey::RightOption;
        assert_eq!(
            tap_to_event(
                TapEvent::FlagsChanged {
                    keycode: 61,
                    pressed: true
                },
                hk
            ),
            Some(press(Key::AltGr))
        );
        assert_eq!(
            tap_to_event(
                TapEvent::FlagsChanged {
                    keycode: 61,
                    pressed: false
                },
                hk
            ),
            Some(release(Key::AltGr))
        );
        // Left Shift (56) with the Option bit still set mid-hold: not ours.
        assert_eq!(
            tap_to_event(
                TapEvent::FlagsChanged {
                    keycode: 56,
                    pressed: true
                },
                hk
            ),
            None
        );
        assert_eq!(
            tap_to_event(TapEvent::KeyDown { keycode: 53 }, hk),
            Some(press(Key::Escape))
        );
        let other = tap_to_event(TapEvent::KeyDown { keycode: 8 }, hk).unwrap();
        assert!(matches!(other, EventType::KeyPress(Key::Unknown(8))));
        let mut s = HookState { held: true };
        assert_eq!(
            handle(&mut s, &other, hk, true),
            (Some(HotkeyEvent::Cancel), false)
        );
    }

    #[test]
    fn controller_reports_not_running_before_start() {
        let c = HotkeyController::new(Arc::new(|| Hotkey::RightOption), Arc::new(|| false));
        assert!(!c.tap_running());
    }
}
