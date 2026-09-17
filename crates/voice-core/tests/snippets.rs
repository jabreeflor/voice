//! Port of Tests/VoiceCoreTests/SnippetTests.swift.
//!
//! `SnippetStore::expand` rewrites the transcript before it is pasted, so a
//! mistake here silently corrupts the user's text. These tests always use a
//! throwaway directory — never `Store::dir()`.

use std::fs;

use tempfile::TempDir;
use voice_core::{Snippet, SnippetStore};

struct Fixture {
    dir: TempDir,
    store: SnippetStore,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SnippetStore::new(dir.path().to_path_buf());
        Fixture { dir, store }
    }

    fn other_store(&self) -> SnippetStore {
        SnippetStore::new(self.dir.path().to_path_buf())
    }

    fn snippets_file(&self) -> std::path::PathBuf {
        self.dir.path().join("snippets.json")
    }
}

fn snippet(trigger: &str, text: &str) -> Snippet {
    Snippet {
        trigger: trigger.to_string(),
        text: text.to_string(),
    }
}

fn triggers(store: &SnippetStore) -> Vec<&str> {
    store
        .snippets()
        .iter()
        .map(|s| s.trigger.as_str())
        .collect()
}

// MARK: - Passthrough

#[test]
fn empty_store_returns_transcript_unchanged() {
    let mut f = Fixture::new();
    assert_eq!(
        f.store.expand("nothing to expand here"),
        "nothing to expand here"
    );
    assert_eq!(f.store.expand(""), "");
}

#[test]
fn unrelated_transcript_is_unchanged() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(f.store.expand("let's meet at noon"), "let's meet at noon");
}

#[test]
fn non_matching_transcript_is_returned_byte_for_byte() {
    let mut f = Fixture::new();
    f.store.add("sig", "Jabree");
    let text = "Punctuation, casing and  spacing are left alone.";
    assert_eq!(f.store.expand(text), text);
}

// MARK: - Whole-utterance triggers

#[test]
fn whole_utterance_trigger_returns_snippet_verbatim() {
    let mut f = Fixture::new();
    f.store.add("my email", "jabreenicholas@gmail.com");
    assert_eq!(f.store.expand("my email"), "jabreenicholas@gmail.com");
}

#[test]
fn whole_utterance_match_ignores_case() {
    let mut f = Fixture::new();
    f.store.add("my email", "jabreenicholas@gmail.com");
    assert_eq!(f.store.expand("My Email"), "jabreenicholas@gmail.com");
    assert_eq!(f.store.expand("MY EMAIL"), "jabreenicholas@gmail.com");
}

/// Whisper punctuates a lone phrase as a sentence, so "my email." has to
/// count as the bare trigger.
#[test]
fn whole_utterance_match_strips_trailing_punctuation() {
    let mut f = Fixture::new();
    f.store.add("my email", "jabreenicholas@gmail.com");
    for spoken in [
        "my email.",
        "my email!",
        "my email?",
        "my email,",
        "my email...",
    ] {
        assert_eq!(
            f.store.expand(spoken),
            "jabreenicholas@gmail.com",
            "failed for {spoken}"
        );
    }
}

#[test]
fn whole_utterance_match_ignores_surrounding_whitespace() {
    let mut f = Fixture::new();
    f.store.add("my email", "jabreenicholas@gmail.com");
    assert_eq!(f.store.expand("   my email  "), "jabreenicholas@gmail.com");
    assert_eq!(f.store.expand("\nmy email\n"), "jabreenicholas@gmail.com");
}

/// The whole-utterance path returns the stored text directly, so multi-line
/// snippets and regex-looking characters come through untouched.
#[test]
fn whole_utterance_snippet_is_not_escaped_or_reflowed() {
    let mut f = Fixture::new();
    let block = "Best,\nJabree\n\nP.S. cost is $1 (50% off) [see notes]";
    f.store.add("signoff", block);
    assert_eq!(f.store.expand("signoff."), block);
}

// MARK: - Mid-sentence replacement

#[test]
fn trigger_inside_sentence_is_replaced_in_place() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(
        f.store.expand("okay brb see you soon"),
        "okay be right back see you soon"
    );
}

