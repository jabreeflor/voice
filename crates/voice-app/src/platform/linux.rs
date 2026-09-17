//! Linux: X11 only for the key hook (Wayland has no global key listening).

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

pub fn relaunch_self() {
    todo!()
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    todo!()
}

pub fn engine_install_hint() -> &'static str {
    "install whisper.cpp (whisper-server) and put it on PATH"
}
