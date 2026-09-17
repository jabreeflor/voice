//! Application state and the dictation flow. Reference: `AppDelegate` in
//! Sources/VoiceCore/app.swift.
//!
//! One `App` lives in Tauri managed state (`app_handle.state::<App>()`). All
//! fields that change are behind mutexes so hotkey, audio, engine and UI
//! threads can share it. The main-thread-only work (window show/hide,
//! activation policy) goes through `tauri::AppHandle::run_on_main_thread`.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use tauri::AppHandle;
use voice_core::{
    HistoryStore, ModelDownloader, Settings, SnippetStore, StatusInfo, WhisperEngine,
};

use crate::audio::Recorder;
use crate::hotkey::HotkeyController;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Recording,
    Transcribing,
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
}

impl App {
    /// Builds stores, wires the downloader/engine/hotkey callbacks and kicks off
    /// model discovery (download base.en if nothing is installed).
    pub fn new(handle: AppHandle) -> Arc<App> {
        let _ = handle;
        todo!()
    }

    /// Called once the Tauri setup hook runs: start the hotkey hook (with a
    /// retry timer while Accessibility is missing), then show onboarding or the
    /// main window depending on the `onboarded` setting.
    pub fn launch(self: &Arc<Self>) {
        todo!()
    }

    pub fn status_info(&self) -> StatusInfo {
        todo!()
    }

    /// Refresh tray menu titles and notify open windows (`status-changed`).
    pub fn refresh_ui(&self) {
        todo!()
    }

    pub fn show_main_window(&self) {
        todo!()
    }

    pub fn show_onboarding(&self) {
        todo!()
    }

    pub fn onboarding_visible(&self) -> bool {
        todo!()
    }

    pub fn main_window_visible(&self) -> bool {
        todo!()
    }

    pub fn hotkeys_running(&self) -> bool {
        todo!()
    }

    pub fn mic_level(&self) -> f32 {
        todo!()
    }

    /// 3-second microphone test from Settings: shows the listening overlay.
    pub fn preview_mic(&self) {
        todo!()
    }

    pub fn request_accessibility(&self) {
        todo!()
    }

    pub fn begin_recording(self: &Arc<Self>) {
        todo!()
    }

    pub fn end_recording(self: &Arc<Self>) {
        todo!()
    }

    pub fn cancel_recording(&self) {
        todo!()
    }

    pub fn copy_last(&self) {
        todo!()
    }

    pub fn finish_onboarding(&self) {
        todo!()
    }

    pub fn shutdown(&self) {
        todo!()
    }
}