#[test]
fn mid_sentence_replacement_ignores_case() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(f.store.expand("okay BRB soon"), "okay be right back soon");
    assert_eq!(f.store.expand("okay Brb soon"), "okay be right back soon");
}

#[test]
fn all_occurrences_are_replaced() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(
        f.store.expand("brb and then brb again"),
        "be right back and then be right back again"
    );
}

#[test]
fn replacement_respects_word_boundaries() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(f.store.expand("abrb"), "abrb");
    assert_eq!(f.store.expand("brbx"), "brbx");
    assert_eq!(f.store.expand("hyperbrbole"), "hyperbrbole");
}

#[test]
fn trigger_adjacent_to_punctuation_still_expands() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    assert_eq!(
        f.store.expand("wait, brb, then we talk"),
        "wait, be right back, then we talk"
    );
}

/// The replacement runs through the regex engine, so a snippet containing
/// "$1" must not be read as a capture-group reference.
#[test]
fn dollar_signs_in_snippet_text_are_not_treated_as_template_references() {
    let mut f = Fixture::new();
    f.store.add("price", "cost $1 per seat");
    assert_eq!(
        f.store.expand("the price is fixed"),
        "the cost $1 per seat is fixed"
    );
}

/// `\w` is Unicode in the Swift regex, so an accented letter next to the
/// trigger is a word character, and a rejected match must not hide a valid
/// one that starts inside it (the "a-a" in "ba-a-a" case). This only uses a
/// precomposed U+00E9; it pins the spec's "alphanumeric or `_`" rule, not
/// full ICU `\w` parity (see `is_word_char` in store.rs).
#[test]
fn boundaries_are_unicode_aware_and_overlapping_matches_are_found() {
    let mut f = Fixture::new();
    f.store.add("brb", "BRB");
    assert_eq!(f.store.expand("ébrb"), "ébrb");
    assert_eq!(f.store.expand("brbé"), "brbé");
    assert_eq!(f.store.expand("é brb é"), "é BRB é");

    f.store.add("a-a", "X");
    assert_eq!(f.store.expand("ba-a-a"), "ba-X");
}

// MARK: - Longest trigger wins

#[test]
fn longer_trigger_is_applied_before_its_shorter_prefix() {
    let mut f = Fixture::new();
    f.store.add("my", "MY");
    f.store.add("my email", "EMAIL");
    assert_eq!(f.store.expand("send my email now"), "send EMAIL now");
}

#[test]
fn shorter_trigger_still_applies_where_the_long_one_does_not() {
    let mut f = Fixture::new();
    f.store.add("my", "MY");
    f.store.add("my email", "EMAIL");
    assert_eq!(f.store.expand("this is my house"), "this is MY house");
}

// MARK: - Regex-special characters in triggers

#[test]
fn trigger_with_plus_signs_does_not_crash_and_matches_whole_utterance() {
    let mut f = Fixture::new();
    f.store.add("c++", "C plus plus");
    assert_eq!(f.store.expand("c++"), "C plus plus");
    assert_eq!(f.store.expand("C++."), "C plus plus");
}

/// Regression: symbol-edged triggers use (?<!\w)…(?!\w) lookarounds instead
/// of \b, so "c++" expands mid-sentence. Reverting to \b would break this.
#[test]
fn trigger_ending_in_non_word_character_expands_mid_sentence() {
    let mut f = Fixture::new();
    f.store.add("c++", "C plus plus");
    assert_eq!(
        f.store.expand("I write c++ every day"),
        "I write C plus plus every day"
    );
}

#[test]
fn symbol_edged_trigger_still_respects_word_boundaries() {
    let mut f = Fixture::new();
    f.store.add("c++", "C plus plus");
    assert_eq!(f.store.expand("see c++x compile"), "see c++x compile");
}

#[test]
fn trigger_with_ampersand_expands_mid_sentence() {
    let mut f = Fixture::new();
    f.store.add("q&a", "questions and answers");
    assert_eq!(
        f.store.expand("the q&a session starts now"),
        "the questions and answers session starts now"
    );
    assert_eq!(f.store.expand("q&a"), "questions and answers");
}

