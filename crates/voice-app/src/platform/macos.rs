//! macOS: Accessibility (event tap) and Microphone (TCC) permission shims.

use super::MicStatus;

pub fn accessibility_trusted() -> bool {
    todo!()
}

pub fn request_accessibility() {
    todo!()
}

pub fn open_accessibility_settings() {
    todo!()
}

pub fn mic_status() -> MicStatus {
    todo!()
}

pub fn request_mic() {
    todo!()
}

pub fn relaunch_self() {
    todo!()
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    Ok(())
}

pub fn engine_install_hint() -> &'static str {
    "brew install whisper-cpp"
}
