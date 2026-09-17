//! Port of Tests/VoiceCoreTests/StatusTests.swift.
//!
//! Decision-table coverage for `compute_status`. The branches are checked in a
//! fixed order, so most cases here pin down one branch plus the precedence that
//! keeps the branches above it from stealing the result.
//!
//! The Swift copy is the macOS copy, so the ported cases fix `platform` to
//! `MacOs` regardless of the host running the tests; the Windows/Linux copy
//! is pinned separately at the bottom.

use voice_core::{compute_status, Platform, StatusColor, StatusInfo, StatusInputs};

// MARK: - Fixture

/// Healthy, fully-ready app on macOS. Each test overrides only what it cares
/// about via struct update syntax.
fn inputs() -> StatusInputs {
    StatusInputs {
        platform: Platform::MacOs,
        ..StatusInputs::default()
    }
}

#[track_caller]
fn assert_contains(info: &StatusInfo, needle: &str) {
    assert!(
        info.text.contains(needle),
        "expected status text to contain \"{needle}\", got \"{}\"",
        info.text
    );
}

// MARK: - Microphone denied

#[test]
fn mic_denied_is_red_and_does_not_ask_for_accessibility() {
    let s = compute_status(&StatusInputs {
        mic_denied: true,
        ..inputs()
    });
    assert_contains(&s, "Microphone access is off");
    assert_eq!(s.color, StatusColor::Red);
    assert!(!s.needs_accessibility);
}

#[test]
fn mic_denied_beats_setup_failed() {
    let s = compute_status(&StatusInputs {
        mic_denied: true,
        setup_failed: true,
        ..inputs()
    });
    assert_contains(&s, "Microphone access is off");
}

#[test]
fn mic_denied_beats_stopped_tap() {
    let s = compute_status(&StatusInputs {
        mic_denied: true,
        tap_running: false,
        ax_trusted: false,
        ..inputs()
    });
    assert_contains(&s, "Microphone access is off");
    assert!(!s.needs_accessibility);
}

#[test]
fn mic_denied_beats_setup_progress() {
    let s = compute_status(&StatusInputs {
        mic_denied: true,
        setup_progress: Some(0.5),
        ..inputs()
    });
    assert_contains(&s, "Microphone access is off");
    assert_eq!(s.color, StatusColor::Red);
}

#[test]
fn mic_denied_beats_engine_ready() {
    let s = compute_status(&StatusInputs {
        mic_denied: true,
        engine_ready: true,
        ..inputs()
    });
    assert_contains(&s, "Microphone access is off");
    assert_ne!(s.color, StatusColor::Green);
}

// MARK: - Event tap not running

#[test]
fn trusted_but_blocked_after_relaunch_asks_for_toggle() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: true,
        ..inputs()
    });
    assert_contains(&s, "Permission granted but blocked");
    assert_contains(&s, "toggle Voice off and on");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(s.needs_accessibility);
}

#[test]
fn trusted_and_not_yet_relaunched_says_restarting() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: false,
        ..inputs()
    });
    assert_contains(&s, "restarting Voice");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

#[test]
fn trusted_during_onboarding_says_setup_will_finish_when_it_closes() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: false,
        onboarding_visible: true,
        ..inputs()
    });
    assert_eq!(
        s.text,
        "Permission granted — Voice will finish applying it when setup closes"
    );
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

#[test]
fn recently_relaunched_beats_onboarding_visible() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: true,
        onboarding_visible: true,
        ..inputs()
    });
    assert_contains(&s, "Permission granted but blocked");
    assert_contains(&s, "toggle Voice off and on");
    assert!(s.needs_accessibility);
}

#[test]
fn not_trusted_asks_to_grant_permission() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        ..inputs()
    });
    assert_contains(&s, "Grant Accessibility permission");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(s.needs_accessibility);
}

#[test]
fn not_trusted_ignores_recently_relaunched() {
    let fresh = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        recently_relaunched: false,
        ..inputs()
    });
    let relaunched = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        recently_relaunched: true,
        ..inputs()
    });
    assert_eq!(fresh, relaunched);
}

#[test]
fn stopped_tap_beats_engine_ready() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        engine_ready: true,
        ..inputs()
    });
    assert_contains(&s, "Grant Accessibility permission");
    assert_ne!(s.color, StatusColor::Green);
}

#[test]
fn stopped_tap_beats_setup_progress() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        setup_progress: Some(0.3),
        ..inputs()
    });
    assert_contains(&s, "Grant Accessibility permission");
    assert!(s.needs_accessibility);
}

#[test]
fn stopped_tap_beats_setup_failed() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: true,
        setup_failed: true,
        ..inputs()
    });
    assert_contains(&s, "Permission granted but blocked");
    assert_eq!(s.color, StatusColor::Orange);
}

