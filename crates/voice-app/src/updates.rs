//! Self-update from the GitHub release feed.
//!
//! Every push to `main` publishes a release (`.github/workflows/release.yml`)
//! whose `latest.json` lists a signed `Voice_<v>_<arch>.app.tar.gz` per Mac
//! architecture. This module is the half that makes an installed Voice notice:
//!
//! 1. A minute after launch, then every four hours, and whenever the user
//!    picks **Check for Updates…** (tray menu or Settings), ask the feed.
//! 2. A newer version is downloaded into memory and its signature checked
//!    against the pubkey compiled into this binary. Nothing on disk changes.
//! 3. The tray item turns into **Restart to Update to x.y.z**, and the main
//!    window shows a banner (`app-update` event). Restarting installs and
//!    relaunches; quitting installs on the way out, so the next launch is the
//!    new version either way.
//!
//! Staging in memory instead of installing at once is deliberate: the install
//! swaps the whole `.app`, and a running Voice whose `Contents/MacOS` (the
//! `whisper-server` sidecar it respawns) changed underneath it would be a
//! mix of two versions.
//!
//! Only release builds update. The feed URL and pubkey live in
//! `tauri.release.conf.json`, which only the release workflow builds with,
//! so `build.sh`, `scripts/install.sh` and `cargo tauri dev` builds have no
//! `plugins.updater` config, never register the plugin, and never replace
//! themselves with whatever is on `main`. macOS only: the Windows and Linux
//! bundles are not published.
//!
//! Privacy: the check is a plain HTTPS GET of `latest.json` (and, when newer,
//! the archive) from github.com. Nothing about the user or their dictations
//! is sent.

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Event carrying an [`UpdateStatus`] to every window.
pub const UPDATE_EVENT: &str = "app-update";

#[cfg(target_os = "macos")]
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
#[cfg(target_os = "macos")]
const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);
#[cfg(target_os = "macos")]
const FLASH_UPDATE: Duration = Duration::from_millis(2200);

/// What the tray item, the Settings row and the main-window banner show.
///
/// `manual` marks the result of a check the user asked for. A background
/// check that finds nothing, or fails because the laptop is offline, is not
/// worth interrupting anyone for; one the user asked for always answers.
///
/// Off macOS only `Disabled` is ever built; the rest still has to compile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum UpdateStatus {
    Disabled { reason: String },
    Idle,
    Checking { manual: bool },
    Current { version: String, manual: bool },
    Ready { version: String, current: String },
    Failed { message: String, manual: bool },
}

/// Whether the compiled-in config names an update feed: true for builds made
/// with `--config tauri.release.conf.json`, false for every local build.
/// Registering the plugin without it would fail at startup (`pubkey` is a
/// required field of the plugin's config).
pub fn feed_configured(config: &tauri::Config) -> bool {
    config.plugins.0.contains_key("updater")
}

/// Why this process must not self-update, or `None` when it should.
pub fn disabled_reason(macos: bool, feed: bool) -> Option<&'static str> {
    if !macos {
        return Some("Updates ship for macOS only");
    }
    if !feed {
        return Some("Built from source, updates are off");
    }
    None
}

/// The tray item for `status`: its title and whether it is clickable, or
/// `None` when this build does not update (no item at all).
pub fn menu_item(status: &UpdateStatus) -> Option<(String, bool)> {
    match status {
        UpdateStatus::Disabled { .. } => None,
        UpdateStatus::Checking { .. } => Some(("Checking for Updates…".into(), false)),
        UpdateStatus::Ready { version, .. } => {
            Some((format!("Restart to Update to {version}"), true))
        }
        UpdateStatus::Idle | UpdateStatus::Current { .. } | UpdateStatus::Failed { .. } => {
            Some(("Check for Updates…".into(), true))
        }
    }
}

/// What the overlay pill flashes once a check the user asked for finishes;
/// background checks stay silent. Short enough for the 192 px pill.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn result_flash(status: &UpdateStatus, manual: bool) -> Option<String> {
    if !manual {
        return None;
    }
    match status {
        UpdateStatus::Current { version, .. } => Some(format!("Voice {version} is up to date")),
        UpdateStatus::Ready { version, .. } => Some(format!("Voice {version} is ready")),
        UpdateStatus::Failed { .. } => Some("Couldn't check for updates".into()),
        _ => None,
    }
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    #[cfg(target_os = "macos")]
    busy: AtomicBool,
    #[cfg(target_os = "macos")]
    pending: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>,
}

