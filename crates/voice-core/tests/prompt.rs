//! `whisper_prompt`: the vocabulary + snippet-trigger line sent to the local
//! whisper-server as its `prompt`. Pure; no server involved (the request
//! layout is pinned in engine.rs and by the fake-server test in
//! engine_integration.rs).

use voice_core::{whisper_prompt, PROMPT_CHAR_BUDGET};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn nothing_to_send_is_an_empty_prompt() {
    assert_eq!(whisper_prompt(&[], &[]), "");
    // Whitespace-only entries count as nothing, too.
    assert_eq!(whisper_prompt(&strings(&["  ", ""]), &strings(&["\t"])), "");
}

#[test]
fn vocabulary_then_triggers_as_one_sentence() {
    let prompt = whisper_prompt(
        &strings(&["Jabree", "Tauri", "whisper.cpp"]),
        &strings(&["my email", "standup"]),
    );
    assert_eq!(prompt, "Jabree, Tauri, whisper.cpp, my email, standup.");
}

#[test]
fn spelling_is_kept_and_whitespace_collapsed() {
    let prompt = whisper_prompt(&strings(&["  GitHub   Actions ", "iOS\n17"]), &[]);
    assert_eq!(prompt, "GitHub Actions, iOS 17.");
}

#[test]
fn case_insensitive_duplicates_keep_the_first_spelling() {
    // A snippet trigger is stored lowercased; the vocabulary entry's casing
    // is the one whisper should copy.
    let prompt = whisper_prompt(
        &strings(&["Ollama", "OLLAMA"]),
        &strings(&["ollama", "deploy"]),
    );
    assert_eq!(prompt, "Ollama, deploy.");
}

#[test]
fn no_double_period_when_last_term_ends_a_sentence() {
    assert_eq!(whisper_prompt(&strings(&["Yahoo!"]), &[]), "Yahoo!");
    assert_eq!(whisper_prompt(&strings(&["Inc."]), &[]), "Inc.");
}

#[test]
fn long_lists_are_trimmed_to_the_budget_keeping_earlier_entries() {
    let vocab: Vec<String> = (0..200).map(|i| format!("Term{i:03}")).collect();
    let prompt = whisper_prompt(&vocab, &strings(&["late trigger"]));
    assert!(
        prompt.chars().count() <= PROMPT_CHAR_BUDGET,
        "{} chars",
        prompt.chars().count()
    );
    assert!(prompt.starts_with("Term000, Term001, Term002"));
    assert!(prompt.ends_with('.'));
    // Earlier entries win: the triggers come after 200 terms and do not fit.
    assert!(!prompt.contains("late trigger"));
    // Nothing is cut mid-term.
    for piece in prompt.trim_end_matches('.').split(", ") {
        assert!(vocab.contains(&piece.to_string()), "partial term {piece:?}");
    }
}

#[test]
fn an_oversized_term_is_skipped_but_later_ones_still_fit() {
    let huge = "x".repeat(PROMPT_CHAR_BUDGET + 10);
    let prompt = whisper_prompt(&strings(&["Jabree", &huge, "Tauri"]), &[]);
    assert_eq!(prompt, "Jabree, Tauri.");
}

#[test]
fn budget_counts_characters_not_bytes() {
    // 4-byte emoji / 2-byte accents must not shrink what fits.
    let term = "é".repeat(PROMPT_CHAR_BUDGET - 1);
    assert_eq!(
        whisper_prompt(std::slice::from_ref(&term), &[]),
        format!("{term}.")
    );
}