/// A "." in a trigger must be a literal dot, not the regex any-character
/// wildcard.
#[test]
fn dot_in_trigger_is_literal_not_a_wildcard() {
    let mut f = Fixture::new();
    f.store.add("a.b", "MATCHED");
    assert_eq!(f.store.expand("axb"), "axb");
    assert_eq!(f.store.expand("a.b"), "MATCHED");
}

#[test]
fn assorted_regex_metacharacters_in_triggers_are_safe() {
    let mut f = Fixture::new();
    let triggers = [
        "c++",
        "q&a",
        "a.b",
        "(paren)",
        "[bracket]",
        "a|b",
        "x*y",
        "^caret",
        "dollar$",
        "back\\slash",
        "a?b",
        "{brace}",
    ];
    for (i, t) in triggers.iter().enumerate() {
        f.store.add(t, &format!("EXPANSION{i}"));
    }
    // Whole-utterance matching works for all of them, and nothing panics.
    for (i, t) in triggers.iter().enumerate() {
        assert_eq!(
            f.store.expand(t),
            format!("EXPANSION{i}"),
            "whole-utterance failed for {t}"
        );
    }
    // An unrelated sentence must survive every one of those patterns.
    assert_eq!(
        f.store.expand("a perfectly ordinary sentence"),
        "a perfectly ordinary sentence"
    );
}

#[test]
fn metacharacter_triggers_do_not_match_arbitrary_text() {
    let mut f = Fixture::new();
    f.store.add("x*y", "STAR");
    assert_eq!(f.store.expand("xy"), "xy");
    assert_eq!(f.store.expand("xxxy"), "xxxy");
}

// MARK: - add / remove

#[test]
fn add_lowercases_and_trims_the_trigger() {
    let mut f = Fixture::new();
    f.store.add("  \"My Email\" ", "x@y.com");
    assert_eq!(
        f.store.snippets().first().map(|s| s.trigger.as_str()),
        Some("my email")
    );
}

#[test]
fn add_rejects_empty_trigger_or_text() {
    let mut f = Fixture::new();
    assert!(!f.store.add("", "something"));
    assert!(!f.store.add("   ", "something"));
    assert!(!f.store.add("trigger", ""));
    assert!(f.store.snippets().is_empty());
}

#[test]
fn adding_same_trigger_replaces_the_old_snippet() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    f.store.add("brb", "back in a bit");
    assert_eq!(f.store.snippets().len(), 1);
    assert_eq!(f.store.expand("brb"), "back in a bit");
}

/// Swift also passes -1; `usize` cannot, so `usize::MAX` stands in for the
/// "wildly out of range" index.
#[test]
fn remove_deletes_by_index_and_ignores_out_of_range() {
    let mut f = Fixture::new();
    f.store.add("a", "AAA");
    f.store.add("b", "BBB");
    f.store.remove_at(0);
    assert_eq!(triggers(&f.store), ["b"]);
    f.store.remove_at(99);
    f.store.remove_at(usize::MAX);
    assert_eq!(f.store.snippets().len(), 1);
}

#[test]
fn stamp_advances_on_mutation_only() {
    let mut f = Fixture::new();
    let start = f.store.stamp();
    f.store.add("brb", "be right back");
    assert_eq!(f.store.stamp(), start + 1);
    f.store.add("", ""); // rejected
    assert_eq!(f.store.stamp(), start + 1);
    f.store.remove_at(0);
    assert_eq!(f.store.stamp(), start + 2);
}

// MARK: - Persistence

#[test]
fn snippets_survive_a_reload() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    f.store.add("my email", "x@y.com");

    let mut reloaded = f.other_store();
    assert_eq!(reloaded.snippets(), f.store.snippets());
    assert_eq!(reloaded.expand("okay brb"), "okay be right back");
    assert_eq!(reloaded.expand("my email."), "x@y.com");
}

#[test]
fn removal_is_persisted() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    f.store.remove_at(0);
    assert!(f.other_store().snippets().is_empty());
}

#[test]
fn fresh_directory_starts_empty() {
    let other = tempfile::tempdir().expect("temp dir");
    assert!(SnippetStore::new(other.path().to_path_buf())
        .snippets()
        .is_empty());
}

