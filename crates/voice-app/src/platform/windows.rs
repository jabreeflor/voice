//! Windows: no accessibility gate; microphone consent is a per-app privacy setting.

use super::MicStatus;

pub fn accessibility_trusted() -> bool {
    true
}

pub fn request_accessibility() {}

pub fn open_accessibility_settings() {
    todo!()
}

pub fn mic_status() -> MicStatus {
    todo!()
}

pub fn request_mic() {}

pub fn relaunch_self() {
    todo!()
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    Ok(())
}

pub fn engine_install_hint() -> &'static str {
    "put whisper-server.exe next to Voice or on PATH"
}
