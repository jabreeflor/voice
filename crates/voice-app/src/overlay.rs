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
//!
//! Every window change goes through one worker thread fed by a channel:
//! show/hide/flash only send a command, and the worker owns the level loop,
//! the deferred hide after a flash and the fade-out before `hide()`. A
//! newer command simply arrives before the pending timeout and replaces it,
//! so a flash can never hide a listening pill that started after it (the
//! `DispatchWorkItem` juggling of the Swift version, without the races a
//! generation counter leaves open).

#![allow(dead_code)]

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

const WINDOW: &str = "overlay";
/// Logical size from tauri.conf.json; the pill is positioned from it.
const WIDTH: f64 = 192.0;
const HEIGHT: f64 = 44.0;
/// Gap between the pill and the bottom of the work area (Dock/taskbar).
const BOTTOM_MARGIN: f64 = 28.0;
const LEVEL_INTERVAL: Duration = Duration::from_millis(1000 / 30);
/// How long the webview's opacity transition runs after `hidden` before the
/// OS window is hidden (Swift animated alpha to 0 over 0.18 s, then
/// `orderOut`). Hiding sooner cuts the fade.
const FADE_OUT: Duration = Duration::from_millis(180);
pub const FLASH_DEFAULT: Duration = Duration::from_millis(1100);

#[derive(Serialize, Clone)]
struct OverlayPayload {
    mode: &'static str,
    message: String,
    duration_ms: u64,
}

#[derive(Serialize, Clone)]
struct LevelPayload {
    value: f32,
}

#[derive(Serialize, Clone)]
struct SoundPayload {
    name: String,
}

type LevelProvider = Box<dyn Fn() -> f32 + Send + Sync>;

enum Command {
    Show {
        mode: &'static str,
        message: String,
        duration: Duration,
        level: Option<LevelProvider>,
    },
    Hide,
}

/// What the worker does while no command is pending.
enum Pending {
    /// Nothing scheduled; block on the channel.
    Idle,
    /// Emit the mic level every `LEVEL_INTERVAL`.
    Level(LevelProvider),
    /// A flash is on screen; start hiding at the deadline.
    FlashUntil(Instant),
    /// `hidden` was emitted; hide the OS window once the fade has run.
    FadeUntil(Instant),
}

pub struct Overlay {
    handle: AppHandle,
    /// `None` when the worker thread could not be spawned; the pill is then
    /// simply never shown (dictation itself still works).
    tx: Option<mpsc::Sender<Command>>,
}

/// Where to put a `w`×`h` window so it sits centred `margin` above the bottom
/// of `area` (all in the same pixel space).
pub fn bottom_centre(
    area_x: i32,
    area_y: i32,
    area_w: u32,
    area_h: u32,
    w: f64,
    h: f64,
    margin: f64,
) -> (i32, i32) {
    let x = area_x as f64 + (area_w as f64 - w) / 2.0;
    let y = area_y as f64 + area_h as f64 - h - margin;
    (x.round() as i32, y.round() as i32)
}

impl Overlay {
    pub fn new(handle: AppHandle) -> Overlay {
        let (tx, rx) = mpsc::channel();
        let worker_handle = handle.clone();
        let spawned = std::thread::Builder::new()
            .name("voice-overlay".into())
            .spawn(move || Worker::new(worker_handle).run(rx));
        let tx = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                log::error!("spawn voice-overlay: {e}");
                None
            }
        };
        Overlay { handle, tx }
    }

    /// One-off window setup that has no WindowConfig key in tauri-utils 2.9,
    /// applied from the setup hook (main thread):
    ///
    /// - clicks fall through the pill to whatever is underneath (the Swift
    ///   panel set `ignoresMouseEvents`);
    /// - on macOS the panel must also join full-screen Spaces and sit at the
    ///   status-bar level. `visibleOnAllWorkspaces` only maps to
    ///   `canJoinAllSpaces` and `alwaysOnTop` to `NSFloatingWindowLevel`, which
    ///   is not enough for the pill to appear over a full-screen app — the
    ///   apps people dictate into most.
    pub fn configure_window(&self) {
        if let Some(window) = self.window() {
            if let Err(e) = window.set_ignore_cursor_events(true) {
                log::warn!("overlay ignore cursor: {e}");
            }
            #[cfg(target_os = "macos")]
            match window.ns_window() {
                Ok(ptr) => macos::pin_above_full_screen(ptr),
                Err(e) => log::warn!("overlay ns_window: {e}"),
            }
        }
    }

    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.handle.get_webview_window(WINDOW)
    }

    fn send(&self, command: Command) {
        if let Some(tx) = &self.tx {
            // The worker only ends when the app does.
            let _ = tx.send(command);
        }
    }

    /// Shows the bars driven by `level_provider` (polled ~30 Hz by the worker).
    pub fn show_listening(&self, level_provider: LevelProvider) {
        self.send(Command::Show {
            mode: "listening",
            message: String::new(),
            duration: Duration::ZERO,
            level: Some(level_provider),
        });
    }

    pub fn show_processing(&self) {
        self.send(Command::Show {
            mode: "processing",
            message: String::new(),
            duration: Duration::ZERO,
            level: None,
        });
    }

    /// Shows `message` then hides after `duration` (default 1.1 s).
    pub fn flash(&self, message: &str, duration: Duration) {
        self.send(Command::Show {
            mode: "flash",
            message: message.to_string(),
            duration,
            level: None,
        });
    }

    pub fn hide(&self) {
        self.send(Command::Hide);
    }

    pub fn play_sound(&self, name: &str) {
        let payload = SoundPayload {
            name: name.to_string(),
        };
        if let Err(e) = self.handle.emit_to(WINDOW, "sound", payload) {
            log::warn!("sound event: {e}");
        }
    }
}

