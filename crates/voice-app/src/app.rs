//! Application state and the dictation flow. Reference: `AppDelegate` in
//! Sources/VoiceCore/app.swift.
//!
//! One `App` lives in Tauri managed state (`app_handle.state::<Arc<App>>()`).
//! All fields that change are behind mutexes so hotkey, audio, engine and UI
//! threads can share it.
//!
//! Threading and lock rules (keep them — the Swift original was single
//! threaded and the port relies on these instead):
//!
//! - Sync `#[tauri::command]`s run on the main thread and lock App mutexes.
//!   Tray menu updates (`MenuItem::set_text`) and window getters
//!   (`is_visible`, `outer_size`) round-trip to the main thread. So **no App
//!   mutex is ever held across a call into tauri (emit, window, menu, tray)
//!   or into another `App` method that locks** — read what you need, drop the
//!   guard, then act. Otherwise a worker holding a lock waits on the main
//!   thread while the main thread waits on the lock.
//! - The mutexes are not nested, with one exception: `claim_recording` /
//!   `claim_preview` read `preview_active` under `state` so the dictation
//!   worker and the main thread cannot both start the recorder. Any further
//!   nesting must follow the order `state` → `preview_active` → `recorder` →
//!   `snippets` → `history` → `engine` → `setup_*` → `tray_items`.
//! - Bind a lock's result before testing it (`let r = lock(&x).f(); if let
//!   Err(e) = r`): in edition 2021 an `if let` on the locked expression keeps
//!   the guard alive for the whole body.
//! - Hotkey events arrive on the hook thread and must not block it: the
//!   callback only pushes onto a channel; a single worker thread applies them
//!   in order (Down before Up, always). Transcription runs on its own thread
//!   because `WhisperEngine::transcribe` blocks for the whole request.
//! - Window visibility is tracked in atomics updated by `show_*` and the
//!   close-request handler, so `status_info()` never has to ask the window
//!   (which would be a main-thread round trip from the status callbacks).

#![allow(dead_code)]

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::menu::MenuItem;
use tauri::{AppHandle, Emitter, Manager};
use voice_core::{
    clean_transcript, compute_status, wav_data, Config, DictationEntry, HistoryStore, Hotkey,
    ModelCatalog, ModelDownloader, Platform, Settings, SnippetStore, StatusInfo, StatusInputs,
    Store, WhisperEngine,
};

use crate::audio::Recorder;
use crate::hotkey::{HotkeyController, HotkeyEvent};
use crate::overlay::Overlay;
use crate::paste::{copy_text, paste_text};
use crate::platform::{self, MicStatus};
use crate::tray;

pub const MAIN_WINDOW: &str = "main";
pub const ONBOARDING_WINDOW: &str = "onboarding";
pub const OVERLAY_WINDOW: &str = "overlay";

/// Taps shorter than this are ignored (an accidental brush of the key).
const MIN_RECORDING: Duration = Duration::from_millis(350);
/// …and so are captures with fewer samples than this (~0.25 s at 16 kHz),
/// which catches a stalled input device even when the wall clock says 0.35 s.
const MIN_SAMPLES: usize = 4000;
/// A relaunch triggered less than this long ago means the Accessibility
/// grant is genuinely stuck, not merely un-applied; stop relaunching.
const RELAUNCH_WINDOW_SECS: f64 = 600.0;
const HOTKEY_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const MIC_PREVIEW: Duration = Duration::from_secs(3);
const FLASH_DEFAULT: Duration = Duration::from_millis(1100);
const FLASH_SETTING_UP: Duration = Duration::from_millis(1400);
const FLASH_MIC_ERROR: Duration = Duration::from_secs(2);
const FLASH_ENGINE_ERROR: Duration = Duration::from_millis(2200);
/// Overlay preview of a pasted dictation: this many characters then "…".
const TYPED_PREVIEW_CHARS: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Recording,
    Transcribing,
}

/// Tray menu items whose text/enabled state changes at runtime.
pub struct TrayItems {
    pub title: MenuItem<tauri::Wry>,
    pub status: MenuItem<tauri::Wry>,
    pub copy_last: MenuItem<tauri::Wry>,
}

