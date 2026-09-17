//! OS-specific shims: permissions, settings panes, relaunch. Everything the
//! Swift app did through TCC / ApplicationServices / NSWorkspace lives here so
//! the rest of the app stays platform-neutral.
//!
//! Each target module must export the same function set with the signatures
//! below; the re-exports are the only public surface.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::*;

/// Each target module constructs only the variants its OS can report (Linux
/// is always `NotApplicable`, macOS never is), so on any one build some
/// variants are never built. They all still reach the UI through `as_str`.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MicStatus {
    Granted,
    Denied,
    Undetermined,
    /// The OS has no per-app microphone consent (Linux).
    NotApplicable,
}

impl MicStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MicStatus::Granted => "granted",
            MicStatus::Denied => "denied",
            MicStatus::Undetermined => "undetermined",
            MicStatus::NotApplicable => "not_applicable",
        }
    }
}

// Contract implemented by every target module:
//
// pub fn accessibility_trusted() -> bool;       // macOS AXIsProcessTrusted; true elsewhere
// pub fn request_accessibility();               // macOS AXIsProcessTrustedWithOptions(prompt); no-op elsewhere
// pub fn open_accessibility_settings();         // macOS Privacy & Security > Accessibility pane; best-effort elsewhere
// pub fn mic_status() -> MicStatus;             // macOS AVCaptureDevice authorization; Windows consent registry; Linux NotApplicable
// pub fn request_mic();                         // macOS AVCaptureDevice.requestAccess (silent after the first answer); Windows/Linux no-op. Safe to call on every launch.
// pub fn open_microphone_settings();           // Windows only: Privacy & security > Microphone page (desktop apps get no prompt, so the onboarding button opens this instead). Callers gate on cfg(target_os = "windows").
// pub fn relaunch_self();                       // macOS: `sh -c 'sleep 0.7; open -n <bundle>'`; Linux: `sh -c 'sleep 0.7; exec <exe>'`; Windows: `cmd /c waitfor /t 1 ... & start <exe>` (~1 s; must not rely on a console)
// pub fn global_hotkeys_supported() -> Result<(), String>;  // Linux: Err on Wayland-only sessions
