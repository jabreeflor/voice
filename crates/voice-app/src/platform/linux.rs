//! Linux: X11 only for the key hook (Wayland has no global key listening).

use std::os::unix::process::CommandExt;
use std::process::Command;

use super::MicStatus;

pub fn accessibility_trusted() -> bool {
    true
}

pub fn request_accessibility() {}

pub fn open_accessibility_settings() {}

pub fn mic_status() -> MicStatus {
    MicStatus::NotApplicable
}

pub fn request_mic() {}

/// Spawns a fresh copy of this executable in its own process group so it
/// outlives the caller, which is expected to exit afterwards.
pub fn relaunch_self() {
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("relaunch: current_exe unavailable");
        return;
    };
    if let Err(e) = Command::new(exe).process_group(0).spawn() {
        log::warn!("relaunch failed: {e}");
    }
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    let get = |k: &str| std::env::var(k).ok();
    check_session(
        get("XDG_SESSION_TYPE").as_deref(),
        get("DISPLAY").as_deref(),
        get("WAYLAND_DISPLAY").as_deref(),
    )
}

/// Heuristic for "Wayland with no X server to hook": the key hook uses
/// XRecord, which only works through an X11 connection. `DISPLAY` set means
/// either a real X session or XWayland, and both let rdev connect (under
/// XWayland only X11 clients' keys are seen, which is still better than
/// nothing). No `DISPLAY` plus either `XDG_SESSION_TYPE=wayland` or a
/// `WAYLAND_DISPLAY` is a Wayland-only session. Empty values count as unset.
/// A session with neither variable is left to rdev, which fails cleanly
/// without a display.
pub fn check_session(
    session_type: Option<&str>,
    display: Option<&str>,
    wayland_display: Option<&str>,
) -> Result<(), String> {
    let set = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).is_some();
    let wayland = session_type
        .map(|s| s.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || set(wayland_display);
    if wayland && !set(display) {
        return Err(
            "this is a Wayland session without an X11 display; global hotkeys need X11 (or XWayland with DISPLAY set)"
                .to_string(),
        );
    }
    Ok(())
}

pub fn engine_install_hint() -> &'static str {
    "install whisper.cpp (whisper-server) and put it on PATH"
}

#[cfg(test)]
mod tests {
    use super::check_session;

    #[test]
    fn x11_session_is_supported() {
        assert!(check_session(Some("x11"), Some(":0"), None).is_ok());
    }

    #[test]
    fn xwayland_with_display_is_supported() {
        assert!(check_session(Some("wayland"), Some(":1"), Some("wayland-0")).is_ok());
    }

    #[test]
    fn wayland_without_display_is_rejected() {
        assert!(check_session(Some("wayland"), None, None).is_err());
        assert!(check_session(Some("Wayland"), Some(""), Some("wayland-0")).is_err());
        assert!(check_session(None, None, Some("wayland-0")).is_err());
    }

    // No session hints at all (ssh, minimal containers): let rdev decide.
    #[test]
    fn unknown_session_is_left_to_rdev() {
        assert!(check_session(None, None, None).is_ok());
        assert!(check_session(Some("tty"), None, None).is_ok());
    }
}
