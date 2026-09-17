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

/// Reads the global "Let apps access your microphone" switch. Per-app entries
/// live in sub-keys of the same store, but the global switch is what a user
/// flips in Settings and it is the only value that can flatly deny us.
pub fn mic_status() -> MicStatus {
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let value: Option<String> = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(KEY)
        .and_then(|k| k.get_value("Value"))
        .ok();
    consent_status(value.as_deref())
}

pub fn consent_status(value: Option<&str>) -> MicStatus {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("Allow") => MicStatus::Granted,
        Some(v) if v.eq_ignore_ascii_case("Deny") => MicStatus::Denied,
        _ => MicStatus::Undetermined,
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
        assert_eq!(consent_status(Some("Allow")), MicStatus::Granted);
        assert_eq!(consent_status(Some("Deny")), MicStatus::Denied);
        assert_eq!(consent_status(Some("Prompt")), MicStatus::Undetermined);
        assert_eq!(consent_status(None), MicStatus::Undetermined);
    }
}
