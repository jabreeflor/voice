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
//! - `updates`   self-update from the GitHub release feed (release builds)
//! - `platform`  OS-specific permission and settings-pane shims
//! - `single_instance`  one running Voice per user (lock + "show yourself")

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod audio;
mod commands;
mod hotkey;
mod overlay;
mod paste;
mod platform;
mod single_instance;
mod tray;
mod updates;

use std::sync::Arc;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use voice_core::{Config, Store};

use crate::app::{App, MAIN_WINDOW, ONBOARDING_WINDOW};
use crate::single_instance::{InstanceGuard, Launch};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Before Tauri, the talk-key hook and whisper-server: a second copy must
    // not get far enough to hook the key or contend for port 8178.
    let instance: Option<Arc<InstanceGuard>> =
        match single_instance::acquire_in(&Store::dir(), single_instance::DEFAULT_WAIT) {
            Ok(Launch::Primary(guard)) => Some(Arc::new(guard)),
            Ok(Launch::Signalled) => {
                log::info!("Voice is already running; showed its window");
                return;
            }
            Ok(Launch::TimedOut) => {
                log::error!("another Voice holds the instance lock and never let go");
                std::process::exit(1);
            }
            // An unwritable data dir breaks history and settings too; run
            // rather than refuse to start over a lock file.
            Err(e) => {
                log::warn!("single-instance guard unavailable, starting anyway: {e}");
                None
            }
        };
    let setup_instance = instance.clone();

    let context = tauri::generate_context!();
    let builder = tauri::Builder::default().plugin(tauri_plugin_autostart::init(
        MacosLauncher::LaunchAgent,
        None,
    ));
    // Only a release build (`--config tauri.release.conf.json`) carries the
    // feed and pubkey; the plugin refuses to start without them, and a local
    // build must never replace itself with what is on main (updates.rs).
    #[cfg(target_os = "macos")]
    let builder = if updates::feed_configured(context.config()) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    let tauri_app = builder
        .invoke_handler(commands::handler())
        .setup(move |app| {
            // Menu-bar accessory until a window is shown (LSUIElement in the
            // Swift bundle). The setup hook runs on the main thread, so
            // menu/tray construction below happens inline.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let state = App::new(app.handle().clone());
            commands::apply_appearance(app.handle(), Config::appearance(&state.settings));
            state.overlay.configure_window();
            app.manage(state.clone());
            // Before the tray: it adds Check for Updates… only when this
            // build updates.
            updates::start(app.handle());
            tray::build(app.handle(), state.clone())?;
            state.launch();
            // Reopening Voice from Finder, the Dock, Spotlight, a second
            // install or the command line lands here (same-bundle reopens
            // still arrive as `RunEvent::Reopen`).
            if let Some(guard) = &setup_instance {
                let handle = app.handle().clone();
                guard.on_show(move || {
                    let h = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Some(app) = h.try_state::<Arc<App>>() {
                            app.reopen();
                        }
                    });
                });
            }
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
        .build(context)
        .expect("error while building Voice");

    tauri_app.run(move |handle, event| match event {
        // `code: None` is tao's "last window went away" exit; a tray app has
        // no windows most of the time, so that must not end the process.
        // Programmatic exits (tray Quit, the Accessibility relaunch) carry a
        // code and go through.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Quit, the Accessibility relaunch and Restart to Update: a copy
        // launched from now on waits for our lock instead of showing us.
        RunEvent::ExitRequested { .. } => {
            if let Some(guard) = &instance {
                guard.mark_closing();
            }
        }
        RunEvent::Exit => {
            if let Some(app) = handle.try_state::<Arc<App>>() {
                app.shutdown();
            }
            // Quitting with an update staged installs it, so the next launch
            // is the new version even without Restart to Update.
            updates::install_pending(handle);
            // Last: whisper-server is stopped and any update is installed,
            // so a relaunched copy can start its own right away.
            if let Some(guard) = &instance {
                guard.release();
            }
        }
        // Dock icon click / second launch: `applicationShouldHandleReopen` in
        // the Swift app (bring back onboarding if it is up or never finished,
        // otherwise the main window).
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if let Some(app) = handle.try_state::<Arc<App>>() {
                app.reopen();
            }
        }
        _ => {}
    });
}
