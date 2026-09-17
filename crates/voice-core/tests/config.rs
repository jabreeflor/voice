//! Port of Tests/VoiceCoreTests/ConfigTests.swift.
//!
//! Model discovery decides whether the app can transcribe at all. These tests
//! only ever *read* the real models directories; the only files they create
//! live in a `tempfile` directory that is deleted on drop. Settings are always
//! a throwaway file in that directory, never the real one.
//!
//! The Swift suite mutated the process environment (`setenv("VOICE_MODEL")`);
//! Rust tests run in parallel threads, so the override is passed explicitly
//! through `find_model_with_env` instead.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use voice_core::{expand_tilde, Config, Hotkey, ModelCatalog, Settings};

struct Fixture {
    dir: TempDir,
    settings: Settings,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let settings = Settings::in_dir(dir.path());
    Fixture { dir, settings }
}

impl Fixture {
    fn make_fake_model(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, [0u8]).expect("write fake model");
        path
    }

    fn find_model(&self, env: Option<&str>) -> Option<PathBuf> {
        Config::find_model_with_env(&self.settings, env)
    }
}

fn home() -> PathBuf {
    dirs::home_dir().expect("home directory")
}

// MARK: - Preference ordering

/// base.en leads deliberately: it transcribes a sentence in about a second,
/// which is what makes hold-to-talk feel instant.
#[test]
fn base_english_is_the_first_preference() {
    assert_eq!(Config::PREFERRED_MODELS.first(), Some(&"ggml-base.en.bin"));
}

#[test]
fn preference_order_runs_fast_to_accurate_with_tiny_last() {
    let order = Config::PREFERRED_MODELS;
    let idx = |name: &str| {
        order
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("{name} missing from PREFERRED_MODELS"))
    };
    assert!(idx("ggml-base.en.bin") < idx("ggml-small.en.bin"));
    assert!(idx("ggml-small.en.bin") < idx("ggml-medium.en.bin"));
    assert!(idx("ggml-medium.en.bin") < idx("ggml-large-v3-turbo.bin"));
    assert!(
        idx("ggml-large-v3-turbo.bin") < idx("ggml-tiny.en.bin"),
        "tiny is the last resort, not an early pick"
    );
    assert_eq!(order.last(), Some(&"ggml-tiny.bin"));
}

#[test]
fn english_only_variants_are_tried_before_multilingual_ones() {
    let order = Config::PREFERRED_MODELS;
    for (en, multi) in [
        ("ggml-base.en.bin", "ggml-base.bin"),
        ("ggml-small.en.bin", "ggml-small.bin"),
        ("ggml-medium.en.bin", "ggml-medium.bin"),
        ("ggml-tiny.en.bin", "ggml-tiny.bin"),
    ] {
        let en_idx = order.iter().position(|n| *n == en);
        let multi_idx = order.iter().position(|n| *n == multi);
        assert!(en_idx.is_some());
        assert!(multi_idx.is_some());
        assert!(en_idx < multi_idx, "{en} should be preferred over {multi}");
    }
}

#[test]
fn preference_list_has_no_duplicates() {
    let unique: HashSet<&str> = Config::PREFERRED_MODELS.into_iter().collect();
    assert_eq!(unique.len(), Config::PREFERRED_MODELS.len());
}

#[test]
fn every_downloadable_model_is_also_discoverable() {
    for spec in ModelCatalog::ALL {
        assert!(
            Config::PREFERRED_MODELS.contains(&spec.file),
            "{} can be downloaded but would never be auto-found",
            spec.file
        );
    }
}

// MARK: - Search directories

#[test]
fn models_directories_are_under_the_user_home_in_order() {
    let home = home();
    assert_eq!(
        Config::models_dirs(),
        vec![
            home.join("voice").join("models"),
            home.join(".voice").join("models"),
        ]
    );
}

// MARK: - VOICE_MODEL override

#[test]
fn environment_override_takes_precedence_over_everything_else() {
    let f = fixture();
    let fake = f.make_fake_model("ggml-fake.bin");
    assert_eq!(
        f.find_model(fake.to_str()),
        Some(fake.clone()),
        "VOICE_MODEL should win over the preference scan"
    );
}

/// The override is not filtered by the ggml naming convention — whatever
/// path is given is used as-is.
#[test]
fn environment_override_accepts_any_filename() {
    let f = fixture();
    let odd = f.make_fake_model("my-custom-weights.gguf");
    assert_eq!(f.find_model(odd.to_str()), Some(odd.clone()));
}