/// The overlay worker: the only code that shows, positions and hides the
/// window, so those calls are serialised in command order.
struct Worker {
    handle: AppHandle,
}

impl Worker {
    fn new(handle: AppHandle) -> Worker {
        Worker { handle }
    }

    fn run(self, rx: mpsc::Receiver<Command>) {
        let mut pending = Pending::Idle;
        loop {
            let received = match &pending {
                Pending::Idle => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Pending::Level(_) => rx.recv_timeout(LEVEL_INTERVAL),
                Pending::FlashUntil(deadline) | Pending::FadeUntil(deadline) => {
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
            };
            match received {
                Ok(command) => pending = self.apply(command),
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => pending = self.tick(pending),
            }
        }
    }

    /// A command always replaces whatever was pending: a show cancels a
    /// deferred hide, a hide stops the level loop.
    fn apply(&self, command: Command) -> Pending {
        match command {
            Command::Show {
                mode,
                message,
                duration,
                level,
            } => {
                self.show(mode, &message, duration);
                match (mode, level) {
                    (_, Some(level)) => Pending::Level(level),
                    ("flash", None) => Pending::FlashUntil(Instant::now() + duration),
                    _ => Pending::Idle,
                }
            }
            Command::Hide => self.begin_hide(),
        }
    }

    fn tick(&self, pending: Pending) -> Pending {
        match pending {
            Pending::Idle => Pending::Idle,
            Pending::Level(level) => {
                let value = level();
                let _ = self.handle.emit_to(WINDOW, "level", LevelPayload { value });
                Pending::Level(level)
            }
            Pending::FlashUntil(_) => self.begin_hide(),
            Pending::FadeUntil(_) => {
                self.hide_window();
                Pending::Idle
            }
        }
    }

    /// Tells the webview to fade out; the OS window follows after `FADE_OUT`.
    fn begin_hide(&self) -> Pending {
        self.emit_overlay("hidden", "", Duration::ZERO);
        Pending::FadeUntil(Instant::now() + FADE_OUT)
    }

    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.handle.get_webview_window(WINDOW)
    }

    fn hide_window(&self) {
        if let Some(window) = self.window() {
            if let Err(e) = window.hide() {
                log::warn!("overlay hide: {e}");
            }
        }
    }

    fn position(&self, window: &tauri::WebviewWindow) {
        // Ask the window, not the AppHandle: the window getter marshals to
        // the main thread, whereas `AppHandle::primary_monitor` reads
        // `NSScreen.screens` on the calling (worker) thread.
        let Ok(Some(monitor)) = window.primary_monitor() else {
            return;
        };
        let scale = monitor.scale_factor();
        // `work_area` excludes the Dock / taskbar; the Swift version used
        // `visibleFrame` for the same reason.
        let area = monitor.work_area();
        let (ax, ay, aw, ah) = if area.size.width == 0 || area.size.height == 0 {
            let size = monitor.size();
            let pos = monitor.position();
            (pos.x, pos.y, size.width, size.height)
        } else {
            (
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
            )
        };
        let (x, y) = bottom_centre(
            ax,
            ay,
            aw,
            ah,
            WIDTH * scale,
            HEIGHT * scale,
            BOTTOM_MARGIN * scale,
        );
        if let Err(e) = window.set_position(PhysicalPosition::new(x, y)) {
            log::warn!("overlay position: {e}");
        }
    }

    fn emit_overlay(&self, mode: &'static str, message: &str, duration: Duration) {
        let payload = OverlayPayload {
            mode,
            message: message.to_string(),
            duration_ms: duration.as_millis() as u64,
        };
        if let Err(e) = self.handle.emit_to(WINDOW, "overlay", payload) {
            log::warn!("overlay event: {e}");
        }
    }

    /// Repositions and shows the window (it is `focusable: false`, so this
    /// does not steal the caret from the app being dictated into), then tells
    /// the webview which face to wear.
    fn show(&self, mode: &'static str, message: &str, duration: Duration) {
        if let Some(window) = self.window() {
            self.position(&window);
            if let Err(e) = window.show() {
                log::warn!("overlay show: {e}");
            }
        }
        self.emit_overlay(mode, message, duration);
    }
}

/// The one FFI shim outside `platform/`: it needs the overlay's `NSWindow`,
/// which only the Tauri window exposes.
#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior, NSWindowLevel};

    /// `NSStatusWindowLevel` (25); the Swift panel used `.statusBar`.
    const STATUS_WINDOW_LEVEL: NSWindowLevel = 25;

    /// Mirrors `panel.collectionBehavior = [.canJoinAllSpaces,
    /// .fullScreenAuxiliary]` and `panel.level = .statusBar`.
    pub fn pin_above_full_screen(ptr: *mut std::ffi::c_void) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: `WebviewWindow::ns_window` returns the live `NSWindow*` of
        // this window, and the caller (the setup hook) is on the main
        // thread, which NSWindow requires. The reference does not outlive
        // this call.
        let window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        let behavior = window.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        window.setCollectionBehavior(behavior);
        window.setLevel(STATUS_WINDOW_LEVEL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_sits_centred_28px_above_the_work_area() {
        // 1920×1080 work area starting at the origin, 1× scale.
        let (x, y) = bottom_centre(0, 0, 1920, 1080, 192.0, 44.0, 28.0);
        assert_eq!((x, y), (864, 1008));
        // A secondary-origin work area (e.g. a Dock on the left) is honoured.
        let (x, y) = bottom_centre(70, 25, 1850, 1055, 192.0, 44.0, 28.0);
        assert_eq!((x, y), (70 + 829, 25 + 1055 - 44 - 28));
    }
}
