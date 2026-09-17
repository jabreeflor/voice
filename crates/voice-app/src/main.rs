//! voice — native tray app for local hold-to-talk dictation.
//!
//! Entry point: builds the Tauri app, registers plugins, tray, windows and the
//! `App` state, then hands control to the event loop. Module map:
//! - `app`       state machine + recording flow (the AppDelegate of the Swift app)
//! - `audio`     microphone capture → 16 kHz mono f32
//! - `hotkey`    hold-to-talk key hook
//! - `paste`     clipboard + synthesized paste keystroke
//! - `tray`      status-bar icon and menu
//! - `overlay`   floating pill window driver
//! - `commands`  #[tauri::command] handlers for the HTML UI
//! - `platform`  OS-specific permission and settings-pane shims

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod audio;
mod commands;
mod hotkey;
mod overlay;
mod paste;
mod platform;
mod tray;

fn main() {
    todo!("main: build tauri app (see SPEC)")
}