#[test]
fn running_tap_never_asks_for_accessibility() {
    for ax_trusted in [true, false] {
        for recently_relaunched in [true, false] {
            let s = compute_status(&StatusInputs {
                tap_running: true,
                ax_trusted,
                recently_relaunched,
                ..inputs()
            });
            assert!(
                !s.needs_accessibility,
                "ax_trusted={ax_trusted} recently_relaunched={recently_relaunched}"
            );
        }
    }
}

// MARK: - Setup progress

#[test]
fn progress_zero_shows_zero_percent() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.0),
        ..inputs()
    });
    assert_contains(&s, "0%");
    assert_contains(&s, "Setting up");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

#[test]
fn progress_forty_two_percent() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.42),
        ..inputs()
    });
    assert_contains(&s, "42%");
    assert_eq!(s.color, StatusColor::Orange);
}

#[test]
fn progress_one_shows_hundred_percent() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(1.0),
        ..inputs()
    });
    assert_contains(&s, "100%");
    assert_eq!(s.color, StatusColor::Orange);
}

/// The percentage truncates rather than rounds, so 99.9% reads as 99%.
#[test]
fn progress_truncates_toward_zero() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.999),
        ..inputs()
    });
    assert_contains(&s, "99%");
}

#[test]
fn progress_beats_setup_failed() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.6),
        setup_failed: true,
        ..inputs()
    });
    assert_contains(&s, "60%");
    assert_eq!(s.color, StatusColor::Orange);
}

#[test]
fn progress_beats_missing_engine() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.1),
        engine_exists: false,
        engine_ready: false,
        ..inputs()
    });
    assert_contains(&s, "10%");
}

#[test]
fn progress_beats_engine_ready() {
    let s = compute_status(&StatusInputs {
        setup_progress: Some(0.25),
        engine_ready: true,
        ..inputs()
    });
    assert_contains(&s, "25%");
    assert_ne!(s.color, StatusColor::Green);
}

// MARK: - Setup failed

#[test]
fn setup_failed_is_red() {
    let s = compute_status(&StatusInputs {
        setup_progress: None,
        setup_failed: true,
        ..inputs()
    });
    assert_contains(&s, "Setup failed");
    assert_eq!(s.color, StatusColor::Red);
    assert!(!s.needs_accessibility);
}

#[test]
fn setup_failed_beats_missing_engine() {
    let s = compute_status(&StatusInputs {
        setup_failed: true,
        engine_exists: false,
        engine_ready: false,
        ..inputs()
    });
    assert_contains(&s, "Setup failed");
    assert_eq!(s.color, StatusColor::Red);
}

#[test]
fn setup_failed_beats_engine_ready() {
    let s = compute_status(&StatusInputs {
        setup_failed: true,
        engine_ready: true,
        ..inputs()
    });
    assert_contains(&s, "Setup failed");
    assert_eq!(s.color, StatusColor::Red);
}

// MARK: - Engine missing

#[test]
fn missing_engine_is_preparing() {
    let s = compute_status(&StatusInputs {
        engine_exists: false,
        engine_ready: false,
        ..inputs()
    });
    assert_eq!(s.text, "Preparing");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

/// A nonexistent engine cannot be ready; the guard runs first either way.
#[test]
fn missing_engine_beats_ready_flag() {
    let s = compute_status(&StatusInputs {
        engine_exists: false,
        engine_ready: true,
        ..inputs()
    });
    assert_eq!(s.text, "Preparing");
}

#[test]
fn missing_engine_beats_not_installed_text() {
    let s = compute_status(&StatusInputs {
        engine_exists: false,
        engine_ready: false,
        engine_status_text: "whisper-server not installed".to_string(),
        ..inputs()
    });
    assert_eq!(s.text, "Preparing");
    assert_eq!(s.color, StatusColor::Orange);
}

// MARK: - Engine ready

#[test]
fn ready_is_green_and_mentions_default_hotkey() {
    let s = compute_status(&inputs());
    assert_contains(&s, "Ready");
    assert_contains(&s, "Right ⌥ Option");
    assert_eq!(s.color, StatusColor::Green);
    assert!(!s.needs_accessibility);
}

#[test]
fn ready_round_trips_custom_hotkey_label() {
    let label = "Fn + ⌃ Control";
    let s = compute_status(&StatusInputs {
        hotkey_label: label.to_string(),
        ..inputs()
    });
    assert_contains(&s, label);
    assert_eq!(s.color, StatusColor::Green);
}

#[test]
fn ready_beats_not_installed_text() {
    let s = compute_status(&StatusInputs {
        engine_ready: true,
        engine_status_text: "whisper-server not installed".to_string(),
        ..inputs()
    });
    assert_contains(&s, "Ready");
    assert_eq!(s.color, StatusColor::Green);
}

// MARK: - whisper-server missing

#[test]
fn not_installed_suggests_brew() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: "whisper-server not installed".to_string(),
        ..inputs()
    });
    assert_contains(&s, "brew install whisper-cpp");
    assert_eq!(s.color, StatusColor::Red);
    assert!(!s.needs_accessibility);
}

