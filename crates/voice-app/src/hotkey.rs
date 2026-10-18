//! Hold-to-talk key hook. Reference: `HotkeyController` in
//! Sources/VoiceCore/core.swift (a CGEvent tap).
//!
//! Semantics: hotkey pressed → `Down`; released → `Up`. While a recording is
//! active, Escape cancels and is swallowed; any other non-modifier key cancels
//! and is passed through so Option/Alt-based shortcuts keep working. Key
//! repeats are ignored. A talk key that is not a modifier (an F-key, a letter,
//! Space, …) is swallowed on press, repeats and release, so holding it does
//! not also type into the frontmost app; modifier talk keys are never
//! swallowed, as their keystrokes are only flag changes.
//!
//! Backends (this deliberately supersedes the SPEC.md bullet that asks for
//! `rdev::grab` on macOS; the spec's key mapping and event rules are kept):
//! - macOS: a CGEvent tap installed directly through `core-graphics` (a
//!   macOS-only dependency of this crate), the same shape as the Swift tap:
//!   session tap, head insert, `flagsChanged | keyDown` mask, re-armed on
//!   `tapDisabledByTimeout` / `tapDisabledByUserInput`, on its own thread's
//!   run loop. rdev's grab is not used here: it subscribes to every event
//!   type (each mouse move through a Rust callback is what trips the
//!   WindowServer tap timeout), never re-enables a tap the WindowServer
//!   switched off, and folds modifier changes into `KeyPress`, which would
//!   cancel dictation on Shift.
//! - Windows: `rdev::grab` (low-level keyboard + mouse hooks).
//! - Linux/X11: `rdev::listen` (XRecord; cannot swallow Escape; Wayland
//!   unsupported).
//!
//! The hook runs on its own thread. Events are queued from there and
//! delivered through the callback on a separate dispatch thread, so the
//! callback may take its time (the OS hook callback itself never waits on
//! it) but must still be `Send + Sync`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
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
/// `.keyDown`; modifiers (and the lock keys, which are `flagsChanged` too)
/// arrive as `.flagsChanged` there and never cancel. rdev (Windows/Linux)
/// reports both as `KeyPress`, so the distinction has to be made here or
/// pressing Shift while talking would kill the dictation.
///
/// The right Win/Super key has no name in rdev's Windows/Linux tables and
/// arrives as `Unknown(<raw code>)` — the same value `rdev_key` returns for
/// `Hotkey::RightCommand` — so it is matched through that mapping rather
/// than a literal.
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
            | Key::NumLock
            | Key::ScrollLock
            | Key::Pause
    ) || *key == HotkeyController::rdev_key(Hotkey::RightCommand)
}

