//! `VocabularyStore`: vocabulary.json, edited from the app and from
//! `voicectl vocab`. Every test uses a throwaway directory.

use std::fs;

use voice_core::VocabularyStore;

fn store_in(dir: &tempfile::TempDir) -> VocabularyStore {
    VocabularyStore::new(dir.path().to_path_buf())
}

#[test]
fn starts_empty_and_lives_next_to_snippets() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = store_in(&dir);
    assert!(store.words().is_empty());
    assert_eq!(store.file_path(), dir.path().join("vocabulary.json"));
}

#[test]
fn add_keeps_spelling_and_persists_as_a_json_array() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    assert!(store.add("  \"Jabree\" "));
    assert!(store.add("whisper.cpp"));
    assert_eq!(store.words(), ["Jabree", "whisper.cpp"]);
    let on_disk = fs::read_to_string(store.file_path()).expect("read");
    assert_eq!(on_disk, r#"["Jabree","whisper.cpp"]"#);
    // A fresh store sees the same list.
    assert_eq!(store_in(&dir).words(), ["Jabree", "whisper.cpp"]);
}

#[test]
fn add_rejects_empty_words() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    assert!(!store.add(""));
    assert!(!store.add("   "));
    assert!(!store.add("\"\""));
    assert!(store.words().is_empty());
    assert!(!store.file_path().exists());
}

#[test]
fn adding_a_case_variant_respells_in_place() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    store.add("jabree");
    store.add("Tauri");
    let stamp = store.stamp();
    store.add("Jabree");
    assert_eq!(store.words(), ["Jabree", "Tauri"]);
    assert!(store.stamp() > stamp);
    // Re-adding the identical spelling is a no-op.
    let stamp = store.stamp();
    store.add("Tauri");
    assert_eq!(store.stamp(), stamp);
}

#[test]
fn remove_is_case_insensitive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    store.add("Kubernetes");
    assert!(store.remove("kubernetes"));
    assert!(store.words().is_empty());
    assert!(!store.remove("kubernetes"));
}

#[test]
fn replace_all_normalizes_and_dedupes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    store.add("Old");
    let list: Vec<String> = ["Ollama", " ", "ollama", "Grafana  Loki"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    store.replace_all(&list);
    assert_eq!(store.words(), ["Ollama", "Grafana Loki"]);
}

#[test]
fn reload_picks_up_external_edits() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app_side = store_in(&dir);
    app_side.add("Tauri");
    // voicectl (another process) writes through its own store.
    let mut cli_side = store_in(&dir);
    cli_side.add("Ollama");
    assert!(app_side.reload_if_changed());
    assert_eq!(app_side.words(), ["Tauri", "Ollama"]);
    assert!(!app_side.reload_if_changed());
}

#[test]
fn stale_store_does_not_clobber_another_writer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut a = store_in(&dir);
    let mut b = store_in(&dir);
    a.add("One");
    b.add("Two");
    assert_eq!(store_in(&dir).words(), ["One", "Two"]);
}

#[test]
fn corrupt_file_reads_as_empty() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("vocabulary.json"), b"{not json").expect("write");
    assert!(store_in(&dir).words().is_empty());
}

#[test]
fn find_returns_the_stored_spelling() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    store.add("PostgreSQL");
    assert_eq!(store.find("postgresql"), Some("PostgreSQL"));
    assert_eq!(store.find("mysql"), None);
}

#[test]
fn csv_preview_and_commit_preserve_existing_entries_and_reload_as_hints() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    assert!(store.add("Tauri"));
    let csv = "\u{feff}tauri\r\n\"Acme, Inc.\"\r\nLee \"broken\"\r\n\"Dr. \"\"AJ\"\" Lee\"\r\nACME, INC.\r\n\"acme, inc.\"\r\n\r\n\"unclosed\r\nZürich";
    let preview = store.import_csv(csv, None).expect("preview");
    assert_eq!(preview.words, ["Acme, Inc.", "Dr. \"AJ\" Lee", "Zürich"]);
    assert_eq!(preview.duplicates, 2);
    assert_eq!(preview.invalid, 4);
    assert_eq!(store_in(&dir).words(), ["Tauri"]);

    let committed = store.import_csv(csv, Some(preview.stamp)).expect("import");
    assert_eq!(committed.words, preview.words);
    assert_eq!(committed.duplicates, preview.duplicates);
    assert_eq!(committed.invalid, preview.invalid);
    let reopened = store_in(&dir);
    assert_eq!(
        reopened.words(),
        ["Tauri", "Acme, Inc.", "Dr. \"AJ\" Lee", "Zürich"]
    );
    assert_eq!(
        voice_core::engine::whisper_prompt(reopened.words(), &["my email".into()]),
        "Tauri, Acme, Inc., Dr. \"AJ\" Lee, Zürich, my email."
    );
    let repeated = store.import_csv(csv, None).expect("preview");
    assert!(repeated.words.is_empty());
    assert_eq!(repeated.duplicates, 5);
}

#[test]
fn csv_commit_requires_a_fresh_preview_after_external_changes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    let preview = store.import_csv("Tauri", None).expect("preview");
    assert!(store_in(&dir).add("Whisper"));
    assert!(store.import_csv("Tauri", Some(preview.stamp)).is_err());
    assert_eq!(store_in(&dir).words(), ["Whisper"]);
}

#[test]
fn failed_csv_write_keeps_the_store_unchanged() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = store_in(&dir);
    fs::create_dir(store.file_path()).expect("block writes");
    let preview = store.import_csv("Tauri", None).expect("preview");
    assert!(store.import_csv("Tauri", Some(preview.stamp)).is_err());
    assert!(store.words().is_empty());
    assert_eq!(store.stamp(), preview.stamp);
}
