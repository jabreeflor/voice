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
