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
/// under `ConsentStore\microphone\NonPackaged\<exe path>` combined with the
/// global "Let apps access your microphone" switch: either one set to Deny
/// blocks capture. The per-app key is written when the user answers the
/// consent prompt (or flips the app in Settings); until then only the global
/// switch exists.
pub fn mic_status() -> MicStatus {
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let global = consent_value(KEY);
    let per_app = std::env::current_exe()
        .ok()
        .map(|exe| format!(r"{KEY}\NonPackaged\{}", per_app_key(&exe.to_string_lossy())))
        .and_then(|k| consent_value(&k));
    consent_status(per_app.as_deref(), global.as_deref())
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

/// `per_app` wins when it has an answer (it is the value the consent prompt
/// writes), except that a global Deny blocks every app regardless.
pub fn consent_status(per_app: Option<&str>, global: Option<&str>) -> MicStatus {
    let parse = |v: Option<&str>| match v.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("Allow") => Some(MicStatus::Granted),
        Some(v) if v.eq_ignore_ascii_case("Deny") => Some(MicStatus::Denied),
        _ => None,
    };
    match (parse(per_app), parse(global)) {
        (_, Some(MicStatus::Denied)) | (Some(MicStatus::Denied), _) => MicStatus::Denied,
        (Some(app), _) => app,
        (None, Some(global)) => global,
        (None, None) => MicStatus::Undetermined,
    }
}

/// Opening the capture device is what triggers the consent prompt on
/// Windows, so there is nothing to request up front.
pub fn request_mic() {}

/// Spawns a detached copy of this executable; the caller exits afterwards.
pub fn relaunch_self() {
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: no inherited console, not in
    // our job/ctrl-c group, so the child survives our exit.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("relaunch: current_exe unavailable");
        return;
    };
    if let Err(e) = Command::new(exe)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
    {
        log::warn!("relaunch failed: {e}");
    }
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    Ok(())
}

pub fn engine_install_hint() -> &'static str {
    "put whisper-server.exe next to Voice or on PATH"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_value_maps_to_status() {
        assert_eq!(consent_status(None, Some("Allow")), MicStatus::Granted);
        assert_eq!(consent_status(None, Some("Deny")), MicStatus::Denied);
        assert_eq!(
            consent_status(None, Some("Prompt")),
            MicStatus::Undetermined
        );
        assert_eq!(consent_status(None, None), MicStatus::Undetermined);
        assert_eq!(
            consent_status(Some("Allow"), Some("Allow")),
            MicStatus::Granted
        );
    }

    // A per-app Deny blocks capture even though the global switch is on, and
    // the global switch off blocks an app that was individually allowed.
    #[test]
    fn per_app_deny_overrides_global_allow() {
        assert_eq!(
            consent_status(Some("Deny"), Some("Allow")),
            MicStatus::Denied
        );
        assert_eq!(
            consent_status(Some("Allow"), Some("Deny")),
            MicStatus::Denied
        );
        assert_eq!(consent_status(Some("Deny"), None), MicStatus::Denied);
    }

    #[test]
    fn per_app_key_replaces_backslashes() {
        assert_eq!(
            per_app_key(r"C:\Program Files\Voice\voice.exe"),
            "C:#Program Files#Voice#voice.exe"
        );
    }
}
