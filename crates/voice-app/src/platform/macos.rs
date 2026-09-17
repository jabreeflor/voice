//! macOS: Accessibility (event tap) and Microphone (TCC) permission shims.
//!
//! Accessibility uses the two C entry points from ApplicationServices; the
//! microphone status goes through AVFoundation's `AVCaptureDevice` exactly like
//! the Swift app, so the TCC prompt text and the per-app grant are shared with
//! the previous build (same bundle id).

use std::path::{Path, PathBuf};
use std::process::Command;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use objc2::runtime::Bool;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

use super::MicStatus;

// `Boolean` in MacTypes.h is an unsigned char, hence u8 rather than bool.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

pub fn accessibility_trusted() -> bool {
    // SAFETY: plain C call with no arguments; thread-safe per Apple docs.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// `AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt: true])`: shows
/// the system "Voice would like to control this computer" dialog once and adds
/// the app to the Accessibility list (unchecked) so the user can flip it.
pub fn request_accessibility() {
    // SAFETY: kAXTrustedCheckOptionPrompt is a constant CFString owned by the
    // framework (get rule → retain via wrap_under_get_rule so our wrapper's
    // release balances).
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    // SAFETY: the dictionary outlives the call; the function only reads it.
    unsafe {
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
    }
}

/// Deep link into System Settings > Privacy & Security > Accessibility; same
/// URL the Swift app opened through NSWorkspace.
pub fn open_accessibility_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    if let Err(e) = Command::new("/usr/bin/open").arg(url).spawn() {
        log::warn!("could not open Accessibility settings: {e}");
    }
}

pub fn mic_status() -> MicStatus {
    // SAFETY: reading a framework-exported constant; None only if the symbol
    // is missing, which cannot happen on a supported macOS.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return MicStatus::Undetermined;
    };
    // SAFETY: class method, audio media type is one of the two accepted values.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
    match status {
        AVAuthorizationStatus::Authorized => MicStatus::Granted,
        // Restricted (parental controls / MDM) can never be granted, so it is
        // treated like a denial for the status line.
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => MicStatus::Denied,
        _ => MicStatus::Undetermined,
    }
}

/// `AVCaptureDevice.requestAccess(for: .audio)`: prompts once while the
/// status is NotDetermined; later calls are no-ops. The result is ignored —
/// the UI polls `mic_status` instead, as the Swift onboarding did.
pub fn request_mic() {
    // SAFETY: see mic_status.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return;
    };
    let handler = block2::RcBlock::new(|_granted: Bool| {});
    // SAFETY: the block is heap-allocated and reference counted; AVFoundation
    // retains its own copy for the asynchronous completion. The handler runs
    // on an arbitrary queue and does nothing, so no thread constraints apply.
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler);
    }
}

/// Re-opens the app after a short delay via `open -n` (a new instance is
/// required because a process that gained Accessibility after its event tap
/// was refused must restart to get a working tap). Does not exit: the caller
/// records `lastAXRelaunch` and quits, mirroring `autoRelaunchIfNeeded`.
pub fn relaunch_self() {
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("relaunch: current_exe unavailable");
        return;
    };
    let script = match bundle_path(&exe) {
        Some(bundle) => format!("sleep 0.7; /usr/bin/open -n {}", shell_quote(&bundle)),
        // Not inside a .app (e.g. `cargo run`): re-exec the binary directly.
        None => format!("sleep 0.7; exec {}", shell_quote(&exe)),
    };
    if let Err(e) = Command::new("/bin/sh").arg("-c").arg(script).spawn() {
        log::warn!("relaunch failed: {e}");
    }
}

/// Walks up from the executable (`Voice.app/Contents/MacOS/voice`) to the
/// nearest `*.app` directory.
fn bundle_path(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

pub fn global_hotkeys_supported() -> Result<(), String> {
    Ok(())
}

pub fn engine_install_hint() -> &'static str {
    "brew install whisper-cpp"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_path_finds_the_app_directory() {
        let exe = Path::new("/Applications/Voice.app/Contents/MacOS/voice");
        assert_eq!(
            bundle_path(exe),
            Some(PathBuf::from("/Applications/Voice.app"))
        );
        assert_eq!(bundle_path(Path::new("/usr/local/bin/voice")), None);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(
            shell_quote(Path::new("/a b/it's.app")),
            "'/a b/it'\\''s.app'"
        );
    }
}
