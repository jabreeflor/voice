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
/// outlives the caller, which is expected to exit afterwards. Delayed by
/// 0.7 s like the macOS path so the old instance has let go of the tray,
/// settings.json and port 8178 before the new one claims them.
pub fn relaunch_self() {
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("relaunch: current_exe unavailable");
        return;
    };
    let script = format!("sleep 0.7; exec {}", shell_quote(&exe));
    if let Err(e) = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .process_group(0)
        .spawn()
    {
        log::warn!("relaunch failed: {e}");
    }
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    let get = |k: &str| std::env::var(k).ok();
    check_session(
        get("XDG_SESSION_TYPE").as_deref(),
        get("WAYLAND_DISPLAY").as_deref(),
    )
}

/// The key hook uses XRecord, which only sees keystrokes routed through the
/// X server. Under Wayland the compositor delivers input straight to
/// Wayland-native clients, so even when XWayland is up and `DISPLAY` is set
/// (GNOME, KDE and sway all do that) XRecord connects fine and then hears
/// nothing — a hook that "starts" but is dead. So either Wayland signal
/// (`XDG_SESSION_TYPE=wayland` or a `WAYLAND_DISPLAY`) is decisive, and
/// `DISPLAY` is deliberately not consulted at all. Empty values count as
/// unset. A session with no hints at all is left to rdev, which fails
/// cleanly without a display.
pub fn check_session(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
) -> Result<(), String> {
    let set = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).is_some();
    let wayland = session_type
        .map(|s| s.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || set(wayland_display);
    if wayland {
        return Err(
            "this is a Wayland session; global hotkeys need an X11 session (XWayland cannot see Wayland clients' keys)"
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
    use super::{check_session, shell_quote};

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(
            shell_quote(std::path::Path::new("/a b/it's/voice")),
            "'/a b/it'\\''s/voice'"
        );
    }

    #[test]
    fn x11_session_is_supported() {
        assert!(check_session(Some("x11"), None).is_ok());
    }

    // XWayland exports DISPLAY on every mainstream Wayland desktop, but
    // XRecord through it never sees Wayland-native clients' keys, so it must
    // be rejected rather than reported as a working hook. DISPLAY is not an
    // input to the decision, so any Wayland signal alone is enough.
    #[test]
    fn xwayland_with_display_is_rejected() {
        assert!(check_session(Some("wayland"), Some("wayland-0")).is_err());
        assert!(check_session(None, Some("wayland-0")).is_err());
    }

    #[test]
    fn wayland_without_display_is_rejected() {
        assert!(check_session(Some("wayland"), None).is_err());
        assert!(check_session(Some("Wayland"), Some("wayland-0")).is_err());
        assert!(check_session(None, Some("wayland-0")).is_err());
        // Whitespace-only counts as unset.
        assert!(check_session(Some(" "), Some("wayland-0")).is_err());
    }

    // No session hints at all (ssh, minimal containers): let rdev decide.
    #[test]
    fn unknown_session_is_left_to_rdev() {
        assert!(check_session(None, None).is_ok());
        assert!(check_session(Some("tty"), None).is_ok());
        assert!(check_session(Some("x11"), Some("")).is_ok());
    }
}
