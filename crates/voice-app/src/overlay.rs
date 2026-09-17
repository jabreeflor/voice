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
//! Every state change bumps a generation counter. The 30 Hz level thread and
//! the deferred hide after a flash both check it, so a newer show cancels a
//! pending hide and stops a stale level loop without any timer bookkeeping
//! (the `DispatchWorkItem` juggling of the Swift version).

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

const WINDOW: &str = "overlay";
/// Logical size from tauri.conf.json; the pill is positioned from it.
const WIDTH: f64 = 192.0;
const HEIGHT: f64 = 44.0;
/// Gap between the pill and the bottom of the work area (Dock/taskbar).
const BOTTOM_MARGIN: f64 = 28.0;
const LEVEL_INTERVAL: Duration = Duration::from_millis(1000 / 30);
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

pub struct Overlay {
    handle: AppHandle,
    /// Incremented on every show/hide; threads spawned for an older value
    /// stop as soon as they notice.
    generation: Arc<Mutex<u64>>,
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
        Overlay {
            handle,
            generation: Arc::new(Mutex::new(0)),
        }
    }

    fn bump(&self) -> u64 {
        let mut g = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        *g += 1;
        *g
    }

    fn current(generation: &Mutex<u64>) -> u64 {
        *generation.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.handle.get_webview_window(WINDOW)
    }

    fn position(&self, window: &tauri::WebviewWindow) {
        let Ok(Some(monitor)) = self.handle.primary_monitor() else {
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
    fn show(&self, mode: &'static str, message: &str, duration: Duration) -> u64 {
        let generation = self.bump();
        if let Some(window) = self.window() {
            self.position(&window);
            if let Err(e) = window.show() {
                log::warn!("overlay show: {e}");
            }
        }
        self.emit_overlay(mode, message, duration);
        generation
    }

    /// Shows the bars driven by `level_provider` (polled ~30 Hz on a thread).
    pub fn show_listening(&self, level_provider: Box<dyn Fn() -> f32 + Send + Sync>) {
        let generation = self.show("listening", "", Duration::ZERO);
        let handle = self.handle.clone();
        let gen_cell = self.generation.clone();
        std::thread::Builder::new()
            .name("voice-overlay-level".into())
            .spawn(move || {
                while Self::current(&gen_cell) == generation {
                    let value = level_provider();
                    let _ = handle.emit_to(WINDOW, "level", LevelPayload { value });
                    std::thread::sleep(LEVEL_INTERVAL);
                }
            })
            .expect("spawn overlay level thread");
    }

    pub fn show_processing(&self) {
        self.show("processing", "", Duration::ZERO);
    }

    /// Shows `message` then hides after `duration` (default 1.1 s).
    pub fn flash(&self, message: &str, duration: Duration) {
        let generation = self.show("flash", message, duration);
        let handle = self.handle.clone();
        let gen_cell = self.generation.clone();
        std::thread::Builder::new()
            .name("voice-overlay-flash".into())
            .spawn(move || {
                std::thread::sleep(duration);
                // A newer show (or an explicit hide) since we were scheduled
                // owns the window now; leave it alone.
                if Self::current(&gen_cell) != generation {
                    return;
                }
                Overlay {
                    handle,
                    generation: gen_cell,
                }
                .hide();
            })
            .expect("spawn overlay flash thread");
    }

    pub fn hide(&self) {
        self.bump();
        self.emit_overlay("hidden", "", Duration::ZERO);
        if let Some(window) = self.window() {
            if let Err(e) = window.hide() {
                log::warn!("overlay hide: {e}");
            }
        }
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