pub struct App {
    pub handle: AppHandle,
    pub settings: Arc<Settings>,
    pub history: Mutex<HistoryStore>,
    pub snippets: Mutex<SnippetStore>,
    pub recorder: Mutex<Recorder>,
    pub hotkeys: HotkeyController,
    pub downloader: ModelDownloader,
    pub engine: Mutex<Option<Arc<WhisperEngine>>>,
    pub state: Mutex<State>,
    pub preview_active: Mutex<bool>,
    pub setup_progress: Mutex<Option<f64>>,
    pub setup_failed: Mutex<bool>,
    pub overlay: Overlay,
    pub tray_items: Mutex<Option<TrayItems>>,
    main_visible: AtomicBool,
    onboarding_visible: AtomicBool,
    /// Set once the onboarding page has been shown, so later shows can
    /// reload it (the Swift wizard restarted at step 1 on every `show()`).
    onboarding_shown: AtomicBool,
    /// True while the 1 s hotkey retry thread is alive (one at a time).
    hotkey_retry_running: AtomicBool,
    /// Bumped per mic preview so a stale 3 s timer cannot stop a newer one.
    preview_generation: AtomicU64,
    /// Hotkey events are queued here and drained by the dictation worker.
    hotkey_tx: Mutex<Option<mpsc::Sender<HotkeyEvent>>>,
    /// Mirror of `state == Recording` for the hook thread's cancel rule,
    /// which must answer without taking `state`.
    recording: Arc<AtomicBool>,
}

/// Everything `status_info()` reads, gathered as plain values so the
/// assembly is testable without a running app.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusSnapshot {
    pub mic_denied: bool,
    pub tap_running: bool,
    pub ax_trusted: bool,
    /// `lastAXRelaunch` setting (unix seconds), if ever set.
    pub last_ax_relaunch: Option<f64>,
    pub now: f64,
    pub onboarding_visible: bool,
    pub setup_progress: Option<f64>,
    pub setup_failed: bool,
    /// `(ready, status_text)` of the engine, or `None` before one exists.
    pub engine: Option<(bool, String)>,
    pub hotkey: Hotkey,
}

/// Swift: `Date().timeIntervalSince1970 - defaults.double(forKey:) < 600`,
/// where a missing key reads as 0 (so "never relaunched" is false).
pub fn recently_relaunched(last_ax_relaunch: Option<f64>, now: f64) -> bool {
    now - last_ax_relaunch.unwrap_or(0.0) < RELAUNCH_WINDOW_SECS
}

pub fn status_inputs(s: &StatusSnapshot) -> StatusInputs {
    let (engine_ready, engine_status_text) = match &s.engine {
        Some((ready, text)) => (*ready, text.clone()),
        None => (false, String::new()),
    };
    StatusInputs {
        mic_denied: s.mic_denied,
        tap_running: s.tap_running,
        ax_trusted: s.ax_trusted,
        recently_relaunched: recently_relaunched(s.last_ax_relaunch, s.now),
        onboarding_visible: s.onboarding_visible,
        setup_progress: s.setup_progress,
        setup_failed: s.setup_failed,
        engine_exists: s.engine.is_some(),
        engine_ready,
        engine_status_text,
        hotkey_label: s.hotkey.label(),
        platform: Platform::current(),
    }
}

/// "Typed: <first 24 chars>…" — the overlay confirmation after a paste.
pub fn typed_flash(text: &str) -> String {
    let snippet = if text.chars().count() > TYPED_PREVIEW_CHARS {
        let head: String = text.chars().take(TYPED_PREVIEW_CHARS).collect();
        format!("{head}…")
    } else {
        text.to_string()
    };
    format!("Typed: {snippet}")
}