/// Applies one raw key event to the state machine and returns the event to
/// deliver (if any) and whether the OS event must be swallowed.
///
/// Rules mirror the Swift tap callback:
/// - hotkey press while not held → `Down`; further presses while held are OS
///   key repeats and are ignored;
/// - hotkey release while held → `Up`. A non-modifier talk key swallows its
///   press, repeats and release. A release for a press this hook never saw
///   (the key was already down when the hook started) passes through, since
///   the app received that press and must receive its release too;
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
    // Modifiers pass through; a key that types must not reach the app.
    let swallow_talk = !hotkey.is_modifier();
    match event {
        EventType::KeyPress(key) if *key == hk => {
            if state.held {
                (None, swallow_talk)
            } else {
                state.held = true;
                (Some(HotkeyEvent::Down), swallow_talk)
            }
        }
        EventType::KeyRelease(key) if *key == hk => {
            if state.held {
                state.held = false;
                (Some(HotkeyEvent::Up), swallow_talk)
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

/// The CGEvent kinds the macOS tap subscribes to, reduced to plain values
/// so the conversion rule can be unit-tested on any host.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapEvent {
    /// `flagsChanged`: `keycode` is the modifier key that changed, `pressed`
    /// whether the *hotkey's* modifier bit is set in the event flags (the
    /// Swift tap reads `event.flags.contains(hk.flag)`).
    FlagsChanged { keycode: i64, pressed: bool },
    /// `keyDown` of a non-modifier key.
    KeyDown { keycode: i64 },
    /// `keyUp` of a non-modifier key. Only a non-modifier talk key needs it.
    KeyUp { keycode: i64 },
}

/// Virtual keycode of Escape on macOS.
#[cfg(any(target_os = "macos", test))]
const MAC_ESCAPE: i64 = 53;

/// Maps a macOS tap event onto the rdev vocabulary `handle` speaks, so both
/// backends share one state machine. Modifier changes other than the hotkey's
/// own key are dropped here, exactly as the Swift tap ignored them. A
/// non-modifier talk key arrives as keyDown/keyUp of its own keycode; every
/// other key release is dropped, since only presses mean anything to `handle`.
#[cfg(any(target_os = "macos", test))]
pub fn tap_to_event(ev: TapEvent, hotkey: Hotkey) -> Option<EventType> {
    let talk_key = HotkeyController::rdev_key(hotkey);
    match ev {
        TapEvent::FlagsChanged { keycode, pressed }
            if hotkey.is_modifier() && keycode == hotkey.key_code() =>
        {
            Some(if pressed {
                EventType::KeyPress(talk_key)
            } else {
                EventType::KeyRelease(talk_key)
            })
        }
        TapEvent::FlagsChanged { .. } => None,
        TapEvent::KeyDown { keycode } if !hotkey.is_modifier() && keycode == hotkey.key_code() => {
            Some(EventType::KeyPress(talk_key))
        }
        TapEvent::KeyUp { keycode } if !hotkey.is_modifier() && keycode == hotkey.key_code() => {
            Some(EventType::KeyRelease(talk_key))
        }
        TapEvent::KeyUp { .. } => None,
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
    /// Queue from the hook thread to the dispatch thread. Unbounded, so a
    /// send never waits on the app. `Inner` itself is pinned for the process
    /// lifetime once a hook is up (the hook thread never returns), so this
    /// sender is not dropped with the controller by accident: `Drop` for
    /// `HotkeyController` takes it out explicitly, which closes the channel
    /// and ends the dispatch thread.
    queue: Mutex<Option<Sender<HotkeyEvent>>>,
    /// Set by `Drop`. mpsc drains what was queued before the sender closed,
    /// so closing `queue` alone would still hand the hook's last event(s) to
    /// the callback of a torn-down app; `deliver` checks this first.
    shutdown: AtomicBool,
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
    /// Runs the state machine for one event and queues the outcome for the
    /// dispatch thread. Returns whether the OS event should be swallowed.
    ///
    /// This runs inside the OS hook callback (the CGEvent tap callback on
    /// macOS, `WH_KEYBOARD_LL` on Windows), where the WindowServer / the
    /// system drops the hook after a few hundred milliseconds of silence —
    /// and the keystroke that tripped it with it. The Swift tap deferred
    /// everything with `DispatchQueue.main.async` for that reason; here the
    /// app callback is never invoked from this thread at all, only a channel
    /// send happens. The mutexes are only ever held for a few instructions.
    fn dispatch(&self, event: &EventType, hotkey: Hotkey) -> bool {
        let active = (self.is_active)();
        // `HookState` is a plain bool, so a poisoned lock still holds a
        // consistent value; giving up here would leave the hook silent for
        // the rest of the process while `tap_running` keeps reporting it up.
        let (out, swallow) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            handle(&mut state, event, hotkey, active)
        };
        if let Some(ev) = out {
            if let Ok(q) = self.queue.lock() {
                if let Some(tx) = q.as_ref() {
                    // A send only fails once the dispatch thread is gone;
                    // by then the controller has been dropped and nobody
                    // wants the event.
                    let _ = tx.send(ev);
                }
            }
        }
        swallow
    }

    /// Delivers one queued event to the app callback (dispatch thread only).
    fn deliver(&self, ev: HotkeyEvent) {
        if self.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let cb = self.on_event.lock().ok().and_then(|g| g.clone());
        if let Some(cb) = cb {
            cb(ev);
        }
    }

    /// The hotkey the hook should act on right now: the configured one,
    /// clamped to what this platform can deliver. `Config::hotkey` reads the
    /// raw settings value without consulting `Hotkey::available()`, so a
    /// `settings.json` carrying `"hotkey":"fn"` (hand-edited, or synced from
    /// a Mac) reaches Windows/Linux, where rdev's key tables have no
    /// `Function` entry and the hook would run fine while no keystroke ever
    /// matched. Fall back to the platform default (the same fallback an
    /// unknown raw value gets) and say so once in the log.
    fn hotkey(&self) -> Hotkey {
        let configured = (self.current_hotkey)();
        let available = Hotkey::available();
        if available.contains(&configured) {
            return configured;
        }
        let fallback = available[0];
        // A fixed message: this runs in the OS hook callback on every event
        // while the setting is wrong, so no formatting there.
        self.warn_once(
            "configured hotkey cannot be hooked on this platform",
            "using the platform default instead",
        );
        fallback
    }

    /// Hook-thread entry for one rdev event (Windows/Linux).
    ///
    /// rdev's hooks also report pointer motion, scrolling and drags (its
    /// Windows grab installs a `WH_MOUSE_LL` hook next to the keyboard one),
    /// and a slow callback there is what trips the OS hook timeout. Nothing
    /// below `handle` cares about non-key events, so bail before touching
    /// the settings lock or the app's `is_active` callback.
    #[cfg(not(target_os = "macos"))]
    fn on_raw(&self, event: &EventType) -> bool {
        if !matches!(event, EventType::KeyPress(_) | EventType::KeyRelease(_)) {
            return false;
        }
        let hotkey = self.hotkey();
        self.dispatch(event, hotkey)
    }

    /// Hook-thread entry for one CGEvent tap event (macOS). The hotkey is
    /// passed in because the caller already needed it to read the flag bit.
    #[cfg(target_os = "macos")]
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

/// The hook thread keeps a strong `Arc<Inner>` and never returns, so the
/// event channel inside `Inner` would otherwise stay open after the
/// controller is gone and the dispatch thread would keep invoking the stored
/// callback on an app that has been torn down. `shutdown` is raised first so
/// that nothing already sitting in the queue is delivered either (mpsc
/// drains buffered items before reporting the close); closing the sender
/// then ends the dispatch thread. The hook itself keeps running (it cannot
/// be stopped from outside the OS run loop) but its events now go nowhere.
impl Drop for HotkeyController {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        if let Ok(mut q) = self.inner.queue.lock() {
            *q = None;
        }
    }
}

impl HotkeyController {
    /// `current_hotkey` is consulted on every event so changing the setting
    /// takes effect without restarting the hook. `is_active` reports whether a
    /// recording is in progress (drives the cancel-on-other-key rule).
    pub fn new(current_hotkey: HotkeyFn, is_active: ActiveFn) -> HotkeyController {
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        let inner = Arc::new(Inner {
            current_hotkey,
            is_active,
            on_event: Mutex::new(None),
            queue: Mutex::new(Some(tx)),
            shutdown: AtomicBool::new(false),
            state: Mutex::new(HookState::default()),
            running: AtomicBool::new(false),
            watch: Mutex::new(None),
            start_lock: Mutex::new(()),
            last_failure: Mutex::new(None),
        });
        // The dispatch thread holds only a weak reference so that it never
        // keeps `Inner` alive on its own. It exits when the channel closes,
        // which `Drop` for `HotkeyController` does explicitly; the `Arc` is
        // otherwise pinned by the hook thread for the rest of the process.
        let weak: Weak<Inner> = Arc::downgrade(&inner);
        thread::Builder::new()
            .name("voice-hotkey-dispatch".into())
            .spawn(move || {
                while let Ok(ev) = rx.recv() {
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    inner.deliver(ev);
                }
            })
            .ok();
        HotkeyController { inner }
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
    /// **This call blocks the calling thread for up to `START_GRACE`
    /// (300 ms) whenever the hook actually comes up** (an immediate refusal
    /// returns at once, and so does the already-running case). The Swift
    /// `startTap()` returned immediately; this one cannot, see below. Call it
    /// off the UI thread — `tauri::async_runtime::spawn_blocking` from the
    /// setup hook and from the once-a-second Accessibility retry timer —
    /// rather than directly on the Tauri main thread.
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
        // Checked before anything is spawned (a Wayland session never gets a
        // hook thread); it is a stateless environment read, so it needs no
        // serialisation with `start_lock`.
        if let Err(reason) = crate::platform::global_hotkeys_supported() {
            self.inner.warn_once("hotkey hook unavailable", &reason);
            return false;
        }
        self.start_with(run_hook)
    }

    /// `start` with the hook entry point injected, so the lifecycle rules
    /// (grace window, silence-means-success, late death, idempotence) can be
    /// tested without an OS hook or any dependence on the host's session.
    /// `run` must behave like `run_hook`: block for as long as the hook
    /// lives, return `Err` if it could not be installed.
    fn start_with(&self, run: fn(Arc<Inner>) -> Result<(), String>) -> bool {
        let _serial = self.inner.start_lock.lock();
        if self.tap_running() {
            return true;
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
                let result = run(inner);
                // The receiver may be gone if the controller was dropped.
                let _ = tx.send(result);
            })
            .ok();

        match rx.recv_timeout(START_GRACE) {
            Err(RecvTimeoutError::Timeout) => {
                // Publish the receiver *before* flipping `running`:
                // `tap_running` does not take `start_lock`, and it reads
                // `running == true` with no receiver as "the hook died",
                // which would reset the controller underneath us and leave
                // this hook unwatched (and a second one spawned by the
                // app's retry).
                *self
                    .inner
                    .watch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(rx);
                self.inner.running.store(true, Ordering::SeqCst);
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
        // Recover a poisoned lock rather than bail: the receiver inside is
        // always consistent, and giving up here (in either direction) would
        // be wrong — "running" would hide a dead hook behind a live status
        // line, "dead" would make the retry timer spawn a second hook thread
        // every second because `start_with` could no longer publish its
        // receiver.
        let mut w = self
            .inner
            .watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let died = match w.as_ref().map(Receiver::try_recv) {
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
        };
        drop(w);
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
    ///
    /// Left Option is rdev's `Alt` and right Option its `AltGr`. Every key on
    /// the talk-key picker has an arm (no wildcard), so a key added to
    /// `Hotkey` does not compile until it is given a delivery key here.
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
            Hotkey::RightControl => Key::ControlRight,
            Hotkey::LeftOption => Key::Alt,
            Hotkey::LeftCommand => Key::MetaLeft,
            Hotkey::LeftControl => Key::ControlLeft,
            Hotkey::LeftShift => Key::ShiftLeft,
            Hotkey::RightShift => Key::ShiftRight,
            Hotkey::Fn => Key::Function,
            Hotkey::F1 => Key::F1,
            Hotkey::F2 => Key::F2,
            Hotkey::F3 => Key::F3,
            Hotkey::F4 => Key::F4,
            Hotkey::F5 => Key::F5,
            Hotkey::F6 => Key::F6,
            Hotkey::F7 => Key::F7,
            Hotkey::F8 => Key::F8,
            Hotkey::F9 => Key::F9,
            Hotkey::F10 => Key::F10,
            Hotkey::F11 => Key::F11,
            Hotkey::F12 => Key::F12,
            Hotkey::A => Key::KeyA,
            Hotkey::B => Key::KeyB,
            Hotkey::C => Key::KeyC,
            Hotkey::D => Key::KeyD,
            Hotkey::E => Key::KeyE,
            Hotkey::F => Key::KeyF,
            Hotkey::G => Key::KeyG,
            Hotkey::H => Key::KeyH,
            Hotkey::I => Key::KeyI,
            Hotkey::J => Key::KeyJ,
            Hotkey::K => Key::KeyK,
            Hotkey::L => Key::KeyL,
            Hotkey::M => Key::KeyM,
            Hotkey::N => Key::KeyN,
            Hotkey::O => Key::KeyO,
            Hotkey::P => Key::KeyP,
            Hotkey::Q => Key::KeyQ,
            Hotkey::R => Key::KeyR,
            Hotkey::S => Key::KeyS,
            Hotkey::T => Key::KeyT,
            Hotkey::U => Key::KeyU,
            Hotkey::V => Key::KeyV,
            Hotkey::W => Key::KeyW,
            Hotkey::X => Key::KeyX,
            Hotkey::Y => Key::KeyY,
            Hotkey::Z => Key::KeyZ,
            Hotkey::Digit0 => Key::Num0,
            Hotkey::Digit1 => Key::Num1,
            Hotkey::Digit2 => Key::Num2,
            Hotkey::Digit3 => Key::Num3,
            Hotkey::Digit4 => Key::Num4,
            Hotkey::Digit5 => Key::Num5,
            Hotkey::Digit6 => Key::Num6,
            Hotkey::Digit7 => Key::Num7,
            Hotkey::Digit8 => Key::Num8,
            Hotkey::Digit9 => Key::Num9,
            Hotkey::Backquote => Key::BackQuote,
            Hotkey::Minus => Key::Minus,
            Hotkey::Equal => Key::Equal,
            Hotkey::BracketLeft => Key::LeftBracket,
            Hotkey::BracketRight => Key::RightBracket,
            Hotkey::Backslash => Key::BackSlash,
            Hotkey::Semicolon => Key::SemiColon,
            Hotkey::Quote => Key::Quote,
            Hotkey::Comma => Key::Comma,
            Hotkey::Period => Key::Dot,
            Hotkey::Slash => Key::Slash,
            Hotkey::Space => Key::Space,
            Hotkey::Tab => Key::Tab,
            Hotkey::Enter => Key::Return,
            Hotkey::Backspace => Key::Backspace,
            Hotkey::ArrowUp => Key::UpArrow,
            Hotkey::ArrowDown => Key::DownArrow,
            Hotkey::ArrowLeft => Key::LeftArrow,
            Hotkey::ArrowRight => Key::RightArrow,
        }
    }
}

/// The CGEventFlags bit a modifier hotkey toggles, i.e. `Hotkey.flag` in
/// Swift. `None` for keys that arrive as keyDown/keyUp instead of
/// flagsChanged.
#[cfg(target_os = "macos")]
fn hotkey_flag(hotkey: Hotkey) -> Option<core_graphics::event::CGEventFlags> {
    use core_graphics::event::CGEventFlags;
    match hotkey {
        Hotkey::LeftOption | Hotkey::RightOption => Some(CGEventFlags::CGEventFlagAlternate),
        Hotkey::LeftCommand | Hotkey::RightCommand => Some(CGEventFlags::CGEventFlagCommand),
        Hotkey::LeftControl | Hotkey::RightControl => Some(CGEventFlags::CGEventFlagControl),
        Hotkey::LeftShift | Hotkey::RightShift => Some(CGEventFlags::CGEventFlagShift),
        Hotkey::Fn => Some(CGEventFlags::CGEventFlagSecondaryFn),
        _ => None,
    }
}

/// Blocks for the lifetime of the tap; returns `Err` when the tap could not
/// be created (no Accessibility grant) and `Ok` only if the run loop ends.
///
/// Mirrors `HotkeyController.startTap` / `handle` in core.swift: session
/// tap, head insert, `flagsChanged | keyDown | keyUp` only, and re-enable on the two
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
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
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
                CGEventType::FlagsChanged | CGEventType::KeyDown | CGEventType::KeyUp => {}
                _ => return CallbackResult::Keep,
            }
            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            let hotkey = inner.hotkey();
            let ev = match kind {
                CGEventType::FlagsChanged => TapEvent::FlagsChanged {
                    keycode,
                    pressed: hotkey_flag(hotkey)
                        .is_some_and(|flag| event.get_flags().contains(flag)),
                },
                CGEventType::KeyUp => TapEvent::KeyUp { keycode },
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
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

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
        // Left Option is rdev's `Alt`, not `AltGr`.
        assert_eq!(HotkeyController::rdev_key(Hotkey::LeftOption), Key::Alt);
        assert_eq!(HotkeyController::rdev_key(Hotkey::F5), Key::F5);
        assert_eq!(HotkeyController::rdev_key(Hotkey::A), Key::KeyA);
        assert_eq!(HotkeyController::rdev_key(Hotkey::Digit1), Key::Num1);
        assert_eq!(HotkeyController::rdev_key(Hotkey::Enter), Key::Return);
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

    // A text key as the talk key must not reach the frontmost app: holding
    // F5 (or Space, or a letter) would otherwise also press it there.
    #[test]
    fn text_talk_key_press_repeat_and_release_are_swallowed() {
        let mut s = HookState::default();
        let hk = Hotkey::F5;
        assert_eq!(
            handle(&mut s, &press(Key::F5), hk, false),
            (Some(HotkeyEvent::Down), true)
        );
        // OS key repeat while held.
        assert_eq!(handle(&mut s, &press(Key::F5), hk, true), (None, true));
        assert_eq!(
            handle(&mut s, &release(Key::F5), hk, true),
            (Some(HotkeyEvent::Up), true)
        );
        assert!(!s.held);
        // Space behaves the same way.
        assert_eq!(
            handle(&mut s, &press(Key::Space), Hotkey::Space, false),
            (Some(HotkeyEvent::Down), true)
        );
        assert_eq!(
            handle(&mut s, &release(Key::Space), Hotkey::Space, true),
            (Some(HotkeyEvent::Up), true)
        );
    }

    // The release of a key whose press this hook never saw (it was already
    // down when the hook started) has to reach the app, because the app got
    // that press.
    #[test]
    fn release_of_a_text_key_never_pressed_here_passes_through() {
        let mut s = HookState::default();
        assert_eq!(
            handle(&mut s, &release(Key::F5), Hotkey::F5, false),
            (None, false)
        );
    }

    // Modifiers are never swallowed: their keystrokes are flag changes that
    // other apps (⇧-click, ⌥-drag) depend on.
    #[test]
    fn modifier_talk_key_is_never_swallowed() {
        let mut s = HookState::default();
        let hk = Hotkey::LeftShift;
        assert_eq!(
            handle(&mut s, &press(Key::ShiftLeft), hk, false),
            (Some(HotkeyEvent::Down), false)
        );
        assert_eq!(
            handle(&mut s, &press(Key::ShiftLeft), hk, true),
            (None, false)
        );
        assert_eq!(
            handle(&mut s, &release(Key::ShiftLeft), hk, true),
            (Some(HotkeyEvent::Up), false)
        );
    }

    // Escape stays the cancel key even when a text key is the talk key, and
    // another text key still cancels and passes through.
    #[test]
    fn text_talk_key_keeps_the_cancel_rules() {
        let mut s = HookState { held: true };
        assert_eq!(
            handle(&mut s, &press(Key::Escape), Hotkey::F5, true),
            (Some(HotkeyEvent::Cancel), true)
        );
        assert_eq!(
            handle(&mut s, &press(Key::KeyC), Hotkey::F5, true),
            (Some(HotkeyEvent::Cancel), false)
        );
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
            // Right Win/Super: `Unknown(92)` / `Unknown(134)` off macOS.
            cmd(),
            Key::CapsLock,
            Key::NumLock,
            Key::ScrollLock,
            Key::Pause,
        ] {
            assert_eq!(
                handle(&mut s, &press(key), Hotkey::RightOption, true),
                (None, false),
                "{key:?}"
            );
        }
        assert!(s.held);
    }

    // A text key's keyDown and keyUp carry its own keycode; the rest of the
    // keyboard is either "some other key" or ignored on release.
    #[test]
    fn tap_events_for_a_text_talk_key() {
        let hk = Hotkey::F5;
        assert_eq!(
            tap_to_event(TapEvent::KeyDown { keycode: 96 }, hk),
            Some(press(Key::F5))
        );
        assert_eq!(
            tap_to_event(TapEvent::KeyUp { keycode: 96 }, hk),
            Some(release(Key::F5))
        );
        // Another key's release is not ours, and cancels nothing.
        assert_eq!(tap_to_event(TapEvent::KeyUp { keycode: 8 }, hk), None);
        // A text key never reports a modifier flag change.
        assert_eq!(
            tap_to_event(
                TapEvent::FlagsChanged {
                    keycode: 96,
                    pressed: true
                },
                hk
            ),
            None
        );
        // Other keyDowns still cancel a recording.
        let other = tap_to_event(TapEvent::KeyDown { keycode: 8 }, hk).unwrap();
        let mut s = HookState { held: true };
        assert_eq!(
            handle(&mut s, &other, hk, true),
            (Some(HotkeyEvent::Cancel), false)
        );
    }

    // End to end for a letter: the keyDown of "A" (keycode 0) is the press
    // of the talk key, is swallowed, and its keyUp ends the recording.
    #[test]
    fn letter_talk_key_round_trips_through_the_tap() {
        let hk = Hotkey::A;
        let mut s = HookState::default();
        let down = tap_to_event(TapEvent::KeyDown { keycode: 0 }, hk).unwrap();
        assert_eq!(
            handle(&mut s, &down, hk, false),
            (Some(HotkeyEvent::Down), true)
        );
        let up = tap_to_event(TapEvent::KeyUp { keycode: 0 }, hk).unwrap();
        assert_eq!(handle(&mut s, &up, hk, true), (Some(HotkeyEvent::Up), true));
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

    // The app callback runs on the dispatch thread, never on the thread that
    // fed the event in (the OS hook thread in production).
    #[test]
    fn events_are_delivered_off_the_hook_thread() {
        let c = HotkeyController::new(Arc::new(|| Hotkey::RightOption), Arc::new(|| false));
        let (tx, rx) = mpsc::channel();
        c.set_on_event(Arc::new(move |ev| {
            let _ = tx.send((ev, thread::current().name().map(str::to_string)));
        }));
        let swallow = c.inner.dispatch(&press(Key::AltGr), Hotkey::RightOption);
        assert!(!swallow);
        let (ev, name) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(ev, HotkeyEvent::Down);
        assert_eq!(name.as_deref(), Some("voice-hotkey-dispatch"));
        c.inner.dispatch(&release(Key::AltGr), Hotkey::RightOption);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
            HotkeyEvent::Up
        );
    }

    // `Config::hotkey` does not clamp to `Hotkey::available()`, so a
    // settings file from another platform can name a key rdev cannot deliver
    // here. The hook must fall back to the platform default rather than run
    // with a key that never matches.
    #[test]
    fn unavailable_hotkey_falls_back_to_platform_default() {
        let available = Hotkey::available();
        // macOS hooks every key on the picker, so it has no foreign key to
        // clamp; the other platforms do.
        if let Some(foreign) = Hotkey::ALL.iter().copied().find(|h| !available.contains(h)) {
            let c = HotkeyController::new(Arc::new(move || foreign), Arc::new(|| false));
            assert_eq!(c.inner.hotkey(), available[0]);
            // Only the log line is deduplicated; the clamp itself is per call.
            assert_eq!(c.inner.hotkey(), available[0]);
        }
        for hk in available.iter().copied() {
            let c = HotkeyController::new(Arc::new(move || hk), Arc::new(|| false));
            assert_eq!(c.inner.hotkey(), hk);
        }
    }

    // The state lock is taken inside the OS hook callback; a panic that
    // poisoned it must not turn the hook mute for the rest of the process.
    #[test]
    fn poisoned_state_lock_still_dispatches() {
        let c = controller();
        let (tx, rx) = mpsc::channel();
        c.set_on_event(Arc::new(move |ev| {
            let _ = tx.send(ev);
        }));
        let inner = Arc::clone(&c.inner);
        let _ = thread::spawn(move || {
            let _guard = inner.state.lock().unwrap();
            panic!("poison the state lock");
        })
        .join();
        assert!(c.inner.state.lock().is_err(), "lock was not poisoned");
        c.inner.dispatch(&press(Key::AltGr), Hotkey::RightOption);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            HotkeyEvent::Down
        );
    }

    // Likewise for the watch lock: the hook's health must stay readable
    // through a poisoned lock. Reporting "dead" instead would send the app's
    // retry into `start`, which could then not publish its receiver and
    // would spawn a fresh hook thread on every tick.
    #[test]
    fn poisoned_watch_lock_keeps_tracking_the_hook() {
        let c = controller();
        assert!(c.start_with(hook_parks_uncounted));
        assert!(c.tap_running());
        let inner = Arc::clone(&c.inner);
        let _ = thread::spawn(move || {
            let _guard = inner.watch.lock().unwrap();
            panic!("poison the watch lock");
        })
        .join();
        assert!(c.inner.watch.lock().is_err(), "lock was not poisoned");
        assert!(c.tap_running());
        // Still one hook: a second start sees it and returns at once.
        let t = Instant::now();
        assert!(c.start_with(hook_parks_uncounted));
        assert!(t.elapsed() < START_GRACE);
        // And a death is still noticed through the poisoned lock.
        *c.inner
            .watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        assert!(!c.tap_running());
        assert!(!c.inner.running.load(Ordering::SeqCst));
    }

    #[test]
    fn controller_reports_not_running_before_start() {
        let c = HotkeyController::new(Arc::new(|| Hotkey::RightOption), Arc::new(|| false));
        assert!(!c.tap_running());
    }

    // ---- start / tap_running lifecycle, with the OS hook replaced by a fake.

    fn controller() -> HotkeyController {
        HotkeyController::new(Arc::new(|| Hotkey::RightOption), Arc::new(|| false))
    }

    fn hook_refuses(_: Arc<Inner>) -> Result<(), String> {
        Err("no grant".to_string())
    }

    /// Counts how many hook threads were ever started with this entry point.
    /// Only `repeated_start_spawns_one_hook` uses it, so the count is exact
    /// even though tests run in parallel.
    static PARKED_HOOKS: AtomicUsize = AtomicUsize::new(0);

    fn hook_parks_forever(_: Arc<Inner>) -> Result<(), String> {
        PARKED_HOOKS.fetch_add(1, Ordering::SeqCst);
        loop {
            thread::park();
        }
    }

    /// Same as `hook_parks_forever` but uncounted, for tests that only need
    /// a live hook and must not disturb `PARKED_HOOKS`.
    fn hook_parks_uncounted(_: Arc<Inner>) -> Result<(), String> {
        loop {
            thread::park();
        }
    }

    fn hook_ends_after_grace(_: Arc<Inner>) -> Result<(), String> {
        thread::sleep(START_GRACE * 2);
        Ok(())
    }

    fn hook_dies_after_grace(_: Arc<Inner>) -> Result<(), String> {
        thread::sleep(START_GRACE * 2);
        Err("tap disabled".to_string())
    }

    /// Polls `tap_running` the way the app's status refresh does.
    fn wait_until_stopped(c: &HotkeyController) -> bool {
        for _ in 0..100 {
            if !c.tap_running() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    // An OS refusal (no Accessibility, no X11) is reported at once: `start`
    // must not sit out the grace period when the answer is already known.
    #[test]
    fn immediate_refusal_fails_fast() {
        let c = controller();
        let t = Instant::now();
        assert!(!c.start_with(hook_refuses));
        assert!(t.elapsed() < START_GRACE, "start waited {:?}", t.elapsed());
        assert!(!c.tap_running());
    }

    // The hook entry points never return on success, so silence for
    // START_GRACE is the success signal — and the cost is that `start`
    // blocks for that long. A second `start` sees the live hook and must not
    // install another one (two hooks would fight over the OS hook slot).
    #[test]
    fn repeated_start_spawns_one_hook() {
        let c = controller();
        let t = Instant::now();
        assert!(c.start_with(hook_parks_forever));
        assert!(t.elapsed() >= START_GRACE);
        assert!(c.tap_running());
        let t = Instant::now();
        assert!(c.start_with(hook_parks_forever));
        assert!(t.elapsed() < START_GRACE, "second start waited the grace");
        assert!(c.tap_running());
        assert_eq!(PARKED_HOOKS.load(Ordering::SeqCst), 1);
    }

    // A hook whose run loop ends after the grace period was reported as up;
    // the next status poll must notice, reset the held state (the key is not
    // really down any more from the app's point of view) and let `start` try
    // again.
    #[test]
    fn hook_ending_after_grace_is_detected_by_tap_running() {
        let c = controller();
        assert!(c.start_with(hook_ends_after_grace));
        assert!(c.tap_running());
        c.inner.state.lock().unwrap().held = true;
        assert!(wait_until_stopped(&c), "hook death never observed");
        assert!(!c.tap_running());
        assert_eq!(*c.inner.state.lock().unwrap(), HookState::default());
        // The controller is startable again, not wedged on the dead watch.
        assert!(!c.start_with(hook_refuses));
    }

    #[test]
    fn hook_failing_after_grace_is_detected_by_tap_running() {
        let c = controller();
        assert!(c.start_with(hook_dies_after_grace));
        assert!(c.tap_running());
        assert!(wait_until_stopped(&c), "hook death never observed");
        assert!(!c.inner.running.load(Ordering::SeqCst));
        assert!(c.inner.watch.lock().unwrap().is_none());
    }

    // The hook thread pins `Inner` for the process lifetime, so the channel
    // would never close on its own; `Drop` has to close it, or events keep
    // reaching the callback after the app has torn the controller down.
    #[test]
    fn dropping_the_controller_closes_the_event_queue() {
        let c = controller();
        let (tx, rx) = mpsc::channel();
        c.set_on_event(Arc::new(move |ev| {
            let _ = tx.send(ev);
        }));
        // Stand in for the hook thread's strong reference.
        let pinned = Arc::clone(&c.inner);
        drop(c);
        assert!(pinned.queue.lock().unwrap().is_none());
        assert!(pinned.shutdown.load(Ordering::SeqCst));
        // A late event from the (still running) hook is dropped, not
        // delivered, and does not panic.
        assert!(!pinned.dispatch(&press(Key::AltGr), Hotkey::RightOption));
        // An event the hook queued just before the drop (mpsc hands buffered
        // items out even after the sender is gone) is not delivered either.
        pinned.deliver(HotkeyEvent::Down);
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(200)),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected)
        ));
    }
}