impl Updates {
    fn new(status: UpdateStatus) -> Self {
        Self {
            status: Mutex::new(status),
            #[cfg(target_os = "macos")]
            busy: AtomicBool::new(false),
            #[cfg(target_os = "macos")]
            pending: Mutex::new(None),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// The current status, `Disabled` before [`start`] ran.
pub fn status(app: &AppHandle) -> UpdateStatus {
    app.try_state::<Updates>()
        .map(|u| u.status())
        .unwrap_or(UpdateStatus::Disabled {
            reason: "Not started".into(),
        })
}

/// Records `status`, retitles the tray item and tells every window. Never
/// called with an `App` mutex held: the tray round-trips to the main thread.
#[cfg(target_os = "macos")]
fn set_status(app: &AppHandle, status: UpdateStatus) {
    use tauri::Emitter;

    if let Some(updates) = app.try_state::<Updates>() {
        *updates.status.lock().unwrap_or_else(|e| e.into_inner()) = status.clone();
    }
    crate::tray::set_update_item(app, &status);
    if let Err(e) = app.emit(UPDATE_EVENT, status) {
        log::warn!("{UPDATE_EVENT}: {e}");
    }
}

/// Manages the state every command reads and, in a release build on macOS,
/// starts the periodic check. Runs in the setup hook before the tray is
/// built, so the tray knows whether to add its item.
pub fn start(app: &AppHandle) {
    let feed = feed_configured(app.config());
    match disabled_reason(cfg!(target_os = "macos"), feed) {
        Some(reason) => {
            log::info!("self-update off: {reason}");
            app.manage(Updates::new(UpdateStatus::Disabled {
                reason: reason.into(),
            }));
        }
        None => {
            app.manage(Updates::new(UpdateStatus::Idle));
            #[cfg(target_os = "macos")]
            spawn_periodic_check(app.clone());
        }
    }
}

#[cfg(target_os = "macos")]
fn spawn_periodic_check(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("voice-updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                tauri::async_runtime::block_on(check(app.clone(), false));
                std::thread::sleep(CHECK_EVERY);
            }
        });
    if let Err(e) = spawned {
        log::error!("spawn update checker: {e}");
    }
}

/// A check the user asked for (tray item, Settings button). Returns at once;
/// the result arrives as an `app-update` event.
pub fn check_now(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { check(app, true).await });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
async fn check(app: AppHandle, manual: bool) {
    use tauri_plugin_updater::UpdaterExt;

    let Some(updates) = app.try_state::<Updates>() else {
        return;
    };
    match updates.status() {
        UpdateStatus::Disabled { .. } => return,
        // Already staged: re-announce it rather than download it again.
        ready @ UpdateStatus::Ready { .. } => {
            if manual {
                flash(&app, &ready, true);
                set_status(&app, ready);
            }
            return;
        }
        _ => {}
    }
    if updates.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    set_status(&app, UpdateStatus::Checking { manual });

    let result: Result<_, tauri_plugin_updater::Error> = async {
        match app.updater()?.check().await? {
            None => Ok(None),
            Some(update) => {
                // Verified against the compiled-in pubkey before it returns.
                let bytes = update.download(|_, _| {}, || {}).await?;
                Ok(Some((update, bytes)))
            }
        }
    }
    .await;

    let status = match result {
        Ok(None) => UpdateStatus::Current {
            version: app.package_info().version.to_string(),
            manual,
        },
        Ok(Some((update, bytes))) => {
            let status = UpdateStatus::Ready {
                version: update.version.clone(),
                current: update.current_version.clone(),
            };
            *updates.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some((update, bytes));
            status
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            UpdateStatus::Failed {
                message: e.to_string(),
                manual,
            }
        }
    };
    flash(&app, &status, manual);
    set_status(&app, status);
    updates.busy.store(false, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn flash(app: &AppHandle, status: &UpdateStatus, manual: bool) {
    if let Some(text) = result_flash(status, manual) {
        if let Some(state) = app.try_state::<std::sync::Arc<crate::app::App>>() {
            state.overlay.flash(&text, FLASH_UPDATE);
        }
    }
}

/// Installs a staged update, if there is one. Called on the way out, so a
/// user who quits instead of restarting still launches the new version next
/// time. Returns whether anything was installed.
///
/// Safe on the main thread: when the bundle's folder is not writable the
/// plugin asks for an administrator password through AppleScript on the
/// main thread, and tauri runs a main-thread task inline when it is already
/// there.
pub fn install_pending(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(updates) = app.try_state::<Updates>() else {
            return false;
        };
        let staged = updates
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((update, bytes)) = staged {
            return match update.install(bytes) {
                Ok(()) => {
                    log::info!("installed Voice {}", update.version);
                    true
                }
                Err(e) => {
                    log::error!("install Voice {}: {e}", update.version);
                    false
                }
            };
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    false
}

/// Installs the staged update and relaunches into it.
pub fn restart(app: &AppHandle) -> Result<(), String> {
    if !install_pending(app) {
        return Err("No update is ready to install".into());
    }
    // `request_restart`, not `restart`: the normal exit path runs first, so
    // `App::shutdown` stops whisper-server before the new copy starts its own
    // on the same port.
    app.request_restart();
    Ok(())
}

/// The tray item was clicked: restart into a staged update, else check.
pub fn menu_clicked(app: &AppHandle) {
    if matches!(status(app), UpdateStatus::Ready { .. }) {
        if let Err(e) = restart(app) {
            log::error!("restart to update: {e}");
        }
    } else {
        check_now(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> UpdateStatus {
        UpdateStatus::Ready {
            version: "2.0.8".into(),
            current: "2.0.7".into(),
        }
    }

    #[test]
    fn only_release_builds_on_macos_update() {
        assert_eq!(disabled_reason(true, true), None);
        // build.sh / install.sh / `cargo tauri dev`: no feed compiled in.
        assert!(disabled_reason(true, false).unwrap().contains("source"));
        assert!(disabled_reason(false, true).unwrap().contains("macOS"));
    }

    #[test]
    fn the_shipped_config_has_no_feed_and_the_release_config_does() {
        // The pins that keep local builds from replacing themselves.
        let base: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let release: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.release.conf.json")).unwrap();
        assert!(base["plugins"]["updater"].is_null());
        assert!(release["plugins"]["updater"]["pubkey"].is_string());
    }

    #[test]
    fn feed_configured_reads_the_plugin_section() {
        let mut config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(!feed_configured(&config));
        config.plugins.0.insert(
            "updater".into(),
            serde_json::json!({ "pubkey": "x", "endpoints": [] }),
        );
        assert!(feed_configured(&config));
    }

    #[test]
    fn tray_item_follows_the_status() {
        assert_eq!(
            menu_item(&UpdateStatus::Disabled { reason: "x".into() }),
            None
        );
        assert_eq!(
            menu_item(&UpdateStatus::Idle),
            Some(("Check for Updates…".into(), true))
        );
        assert_eq!(
            menu_item(&UpdateStatus::Checking { manual: true }),
            Some(("Checking for Updates…".into(), false))
        );
        assert_eq!(
            menu_item(&ready()),
            Some(("Restart to Update to 2.0.8".into(), true))
        );
        // A failed check leaves the item ready for another try.
        let failed = UpdateStatus::Failed {
            message: "offline".into(),
            manual: false,
        };
        assert_eq!(
            menu_item(&failed),
            Some(("Check for Updates…".into(), true))
        );
    }

    #[test]
    fn only_checks_the_user_asked_for_flash() {
        let current = UpdateStatus::Current {
            version: "2.0.7".into(),
            manual: true,
        };
        assert_eq!(
            result_flash(&current, true).as_deref(),
            Some("Voice 2.0.7 is up to date")
        );
        assert_eq!(result_flash(&current, false), None);
        assert_eq!(result_flash(&ready(), false), None);
        assert_eq!(
            result_flash(&ready(), true).as_deref(),
            Some("Voice 2.0.8 is ready")
        );
    }

    #[test]
    fn flashes_fit_the_overlay_pill() {
        // `typed_flash` caps dictation previews at "Typed: " + 24 chars; an
        // update message must not be longer than that.
        let long = UpdateStatus::Current {
            version: "10.10.10".into(),
            manual: true,
        };
        let failed = UpdateStatus::Failed {
            message: "x".into(),
            manual: true,
        };
        for status in [long, failed, ready()] {
            let text = result_flash(&status, true).unwrap();
            assert!(text.chars().count() <= 31, "{text}");
        }
    }

    #[test]
    fn status_serializes_as_the_ui_reads_it() {
        assert_eq!(
            serde_json::to_value(ready()).unwrap(),
            serde_json::json!({ "state": "ready", "version": "2.0.8", "current": "2.0.7" })
        );
        assert_eq!(
            serde_json::to_value(UpdateStatus::Current {
                version: "2.0.7".into(),
                manual: true
            })
            .unwrap(),
            serde_json::json!({ "state": "current", "version": "2.0.7", "manual": true })
        );
        assert_eq!(
            serde_json::to_value(UpdateStatus::Idle).unwrap(),
            serde_json::json!({ "state": "idle" })
        );
    }
}