/// A recording is kept only when both the wall clock and the sample count
/// say it was a deliberate hold.
pub fn recording_long_enough(duration: Duration, sample_count: usize) -> bool {
    duration >= MIN_RECORDING && sample_count > MIN_SAMPLES
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A poisoned mutex here means a worker panicked mid-update; the data is
/// plain values, so carrying on with it beats taking the whole app down.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    /// Builds stores, wires the downloader/engine/hotkey callbacks and kicks off
    /// model discovery (download base.en if nothing is installed).
    pub fn new(handle: AppHandle) -> Arc<App> {
        let dir = Store::dir();
        let settings = Arc::new(Settings::in_dir(&dir));
        let history = HistoryStore::new(dir.clone(), settings.clone());
        let snippets = SnippetStore::new(dir);

        let hotkey_settings = settings.clone();
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        let recording = Arc::new(AtomicBool::new(false));
        let is_active = recording.clone();
        let hotkeys = HotkeyController::new(
            Arc::new(move || Config::hotkey(&hotkey_settings)),
            Arc::new(move || is_active.load(Ordering::SeqCst)),
        );

        let app = Arc::new(App {
            overlay: Overlay::new(handle.clone()),
            handle,
            settings,
            history: Mutex::new(history),
            snippets: Mutex::new(snippets),
            recorder: Mutex::new(Recorder::new()),
            hotkeys,
            downloader: ModelDownloader::new(),
            engine: Mutex::new(None),
            state: Mutex::new(State::Idle),
            preview_active: Mutex::new(false),
            setup_progress: Mutex::new(None),
            setup_failed: Mutex::new(false),
            tray_items: Mutex::new(None),
            main_visible: AtomicBool::new(false),
            onboarding_visible: AtomicBool::new(false),
            onboarding_shown: AtomicBool::new(false),
            hotkey_retry_running: AtomicBool::new(false),
            preview_generation: AtomicU64::new(0),
            hotkey_tx: Mutex::new(Some(tx)),
            recording,
        });

        // Hook thread → channel → worker, so the hook never blocks and Down
        // is always applied before the matching Up. The worker holds only a
        // weak reference and ends when `shutdown` drops the sender.
        {
            let weak = Arc::downgrade(&app);
            app.hotkeys.set_on_event(Arc::new(move |event| {
                if let Some(app) = weak.upgrade() {
                    if let Some(tx) = lock(&app.hotkey_tx).as_ref() {
                        let _ = tx.send(event);
                    }
                }
            }));
            let weak = Arc::downgrade(&app);
            let worker = std::thread::Builder::new()
                .name("voice-dictation".into())
                .spawn(move || {
                    for event in rx {
                        let Some(app) = weak.upgrade() else { break };
                        // One bad event must not end the loop: hold-to-talk
                        // would be dead for the rest of the session.
                        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| match event {
                            HotkeyEvent::Down => app.begin_recording(),
                            HotkeyEvent::Up => app.end_recording(),
                            HotkeyEvent::Cancel => app.cancel_recording(),
                        }));
                        if outcome.is_err() {
                            log::error!("dictation worker: {event:?} panicked; resetting");
                            app.recover_from_panic();
                        }
                    }
                });
            if let Err(e) = worker {
                // No hold-to-talk this session, but the windows and tray
                // still work; a panic here would take the whole app down.
                log::error!("spawn dictation worker: {e}");
            }
        }

        {
            let weak = Arc::downgrade(&app);
            app.downloader.set_on_progress(Box::new(move |_, p| {
                if let Some(app) = weak.upgrade() {
                    *lock(&app.setup_progress) = Some(p);
                    app.refresh_ui();
                }
            }));
            let weak = Arc::downgrade(&app);
            app.downloader.set_on_finished(Box::new(move |_, path| {
                if let Some(app) = weak.upgrade() {
                    *lock(&app.setup_progress) = None;
                    match path {
                        Some(model) => app.start_engine(model),
                        None => *lock(&app.setup_failed) = true,
                    }
                    app.refresh_ui();
                }
            }));
        }

        match Config::find_model(&app.settings) {
            Some(model) => app.start_engine(model),
            None => {
                *lock(&app.setup_progress) = Some(0.0);
                app.downloader.download(ModelCatalog::default_spec());
            }
        }

        app
    }

    /// Called once the Tauri setup hook runs: start the hotkey hook (with a
    /// retry timer while Accessibility is missing), then show onboarding or the
    /// main window depending on the `onboarded` setting.
    pub fn launch(self: &Arc<Self>) {
        if self.settings.get_bool("onboarded").unwrap_or(false) {
            platform::request_mic();
            self.ensure_event_tap(true);
            self.show_main_window();
        } else {
            // First run: the onboarding flow drives permission prompts itself.
            self.ensure_event_tap(false);
            self.show_onboarding();
        }
    }

    // MARK: engine

    fn start_engine(self: &Arc<Self>, model: std::path::PathBuf) {
        let previous = lock(&self.engine).take();
        if let Some(old) = previous {
            old.stop();
        }
        let engine = Arc::new(WhisperEngine::new(Some(model), Config::SERVER_PORT, 120));
        let weak = Arc::downgrade(self);
        engine.set_on_status_change(Box::new(move || {
            if let Some(app) = weak.upgrade() {
                app.refresh_ui();
            }
        }));
        *lock(&self.engine) = Some(engine.clone());
        engine.start();
        self.refresh_ui();
    }

    fn engine(&self) -> Option<Arc<WhisperEngine>> {
        lock(&self.engine).clone()
    }

    // MARK: status

    fn recently_auto_relaunched(&self) -> bool {
        recently_relaunched(self.settings.get_f64("lastAXRelaunch"), unix_now())
    }

    fn status_snapshot(&self) -> StatusSnapshot {
        let engine = self.engine().map(|e| (e.ready(), e.status_text()));
        StatusSnapshot {
            mic_denied: platform::mic_status() == MicStatus::Denied,
            tap_running: self.hotkeys.tap_running(),
            ax_trusted: platform::accessibility_trusted(),
            last_ax_relaunch: self.settings.get_f64("lastAXRelaunch"),
            now: unix_now(),
            onboarding_visible: self.onboarding_visible(),
            setup_progress: *lock(&self.setup_progress),
            setup_failed: *lock(&self.setup_failed),
            engine,
            hotkey: Config::hotkey(&self.settings),
        }
    }

    pub fn status_info(&self) -> StatusInfo {
        compute_status(&status_inputs(&self.status_snapshot()))
    }

    /// Refresh tray menu titles and notify open windows (`status-changed`).
    pub fn refresh_ui(&self) {
        tray::refresh(self);
        let _ = self.handle.emit("status-changed", ());
    }

    // MARK: windows

    /// macOS: a Dock icon only while a real window is up (LSUIElement-style
    /// accessory the rest of the time). No-op elsewhere.
    fn apply_activation_policy(&self) {
        #[cfg(target_os = "macos")]
        {
            let visible = self.main_window_visible() || self.onboarding_visible();
            let policy = if visible {
                tauri::ActivationPolicy::Regular
            } else {
                tauri::ActivationPolicy::Accessory
            };
            if let Err(e) = self.handle.set_activation_policy(policy) {
                log::warn!("activation policy: {e}");
            }
        }
    }

    fn show_window(&self, label: &str) {
        let Some(window) = self.handle.get_webview_window(label) else {
            log::error!("window {label} missing");
            return;
        };
        if let Err(e) = window.show() {
            log::warn!("show {label}: {e}");
        }
        let _ = window.set_focus();
    }

    pub fn show_main_window(&self) {
        self.main_visible.store(true, Ordering::SeqCst);
        self.apply_activation_policy();
        self.show_window(MAIN_WINDOW);
    }

    pub fn show_onboarding(&self) {
        self.onboarding_visible.store(true, Ordering::SeqCst);
        self.apply_activation_policy();
        // The window is hidden, never destroyed, so onboarding.js would
        // resume wherever it was left (the "done" step after a finished
        // run). Reload it so `init()` starts at step 1 like the Swift
        // `OnboardingWindow.show()`; the first show is still loading.
        if self.onboarding_shown.swap(true, Ordering::SeqCst) {
            if let Some(window) = self.handle.get_webview_window(ONBOARDING_WINDOW) {
                if let Err(e) = window.eval("location.reload()") {
                    log::warn!("onboarding reload: {e}");
                }
            }
        }
        self.show_window(ONBOARDING_WINDOW);
        self.refresh_ui();
    }

    /// Dock icon click / relaunch while running
    /// (`applicationShouldHandleReopen`).
    pub fn reopen(&self) {
        if self.onboarding_visible() || !self.settings.get_bool("onboarded").unwrap_or(false) {
            self.show_onboarding();
        } else {
            self.show_main_window();
        }
    }

    /// The close-box handler for `main` / `onboarding`: the window is hidden,
    /// never destroyed (the process lives in the tray). Mirrors
    /// `windowWillClose` in ui.swift, including "closing onboarding counts as
    /// having been onboarded".
    pub fn hide_window(&self, label: &str) {
        match label {
            MAIN_WINDOW => self.main_visible.store(false, Ordering::SeqCst),
            ONBOARDING_WINDOW => {
                self.onboarding_visible.store(false, Ordering::SeqCst);
                self.settings.set("onboarded", true);
            }
            _ => return,
        }
        if let Some(window) = self.handle.get_webview_window(label) {
            if let Err(e) = window.hide() {
                log::warn!("hide {label}: {e}");
            }
        }
        self.apply_activation_policy();
        if label == ONBOARDING_WINDOW {
            self.refresh_ui();
        }
    }

    pub fn onboarding_visible(&self) -> bool {
        self.onboarding_visible.load(Ordering::SeqCst)
    }

    pub fn main_window_visible(&self) -> bool {
        self.main_visible.load(Ordering::SeqCst)
    }

    pub fn hotkeys_running(&self) -> bool {
        self.hotkeys.tap_running()
    }

    pub fn mic_level(&self) -> f32 {
        lock(&self.recorder).level()
    }

    fn state_is(&self, state: State) -> bool {
        *lock(&self.state) == state
    }

    /// Moves Idle → Recording unless a mic preview owns the device. The
    /// preview flag is read under the `state` lock (the one place the
    /// mutexes nest, in the documented order) so this and `claim_preview`
    /// exclude each other across the worker and main threads.
    fn claim_recording(&self) -> bool {
        let mut state = lock(&self.state);
        if *state != State::Idle || *lock(&self.preview_active) {
            return false;
        }
        *state = State::Recording;
        self.recording.store(true, Ordering::SeqCst);
        true
    }

    /// Counterpart of `claim_recording` for the Settings mic test.
    fn claim_preview(&self) -> bool {
        let state = lock(&self.state);
        if *state != State::Idle {
            return false;
        }
        let mut active = lock(&self.preview_active);
        if *active {
            return false;
        }
        *active = true;
        true
    }

    fn set_state(&self, state: State) {
        *lock(&self.state) = state;
        self.recording
            .store(state == State::Recording, Ordering::SeqCst);
    }

    fn level_provider(self: &Arc<Self>) -> Box<dyn Fn() -> f32 + Send + Sync> {
        let weak: Weak<App> = Arc::downgrade(self);
        Box::new(move || weak.upgrade().map(|app| app.mic_level()).unwrap_or(0.0))
    }

    // MARK: permissions

    pub fn request_accessibility(&self) {
        platform::request_accessibility();
    }

    fn ensure_event_tap(self: &Arc<Self>, prompt: bool) {
        if prompt {
            self.request_accessibility();
        }
        if self.hotkeys.start() {
            self.refresh_ui();
            return;
        }
        if self.hotkey_retry_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        let retry = std::thread::Builder::new()
            .name("voice-hotkey-retry".into())
            .spawn(move || loop {
                std::thread::sleep(HOTKEY_RETRY_INTERVAL);
                let Some(app) = weak.upgrade() else { break };
                if app.hotkeys.start() {
                    app.hotkey_retry_running.store(false, Ordering::SeqCst);
                    app.refresh_ui();
                    break;
                }
                if platform::accessibility_trusted() {
                    app.auto_relaunch_if_needed();
                    app.refresh_ui();
                }
            });
        if let Err(e) = retry {
            log::error!("spawn hotkey retry: {e}");
            // Let the next `ensure_event_tap` try again.
            self.hotkey_retry_running.store(false, Ordering::SeqCst);
        }
    }

    /// macOS applies a fresh Accessibility grant only to a new process. Do it
    /// once per 10 minutes so a grant that still does not take (stale TCC
    /// entry) does not loop forever, and never while onboarding is up.
    fn auto_relaunch_if_needed(&self) {
        if self.recently_auto_relaunched() || self.onboarding_visible() {
            return;
        }
        self.settings.set("lastAXRelaunch", unix_now());
        platform::relaunch_self();
        self.handle.exit(0);
    }

    // MARK: mic preview

    /// 3-second microphone test from Settings: shows the listening overlay.
    pub fn preview_mic(self: &Arc<Self>) {
        // Claim the preview before touching the device: `begin_recording`
        // runs on the dictation worker, so a hotkey Down between `start()`
        // and the claim would start a dictation that the 3 s timer below
        // then cancels out from under the user.
        if !self.claim_preview() {
            return;
        }
        let generation = self.preview_generation.fetch_add(1, Ordering::SeqCst) + 1;
        // Bind first so the guard is released at the `;`: an `if let` on the
        // locked expression would hold `recorder` across `overlay.flash`,
        // which round-trips to the main thread.
        let started = lock(&self.recorder).start();
        if let Err(e) = started {
            *lock(&self.preview_active) = false;
            self.overlay
                .flash(&format!("Mic error: {e}"), FLASH_MIC_ERROR);
            return;
        }
        self.overlay.show_listening(self.level_provider());
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(MIC_PREVIEW);
            let Some(app) = weak.upgrade() else { return };
            if app.preview_generation.load(Ordering::SeqCst) != generation {
                return;
            }
            {
                let mut active = lock(&app.preview_active);
                if !*active {
                    return;
                }
                *active = false;
            }
            lock(&app.recorder).cancel();
            app.overlay.hide();
        });
    }

    // MARK: recording flow

    pub fn begin_recording(self: &Arc<Self>) {
        if !self.claim_recording() {
            return;
        }
        if self.engine().is_none() {
            self.set_state(State::Idle);
            self.overlay
                .flash("Voice is still setting up", FLASH_SETTING_UP);
            return;
        }
        // Bind first so the guard is released at the `;`: an `if let` on the
        // locked expression would hold `recorder` across `overlay.flash`,
        // which round-trips to the main thread.
        let started = lock(&self.recorder).start();
        if let Err(e) = started {
            self.set_state(State::Idle);
            self.overlay
                .flash(&format!("Mic error: {e}"), FLASH_MIC_ERROR);
            return;
        }
        tray::set_recording(&self.handle, true);
        // Show first: the sound is played by the overlay webview, which may
        // be throttled while its window is hidden.
        self.overlay.show_listening(self.level_provider());
        if Config::sounds_enabled(&self.settings) {
            self.overlay.play_sound("start");
        }
    }

    pub fn end_recording(self: &Arc<Self>) {
        if !self.state_is(State::Recording) {
            return;
        }
        let (duration, samples) = {
            let mut recorder = lock(&self.recorder);
            let duration = recorder.duration();
            (duration, recorder.stop())
        };
        tray::set_recording(&self.handle, false);

        if !recording_long_enough(duration, samples.len()) {
            self.set_state(State::Idle);
            self.overlay.hide();
            return;
        }

        self.set_state(State::Transcribing);
        self.overlay.show_processing();
        let wav = wav_data(&samples);
        let sent_at = Instant::now();
        let Some(engine) = self.engine() else {
            // Cannot happen (begin_recording checked) unless the engine was
            // swapped mid-hold; do not strand the state machine.
            self.set_state(State::Idle);
            self.overlay.hide();
            return;
        };

        let app = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("voice-transcribe".into())
            .spawn(move || {
                let result = engine.transcribe(&wav);
                app.set_state(State::Idle);
                match result {
                    Err(error) => {
                        app.overlay
                            .flash(&format!("Error: {error}"), FLASH_ENGINE_ERROR);
                    }
                    Ok(raw) => app.deliver(raw, duration.as_secs_f64(), sent_at),
                }
            });
        if let Err(e) = spawned {
            // Do not strand the state machine in Transcribing (and never
            // panic on the dictation worker).
            log::error!("spawn transcription: {e}");
            self.set_state(State::Idle);
            self.overlay
                .flash(&format!("Error: {e}"), FLASH_ENGINE_ERROR);
        }
    }

    /// After a panic in begin/end/cancel: drop whatever was recording and go
    /// back to Idle so the next hold works.
    fn recover_from_panic(&self) {
        lock(&self.recorder).cancel();
        self.set_state(State::Idle);
        tray::set_recording(&self.handle, false);
        self.overlay.hide();
    }

    /// Everything after a successful transcription: cleanup, snippets,
    /// history, paste, confirmation.
    fn deliver(&self, raw: String, duration: f64, sent_at: Instant) {
        let cleaned = clean_transcript(&raw);
        if cleaned.is_empty() {
            self.overlay.flash("No speech detected", FLASH_DEFAULT);
            return;
        }
        let text = lock(&self.snippets).expand(&cleaned);
        let latency = sent_at.elapsed().as_secs_f64();
        lock(&self.history).add(DictationEntry::new(
            text.clone(),
            SystemTime::now(),
            duration,
            latency,
        ));
        if Config::trailing_space(&self.settings) {
            paste_text(&format!("{text} "));
        } else {
            paste_text(&text);
        }
        if Config::sounds_enabled(&self.settings) {
            self.overlay.play_sound("done");
        }
        self.overlay.flash(&typed_flash(&text), FLASH_DEFAULT);
        let _ = self.handle.emit("dictation-landed", ());
        let _ = self.handle.emit("history-changed", ());
        self.refresh_ui();
    }

    pub fn cancel_recording(&self) {
        if !self.state_is(State::Recording) {
            return;
        }
        lock(&self.recorder).cancel();
        self.set_state(State::Idle);
        tray::set_recording(&self.handle, false);
        self.overlay.hide();
    }

    // MARK: menu actions

    pub fn copy_last(&self) {
        let last = lock(&self.history)
            .entries()
            .first()
            .map(|e| e.text.clone());
        if let Some(text) = last {
            copy_text(&text);
        }
    }

    pub fn finish_onboarding(&self) {
        self.settings.set("onboarded", true);
        self.hide_window(ONBOARDING_WINDOW);
        self.show_main_window();
    }

    pub fn shutdown(&self) {
        lock(&self.hotkey_tx).take();
        // Drop the guard before `stop()` (kill + wait): the whisper supervisor
        // thread may be in `refresh_ui` waiting for `engine`.
        let engine = lock(&self.engine).take();
        if let Some(engine) = engine {
            engine.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_flash_keeps_short_text_whole() {
        assert_eq!(typed_flash("hello world"), "Typed: hello world");
        // Exactly 24 characters is not truncated (Swift: `count > 24`).
        let exact = "abcdefghijklmnopqrstuvwx";
        assert_eq!(exact.chars().count(), 24);
        assert_eq!(typed_flash(exact), format!("Typed: {exact}"));
    }

    #[test]
    fn typed_flash_truncates_at_24_chars_with_ellipsis() {
        let long = "the quick brown fox jumps over the lazy dog";
        assert_eq!(typed_flash(long), "Typed: the quick brown fox jump…");
        // Counted in characters, not bytes, so multi-byte text is not cut
        // mid-character.
        let accented = "ééééééééééééééééééééééééé";
        assert_eq!(
            typed_flash(accented),
            format!("Typed: {}…", &accented[..48])
        );
    }

    #[test]
    fn recording_guard_needs_both_duration_and_samples() {
        assert!(recording_long_enough(Duration::from_millis(350), 4001));
        assert!(!recording_long_enough(Duration::from_millis(349), 4001));
        assert!(!recording_long_enough(Duration::from_millis(350), 4000));
        assert!(!recording_long_enough(Duration::from_secs(2), 0));
    }

    #[test]
    fn recently_relaunched_window_is_600s_and_missing_key_is_never() {
        assert!(!recently_relaunched(None, 1_700_000_000.0));
        assert!(recently_relaunched(Some(1_000.0), 1_599.0));
        assert!(!recently_relaunched(Some(1_000.0), 1_600.0));
    }

    fn snapshot() -> StatusSnapshot {
        StatusSnapshot {
            mic_denied: false,
            tap_running: true,
            ax_trusted: true,
            last_ax_relaunch: None,
            now: 10_000.0,
            onboarding_visible: false,
            setup_progress: None,
            setup_failed: false,
            engine: Some((true, "ready".into())),
            hotkey: Hotkey::RightOption,
        }
    }

    #[test]
    fn status_inputs_carry_every_field_through() {
        let s = StatusSnapshot {
            mic_denied: true,
            tap_running: false,
            ax_trusted: false,
            last_ax_relaunch: Some(9_900.0),
            onboarding_visible: true,
            setup_progress: Some(0.42),
            setup_failed: true,
            engine: Some((false, "loading model…".into())),
            hotkey: Hotkey::RightCommand,
            ..snapshot()
        };
        let i = status_inputs(&s);
        assert!(i.mic_denied);
        assert!(!i.tap_running);
        assert!(!i.ax_trusted);
        assert!(i.recently_relaunched);
        assert!(i.onboarding_visible);
        assert_eq!(i.setup_progress, Some(0.42));
        assert!(i.setup_failed);
        assert!(i.engine_exists);
        assert!(!i.engine_ready);
        assert_eq!(i.engine_status_text, "loading model…");
        assert_eq!(i.hotkey_label, Hotkey::RightCommand.label());
        assert_eq!(i.platform, Platform::current());
    }

    #[test]
    fn status_inputs_without_engine_report_missing_not_ready() {
        let s = StatusSnapshot {
            engine: None,
            ..snapshot()
        };
        let i = status_inputs(&s);
        assert!(!i.engine_exists);
        assert!(!i.engine_ready);
        assert_eq!(i.engine_status_text, "");
        assert!(!i.recently_relaunched);
        // And the healthy snapshot maps to the green "Ready" line.
        let ready = compute_status(&status_inputs(&snapshot()));
        assert!(ready.text.starts_with("Ready — hold "));
    }
}