#[test]
fn environment_override_pointing_at_a_missing_file_is_ignored() {
    let f = fixture();
    let missing = f.dir.path().join("not-there.bin");
    assert_ne!(
        f.find_model(missing.to_str()),
        Some(missing.clone()),
        "a dangling override must fall through to the normal search"
    );
}

/// Swift skipped this when no model was installed under $HOME. Here the fake
/// model lives in a temp dir created *inside* $HOME, so the tilde form always
/// has something real to resolve to and the test never skips.
#[test]
fn environment_override_expands_a_tilde() {
    let home = home();
    let dir = tempfile::tempdir_in(&home).expect("temp dir under $HOME");
    let settings = Settings::in_dir(dir.path());
    let real = dir.path().join("ggml-tilde.bin");
    fs::write(&real, [0u8]).expect("write fake model");

    let relative = real
        .strip_prefix(&home)
        .expect("temp dir is under home")
        .to_str()
        .expect("utf-8 path");
    let tilde = format!("~/{relative}");
    assert_eq!(expand_tilde(&tilde), real);
    assert_eq!(
        Config::find_model_with_env(&settings, Some(&tilde)),
        Some(real.clone())
    );
}

#[test]
fn empty_environment_override_is_ignored() {
    let f = fixture();
    assert_ne!(f.find_model(Some("")), Some(PathBuf::from("")));
}

/// A bare `~` and a non-tilde path are the other two shapes `expand_tilde`
/// must handle; the app only ever writes `~/…`, but a user can type anything.
#[test]
fn expand_tilde_handles_bare_and_absent_tilde() {
    assert_eq!(expand_tilde("~"), home());
    assert_eq!(
        expand_tilde("/abs/path.bin"),
        PathBuf::from("/abs/path.bin")
    );
    assert_eq!(expand_tilde("~user/x"), PathBuf::from("~user/x"));
}

// MARK: - find_model invariants

/// Whatever find_model returns must actually be on disk, or the engine is
/// launched against a path that cannot be opened.
#[test]
fn find_model_only_returns_paths_that_exist() {
    let f = fixture();
    if let Some(found) = f.find_model(None) {
        assert!(found.exists());
    }
}

#[test]
fn find_model_is_stable_across_calls() {
    let f = fixture();
    assert_eq!(f.find_model(None), f.find_model(None));
}

// MARK: - Selected model

#[test]
fn selected_model_file_round_trips_and_none_removes_the_key() {
    let f = fixture();
    assert_eq!(Config::selected_model_file(&f.settings), None);
    Config::set_selected_model_file(&f.settings, Some("ggml-small.en.bin"));
    assert_eq!(
        Config::selected_model_file(&f.settings).as_deref(),
        Some("ggml-small.en.bin")
    );
    Config::set_selected_model_file(&f.settings, None);
    assert_eq!(Config::selected_model_file(&f.settings), None);
}

/// A selected model that is no longer installed must not short-circuit the
/// search to a dangling path.
#[test]
fn selected_model_that_is_not_installed_falls_through() {
    let f = fixture();
    let ghost = format!("ggml-not-a-real-model-{}.bin", std::process::id());
    Config::set_selected_model_file(&f.settings, Some(&ghost));
    if let Some(found) = f.find_model(None) {
        assert!(found.exists());
        assert_ne!(
            found.file_name().and_then(|n| n.to_str()),
            Some(ghost.as_str())
        );
    }
}

// MARK: - ModelCatalog

#[test]
fn default_spec_is_base_english() {
    assert_eq!(ModelCatalog::default_spec().file, "ggml-base.en.bin");
}

#[test]
fn download_urls_point_at_the_whisper_cpp_repo() {
    for spec in ModelCatalog::ALL {
        let url = ModelCatalog::download_url(&spec);
        assert_eq!(
            url,
            format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
                spec.file
            )
        );
        assert!(url.starts_with("https://"));
    }
}

#[test]
fn catalog_entries_are_unique_and_labelled() {
    let unique: HashSet<&str> = ModelCatalog::ALL.iter().map(|s| s.file).collect();
    assert_eq!(unique.len(), ModelCatalog::ALL.len());
    for spec in ModelCatalog::ALL {
        assert!(!spec.label.is_empty());
        assert!(spec.file.starts_with("ggml"));
        assert_eq!(
            Path::new(spec.file).extension().and_then(|e| e.to_str()),
            Some("bin")
        );
    }
}

#[test]
fn installed_url_is_nil_for_a_model_that_is_not_there() {
    let ghost = format!("ggml-not-a-real-model-{}.bin", std::process::id());
    assert_eq!(ModelCatalog::installed_path(&ghost), None);
}

