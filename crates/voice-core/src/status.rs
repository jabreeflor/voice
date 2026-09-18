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

fn info(text: impl Into<String>, color: StatusColor, needs_accessibility: bool) -> StatusInfo {
    StatusInfo {
        text: text.into(),
        color,
        needs_accessibility,
    }
}

/// Branch order is the contract: mic → hook → setup progress → setup failed →
/// engine exists → engine ready → not installed → starting. Only the copy for
/// platform-specific remedies differs off macOS; precedence never does.
pub fn compute_status(i: &StatusInputs) -> StatusInfo {
    use StatusColor::{Green, Orange, Red};

    if i.mic_denied {
        let text = match i.platform {
            Platform::MacOs => {
                "Microphone access is off — enable it in System Settings, Privacy & Security"
            }
            _ => "Microphone access is off — enable it in your system privacy settings",
        };
        return info(text, Red, false);
    }
    if !i.tap_running {
        if i.ax_trusted {
            if i.recently_relaunched {
                return info(
                    "Permission granted but blocked — toggle Voice off and on in Accessibility settings",
                    Orange,
                    true,
                );
            }
            // The auto-relaunch is suppressed while onboarding is on screen,
            // so don't promise a restart that won't happen until it closes.
            if i.onboarding_visible {
                return info(
                    "Permission granted — Voice will finish applying it when setup closes",
                    Orange,
                    false,
                );
            }
            return info(
                "Permission granted — restarting Voice to apply it",
                Orange,
                false,
            );
        }
        // Only macOS gates the hook behind a permission the user can grant;
        // elsewhere a failed hook is an environment problem, so there is no
        // Accessibility pane to open.
        return match i.platform {
            Platform::MacOs => info(
                "Grant Accessibility permission to enable the talk key. Already listed? Toggle Voice off and on.",
                Orange,
                true,
            ),
            Platform::Linux => info(
                "Could not listen for the talk key. On Linux this needs an X11 session (Wayland is not supported yet).",
                Orange,
                false,
            ),
            Platform::Windows => info(
                "Could not listen for the talk key — restart Voice",
                Orange,
                false,
            ),
        };
    }
    if let Some(p) = i.setup_progress {
        // Truncates like Swift's Int(p * 100): 0.999 reads as 99%.
        let percent = (p * 100.0) as i64;
        return info(
            format!("Setting up — downloading the speech engine ({percent}%)"),
            Orange,
            false,
        );
    }
    if i.setup_failed {
        return info(
            "Setup failed — check your connection and relaunch Voice",
            Red,
            false,
        );
    }
    if !i.engine_exists {
        return info("Preparing", Orange, false);
    }
    if i.engine_ready {
        return info(
            format!("Ready — hold {} and speak", i.hotkey_label),
            Green,
            false,
        );
    }
    if i.engine_status_text == "whisper-server not installed" {
        let text = match i.platform {
            Platform::MacOs => "Speech engine missing — run: brew install whisper-cpp",
            Platform::Windows => {
                "Speech engine missing — put whisper-server.exe next to Voice or on PATH"
            }
            Platform::Linux => {
                "Speech engine missing — install whisper.cpp (whisper-server) and put it on PATH"
            }
        };
        return info(text, Red, false);
    }
    info("Starting the speech engine", Orange, false)
}
