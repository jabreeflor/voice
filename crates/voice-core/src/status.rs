//! Status line decision table. `compute_status` is a pure function of
//! `StatusInputs`; copy and precedence belong here (and in tests/status.rs),
//! never inlined in UI. Reference: Sources/VoiceCore/app.swift.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusColor {
    Red,
    Orange,
    Green,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// Everything the status line depends on, gathered so the decision logic
/// stays a pure function of its inputs.
#[derive(Clone, Debug)]
pub struct StatusInputs {
    pub mic_denied: bool,
    pub tap_running: bool,
    pub ax_trusted: bool,
    pub recently_relaunched: bool,
    pub onboarding_visible: bool,
    pub setup_progress: Option<f64>,
    pub setup_failed: bool,
    pub engine_exists: bool,
    pub engine_ready: bool,
    pub engine_status_text: String,
    pub hotkey_label: String,
    pub platform: Platform,
}

impl Default for StatusInputs {
    fn default() -> Self {
        StatusInputs {
            mic_denied: false,
            tap_running: true,
            ax_trusted: true,
            recently_relaunched: false,
            onboarding_visible: false,
            setup_progress: None,
            setup_failed: false,
            engine_exists: true,
            engine_ready: true,
            engine_status_text: String::new(),
            hotkey_label: "Right ⌥ Option".to_string(),
            platform: Platform::current(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StatusInfo {
    pub text: String,
    pub color: StatusColor,
    pub needs_accessibility: bool,
}

pub fn compute_status(i: &StatusInputs) -> StatusInfo {
    let _ = i;
    todo!()
}