#[test]
fn installed_url_returns_a_path_inside_a_search_directory() {
    let dirs: HashSet<PathBuf> = Config::models_dirs().into_iter().collect();
    for spec in ModelCatalog::ALL {
        let Some(path) = ModelCatalog::installed_path(spec.file) else {
            continue;
        };
        assert!(dirs.contains(path.parent().expect("model has a parent dir")));
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(spec.file));
        assert!(path.exists());
    }
}

// MARK: - Hotkey

#[test]
fn hotkey_key_codes_are_distinct() {
    let codes: HashSet<i64> = Hotkey::ALL.iter().map(|hk| hk.key_code()).collect();
    assert_eq!(codes.len(), Hotkey::ALL.len());
}

#[test]
fn hotkey_raw_values_round_trip() {
    for hk in Hotkey::ALL {
        assert_eq!(Hotkey::from_raw(hk.raw_value()), Some(hk));
        assert!(!hk.label().is_empty());
        assert!(!hk.short_label().is_empty());
    }
}

#[test]
fn unknown_hotkey_raw_value_does_not_resolve() {
    assert_eq!(Hotkey::from_raw("leftPinky"), None);
}

/// Raw values are what the Swift app wrote to UserDefaults; changing them
/// would silently reset every migrated user's talk key.
#[test]
fn hotkey_raw_values_match_the_swift_user_defaults_strings() {
    assert_eq!(Hotkey::RightOption.raw_value(), "rightOption");
    assert_eq!(Hotkey::RightCommand.raw_value(), "rightCommand");
    assert_eq!(Hotkey::Fn.raw_value(), "fn");
    assert_eq!(Hotkey::RightControl.raw_value(), "rightControl");
}

/// fn cannot be delivered to apps off macOS, and Right Control has no
/// macOS-side hook, so each platform offers a different trio. The order is
/// the order the Talk key select shows.
#[test]
fn available_hotkeys_depend_on_the_platform() {
    let available = Hotkey::available();
    if cfg!(target_os = "macos") {
        assert_eq!(
            available,
            vec![Hotkey::RightOption, Hotkey::RightCommand, Hotkey::Fn]
        );
    } else {
        assert_eq!(
            available,
            vec![
                Hotkey::RightOption,
                Hotkey::RightControl,
                Hotkey::RightCommand
            ]
        );
    }
    for hk in &available {
        assert!(Hotkey::ALL.contains(hk));
    }
}

/// Labels use the platform's own key names: glyphs on macOS, plain words
/// elsewhere, and the Command key is "Win" on Windows but "Super" on Linux.
#[test]
fn hotkey_labels_use_the_platform_key_names() {
    let labels: Vec<String> = Hotkey::ALL.iter().map(|hk| hk.label()).collect();
    let short: Vec<String> = Hotkey::ALL.iter().map(|hk| hk.short_label()).collect();
    if cfg!(target_os = "macos") {
        assert_eq!(
            labels,
            ["Right ⌥ Option", "Right ⌘ Command", "fn", "Right ⌃ Control"]
        );
        assert_eq!(short, ["Right ⌥", "Right ⌘", "fn", "Right ⌃"]);
    } else if cfg!(target_os = "windows") {
        assert_eq!(labels, ["Right Alt", "Right Win", "fn", "Right Ctrl"]);
        assert_eq!(short, labels);
    } else {
        assert_eq!(labels, ["Right Alt", "Right Super", "fn", "Right Ctrl"]);
        assert_eq!(short, labels);
    }
}

// MARK: - Settings-backed config (Swift read UserDefaults directly; these pin
// the defaults the app relied on there)

#[test]
fn hotkey_defaults_to_right_option_and_unknown_raw_falls_back() {
    let f = fixture();
    assert_eq!(Config::hotkey(&f.settings), Hotkey::RightOption);
    f.settings.set("hotkey", "leftPinky");
    assert_eq!(Config::hotkey(&f.settings), Hotkey::RightOption);
    Config::set_hotkey(&f.settings, Hotkey::RightCommand);
    assert_eq!(Config::hotkey(&f.settings), Hotkey::RightCommand);
    assert_eq!(
        f.settings.get_string("hotkey").as_deref(),
        Some("rightCommand")
    );
}

#[test]
fn sounds_and_trailing_space_default_to_on() {
    let f = fixture();
    assert!(Config::sounds_enabled(&f.settings));
    assert!(Config::trailing_space(&f.settings));
    Config::set_sounds_enabled(&f.settings, false);
    assert!(!Config::sounds_enabled(&f.settings));
    f.settings.set("trailingSpace", false);
    assert!(!Config::trailing_space(&f.settings));
}