// MARK: - External edits (voicectl writes the same file)

#[test]
fn reload_if_changed_picks_up_another_stores_write() {
    let mut f = Fixture::new();
    let mut writer = f.other_store();
    let stamp = f.store.stamp();
    assert!(!f.store.reload_if_changed(), "nothing changed yet");

    writer.add("brb", "be right back");
    assert!(f.store.reload_if_changed());
    assert_eq!(f.store.snippets(), writer.snippets());
    assert_eq!(
        f.store.stamp(),
        stamp + 1,
        "the UI keys its rebuild off the stamp"
    );
    assert!(!f.store.reload_if_changed(), "a second call is a no-op");
}

#[test]
fn expand_and_mutations_see_external_changes_without_an_explicit_reload() {
    let mut f = Fixture::new();
    let mut writer = f.other_store();
    writer.add("brb", "be right back");
    assert_eq!(f.store.expand("okay brb"), "okay be right back");

    // A stale in-memory list must not clobber the other writer's snippet.
    f.store.add("sig", "Jabree");
    assert_eq!(triggers(&f.other_store()), ["brb", "sig"]);
}

#[test]
fn deleted_file_empties_the_store_on_reload() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    fs::remove_file(f.snippets_file()).expect("remove");
    assert!(f.store.reload_if_changed());
    assert!(f.store.snippets().is_empty());
}

#[test]
fn own_save_does_not_count_as_an_external_change() {
    let mut f = Fixture::new();
    f.store.add("brb", "be right back");
    let stamp = f.store.stamp();
    assert!(!f.store.reload_if_changed());
    assert_eq!(f.store.stamp(), stamp);
}

#[test]
fn remove_by_trigger_normalizes_and_reports_missing() {
    let mut f = Fixture::new();
    f.store.add("my email", "x@y.com");
    assert!(!f.store.remove_trigger("nope"));
    assert!(f.store.remove_trigger(" \"My Email\" "));
    assert!(f.store.snippets().is_empty());
}

#[test]
fn snippet_for_trigger_normalizes() {
    let mut f = Fixture::new();
    f.store.add("my email", "x@y.com");
    assert_eq!(
        f.store.snippet_for("MY EMAIL").map(|s| s.text.as_str()),
        Some("x@y.com")
    );
    assert!(f.store.snippet_for("other").is_none());
}

#[test]
fn replace_all_normalizes_drops_empties_and_dedupes() {
    let mut f = Fixture::new();
    f.store.add("old", "OLD");
    f.store.replace_all(&[
        snippet(" A ", "first"),
        snippet("", "dropped"),
        snippet("b", ""),
        snippet("a", "second"),
    ]);
    assert_eq!(f.store.snippets(), [snippet("a", "second")]);
    assert_eq!(f.other_store().snippets(), f.store.snippets());
}

/// load() does not normalize, so an empty trigger can only reach the matcher
/// from a hand-edited snippets.json; its zero-width match must not spin
/// forever. "!!" trims to "" and takes the whole-utterance path; "! x" does
/// not, so it exercises replace_bounded, whose `break` exists solely to stop
/// the zero-width match from looping. It deliberately stops after the first
/// insertion rather than reproducing Swift's replacingOccurrences, which
/// inserted the text at every word boundary ("ZZZ!ZZZ x") - nonsense output
/// that is not worth matching.
#[test]
fn empty_trigger_from_file_does_not_hang_expand() {
    let f = Fixture::new();
    fs::write(f.snippets_file(), r#"[{"trigger":"","text":"ZZZ"}]"#).expect("write");
    let mut store = f.other_store();
    assert_eq!(store.expand("!!"), "ZZZ");
    assert_eq!(store.expand("! x"), "ZZZ! x");
}

#[test]
fn corrupt_snippet_file_leaves_store_empty_rather_than_crashing() {
    let f = Fixture::new();
    fs::write(f.snippets_file(), "not json").expect("write");
    let mut broken = f.other_store();
    assert!(broken.snippets().is_empty());
    assert_eq!(broken.expand("hello"), "hello");
}
