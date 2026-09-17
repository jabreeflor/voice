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

use std::sync::Arc;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::app::{App, MAIN_WINDOW, ONBOARDING_WINDOW};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let tauri_app = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(commands::handler())
        .setup(|app| {
            // Menu-bar accessory until a window is shown (LSUIElement in the
            // Swift bundle). The setup hook runs on the main thread, so
            // menu/tray construction below happens inline.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let state = App::new(app.handle().clone());
            app.manage(state.clone());
            tray::build(app.handle(), state.clone())?;
            state.launch();
            Ok(())
        })
        .on_window_event(|window, event| {
            // The close box hides `main` / `onboarding`; the process lives on
            // in the tray. `overlay` is `closable: false` and never gets here.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if label == MAIN_WINDOW || label == ONBOARDING_WINDOW {
                    api.prevent_close();
                    if let Some(app) = window.app_handle().try_state::<Arc<App>>() {
                        app.hide_window(label);
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Voice");

    tauri_app.run(|handle, event| match event {
        // `code: None` is tao's "last window went away" exit; a tray app has
        // no windows most of the time, so that must not end the process.
        // Programmatic exits (tray Quit, the Accessibility relaunch) carry a
        // code and go through.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(app) = handle.try_state::<Arc<App>>() {
                app.shutdown();
            }
        }
        _ => {}
    });
}