/// The match is an exact string compare, so near-misses fall through.
#[test]
fn not_installed_match_is_exact() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: "whisper-server not installed.".to_string(),
        ..inputs()
    });
    assert_contains(&s, "Starting the speech engine");
    assert_eq!(s.color, StatusColor::Orange);
}

// MARK: - Fallback

#[test]
fn fallback_is_starting_engine() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: String::new(),
        ..inputs()
    });
    assert_eq!(s.text, "Starting the speech engine");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

#[test]
fn fallback_for_unrecognized_engine_status() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: "loading model".to_string(),
        ..inputs()
    });
    assert_eq!(s.text, "Starting the speech engine");
    assert_eq!(s.color, StatusColor::Orange);
}

// MARK: - Purity

#[test]
fn same_inputs_produce_same_output() {
    let i = StatusInputs {
        tap_running: false,
        ax_trusted: true,
        recently_relaunched: true,
        ..inputs()
    };
    assert_eq!(compute_status(&i), compute_status(&i));
}

// MARK: - Platform copy (Windows / Linux)
//
// Off macOS only the remedy text changes; colour and precedence stay the
// same, and there is no Accessibility pane to send the user to.

#[test]
fn default_platform_is_the_host() {
    assert_eq!(StatusInputs::default().platform, Platform::current());
    let expected = if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };
    assert_eq!(Platform::current(), expected);
}

#[test]
fn mic_denied_off_macos_points_at_the_generic_privacy_settings() {
    for platform in [Platform::Windows, Platform::Linux] {
        let s = compute_status(&StatusInputs {
            mic_denied: true,
            platform,
            ..inputs()
        });
        assert_eq!(
            s.text,
            "Microphone access is off — enable it in your system privacy settings"
        );
        assert_eq!(s.color, StatusColor::Red);
        assert!(!s.needs_accessibility);
    }
}

#[test]
fn hook_failure_on_linux_blames_wayland_and_never_asks_for_accessibility() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        platform: Platform::Linux,
        ..inputs()
    });
    assert_eq!(
        s.text,
        "Could not listen for the talk key. On Linux this needs an X11 session (Wayland is not supported yet)."
    );
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

#[test]
fn hook_failure_on_windows_suggests_a_restart_and_never_asks_for_accessibility() {
    let s = compute_status(&StatusInputs {
        tap_running: false,
        ax_trusted: false,
        platform: Platform::Windows,
        ..inputs()
    });
    assert_eq!(s.text, "Could not listen for the talk key — restart Voice");
    assert_eq!(s.color, StatusColor::Orange);
    assert!(!s.needs_accessibility);
}

/// The trusted-but-not-running branches carry no macOS-only remedy, so their
/// copy is shared by every platform.
#[test]
fn trusted_hook_failure_copy_is_platform_independent() {
    for platform in [Platform::Windows, Platform::Linux] {
        let mac = compute_status(&StatusInputs {
            tap_running: false,
            ax_trusted: true,
            recently_relaunched: true,
            ..inputs()
        });
        let other = compute_status(&StatusInputs {
            tap_running: false,
            ax_trusted: true,
            recently_relaunched: true,
            platform,
            ..inputs()
        });
        assert_eq!(mac, other);
    }
}

#[test]
fn not_installed_on_windows_points_at_the_exe_location() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: "whisper-server not installed".to_string(),
        platform: Platform::Windows,
        ..inputs()
    });
    assert_eq!(
        s.text,
        "Speech engine missing — put whisper-server.exe next to Voice or on PATH"
    );
    assert_eq!(s.color, StatusColor::Red);
    assert!(!s.needs_accessibility);
}

#[test]
fn not_installed_on_linux_points_at_path() {
    let s = compute_status(&StatusInputs {
        engine_ready: false,
        engine_status_text: "whisper-server not installed".to_string(),
        platform: Platform::Linux,
        ..inputs()
    });
    assert_eq!(
        s.text,
        "Speech engine missing — install whisper.cpp (whisper-server) and put it on PATH"
    );
    assert_eq!(s.color, StatusColor::Red);
    assert!(!s.needs_accessibility);
}

/// Everything below the hook branch reads the same on every platform; only
/// the two remedies above are localised.
#[test]
fn shared_branches_read_the_same_on_every_platform() {
    for platform in [Platform::Windows, Platform::Linux] {
        let cases = [
            StatusInputs {
                setup_progress: Some(0.42),
                ..inputs()
            },
            StatusInputs {
                setup_failed: true,
                ..inputs()
            },
            StatusInputs {
                engine_exists: false,
                engine_ready: false,
                ..inputs()
            },
            inputs(),
            StatusInputs {
                engine_ready: false,
                engine_status_text: "loading model".to_string(),
                ..inputs()
            },
        ];
        for case in cases {
            let other = StatusInputs {
                platform,
                ..case.clone()
            };
            assert_eq!(compute_status(&case), compute_status(&other));
        }
    }
}
