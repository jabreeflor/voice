//! Windows: no accessibility gate; microphone consent is a per-app privacy setting.

use std::os::windows::process::CommandExt;
use std::process::Command;

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use super::MicStatus;

pub fn accessibility_trusted() -> bool {
    true
}

pub fn request_accessibility() {}

/// Low-level keyboard hooks need no grant on Windows, so there is no pane to
/// open.
pub fn open_accessibility_settings() {}

/// Microphone consent for a desktop (non-packaged) app is the per-app entry
/// under `ConsentStore\microphone\NonPackaged\<exe path>` combined with two
/// group switches: `NonPackaged` itself ("Let desktop apps access your
/// microphone") and the global "Let apps access your microphone" one. Any of
/// the three set to Deny blocks capture. The per-app key is written when the
/// user answers the consent prompt (or flips the app in Settings); until
/// then only the switches exist.
pub fn mic_status() -> MicStatus {
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let global = consent_value(KEY);
    let non_packaged = consent_value(&format!(r"{KEY}\NonPackaged"));
    let per_app = std::env::current_exe()
        .ok()
        .map(|exe| format!(r"{KEY}\NonPackaged\{}", per_app_key(&exe.to_string_lossy())))
        .and_then(|k| consent_value(&k));
    consent_status(
        per_app.as_deref(),
        non_packaged.as_deref(),
        global.as_deref(),
    )
}

fn consent_value(key: &str) -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(key)
        .and_then(|k| k.get_value("Value"))
        .ok()
}

/// Windows names the per-app sub-key after the executable path with every
/// backslash replaced by `#` (`C:#Program Files#Voice#voice.exe`).
pub fn per_app_key(exe_path: &str) -> String {
    exe_path.replace('\\', "#")
}

/// Any Deny wins (each switch blocks everything beneath it). Otherwise the
/// most specific answer counts: `per_app` (the value the consent prompt
/// writes), then the desktop-apps switch, then the global one.
pub fn consent_status(
    per_app: Option<&str>,
    non_packaged: Option<&str>,
    global: Option<&str>,
) -> MicStatus {
    let parse = |v: Option<&str>| match v.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("Allow") => Some(MicStatus::Granted),
        Some(v) if v.eq_ignore_ascii_case("Deny") => Some(MicStatus::Denied),
        _ => None,
    };
    let levels = [parse(per_app), parse(non_packaged), parse(global)];
    if levels.contains(&Some(MicStatus::Denied)) {
        return MicStatus::Denied;
    }
    levels
        .into_iter()
        .flatten()
        .next()
        .unwrap_or(MicStatus::Undetermined)
}

/// Desktop (non-packaged) apps never get a consent dialog on Windows: the
/// answer is the Settings switches read by `mic_status`. So the onboarding
/// "Open Settings" button deep-links to Privacy & security > Microphone, the
/// only place the user can flip them.
pub fn request_mic() {
    // `start` is a cmd builtin; CREATE_NO_WINDOW keeps the helper console
    // from flashing up. `""` is the window-title slot `start` would
    // otherwise fill with the first quoted argument.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match Command::new("cmd")
        .args(["/c", "start", "", "ms-settings:privacy-microphone"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        // Reap off the caller's thread so each click does not leave a
        // zombie for the lifetime of the app.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("could not open Microphone settings: {e}"),
    }
}

/// Spawns a detached copy of this executable; the caller exits afterwards.
/// Like the macOS path, the new instance starts about a second later so the
/// old one has released the tray icon, settings.json and port 8178 first.
///
/// The delay is `waitfor /t 1`, not `timeout /t 1`: the child runs detached
/// from a GUI process, so it has no console and its stdin is not a console
/// handle. `timeout.exe` refuses to run in that situation ("Input redirection
/// is not supported") and exits at once, which would launch the new instance
/// with no pause at all. `waitfor` never touches stdin; it sleeps until the
/// timeout and exits with errorlevel 1, which `&` ignores. (Its resolution is
/// whole seconds, hence ~1 s rather than macOS's 0.7 s.)
pub fn relaunch_self() {
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: no inherited console, not in
    // our job/ctrl-c group, so the child survives our exit.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("relaunch: current_exe unavailable");
        return;
    };
    // `start ""`: the first quoted argument is the window title, so the
    // quoted path needs an empty one in front of it.
    let script = format!(
        "waitfor /t 1 VoiceRelaunch >nul 2>&1 & start \"\" \"{}\"",
        exe.to_string_lossy()
    );
    if let Err(e) = Command::new("cmd")
        .arg("/c")
        .raw_arg(script)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
    {
        log::warn!("relaunch failed: {e}");
    }
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_value_maps_to_status() {
        assert_eq!(
            consent_status(None, None, Some("Allow")),
            MicStatus::Granted
        );
        assert_eq!(consent_status(None, None, Some("Deny")), MicStatus::Denied);
        assert_eq!(
            consent_status(None, None, Some("Prompt")),
            MicStatus::Undetermined
        );
        assert_eq!(consent_status(None, None, None), MicStatus::Undetermined);
        assert_eq!(
            consent_status(Some("Allow"), Some("Allow"), Some("Allow")),
            MicStatus::Granted
        );
    }

    // A per-app Deny blocks capture even though the global switch is on, and
    // the global switch off blocks an app that was individually allowed.
    #[test]
    fn per_app_deny_overrides_global_allow() {
        assert_eq!(
            consent_status(Some("Deny"), None, Some("Allow")),
            MicStatus::Denied
        );
        assert_eq!(
            consent_status(Some("Allow"), None, Some("Deny")),
            MicStatus::Denied
        );
        assert_eq!(consent_status(Some("Deny"), None, None), MicStatus::Denied);
    }

    // "Let desktop apps access your microphone" off blocks capture even with
    // the global switch on and the app individually allowed.
    #[test]
    fn non_packaged_deny_overrides_global_allow() {
        assert_eq!(
            consent_status(None, Some("Deny"), Some("Allow")),
            MicStatus::Denied
        );
        assert_eq!(
            consent_status(Some("Allow"), Some("Deny"), Some("Allow")),
            MicStatus::Denied
        );
        assert_eq!(
            consent_status(None, Some("Allow"), Some("Prompt")),
            MicStatus::Granted
        );
    }

    #[test]
    fn per_app_key_replaces_backslashes() {
        assert_eq!(
            per_app_key(r"C:\Program Files\Voice\voice.exe"),
            "C:#Program Files#Voice#voice.exe"
        );
    }
}
