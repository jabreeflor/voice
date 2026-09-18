//! `clean_transcript` is the last thing that touches whisper output before it is
//! pasted into the user's document, so its job is to strip the model's
//! non-speech annotations without disturbing real dictation.
//! Port of Tests/VoiceCoreTests/TranscriptTests.swift.

use voice_core::clean_transcript;

// Bracketed artifacts

#[test]
fn removes_blank_audio_marker() {
    assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
}

#[test]
fn removes_every_known_artifact() {
    let artifacts = [
        "[BLANK_AUDIO]",
        "[INAUDIBLE]",
        "[MUSIC]",
        "[SILENCE]",
        "[NOISE]",
        "[TYPING]",
        "(blank audio)",
        "(silence)",
        "(music)",
        "(noise)",
        "(typing)",
        "[MUSIC PLAYING]",
        "(music playing)",
        "[SOUND]",
        "♪",
    ];
    for a in artifacts {
        assert_eq!(clean_transcript(a), "", "artifact {a} survived cleaning");
        assert_eq!(
            clean_transcript(&format!("hello {a} world")),
            "hello world",
            "artifact {a} was not stripped from mid-sentence"
        );
    }
}

#[test]
fn artifact_matching_is_case_insensitive() {
    assert_eq!(clean_transcript("[blank_audio]"), "");
    assert_eq!(clean_transcript("[Music Playing]"), "");
    assert_eq!(clean_transcript("(SILENCE)"), "");
}

#[test]
fn removes_repeated_and_mixed_artifacts() {
    assert_eq!(clean_transcript("[BLANK_AUDIO][BLANK_AUDIO][MUSIC]"), "");
    assert_eq!(
        clean_transcript("[MUSIC] send the file [TYPING] tomorrow ♪"),
        "send the file tomorrow"
    );
}

/// Only the listed markers are stripped. Anything else in brackets is left
/// alone rather than guessed at, so unknown annotations reach the user.
#[test]
fn unknown_bracketed_text_is_preserved() {
    assert_eq!(
        clean_transcript("[LAUGHTER] that was good"),
        "[LAUGHTER] that was good"
    );
    assert_eq!(clean_transcript("see item [3] below"), "see item [3] below");
}

// Whitespace

#[test]
fn collapses_runs_of_spaces() {
    assert_eq!(clean_transcript("hello     world"), "hello world");
    assert_eq!(clean_transcript("a  b   c    d"), "a b c d");
}

#[test]
fn newlines_become_spaces() {
    assert_eq!(
        clean_transcript("first line\nsecond line"),
        "first line second line"
    );
    assert_eq!(clean_transcript("a\n\n\nb"), "a b");
}

#[test]
fn trims_leading_and_trailing_whitespace() {
    assert_eq!(clean_transcript("   hello world   "), "hello world");
    assert_eq!(clean_transcript("\n\thello\n"), "hello");
}

/// Whisper emits a leading space on nearly every segment; removing an
/// artifact then leaves a double space where the marker used to be.
#[test]
fn gap_left_by_removed_artifact_is_closed() {
    assert_eq!(
        clean_transcript(" [BLANK_AUDIO] send it now"),
        "send it now"
    );
    assert_eq!(clean_transcript("send it [SILENCE] now"), "send it now");
}

// Empty and whitespace-only input

#[test]
fn empty_input_stays_empty() {
    assert_eq!(clean_transcript(""), "");
}

#[test]
fn whitespace_only_input_becomes_empty() {
    assert_eq!(clean_transcript("   "), "");
    assert_eq!(clean_transcript("\n \t \n"), "");
}

// Real dictation is untouched

#[test]
fn ordinary_sentence_is_unchanged() {
    let sentence = "Let's ship the release on Friday.";
    assert_eq!(clean_transcript(sentence), sentence);
}

#[test]
fn punctuation_and_casing_are_preserved() {
    let sentence = "Hey Dr. Chen — can you review PR #42? Thanks!";
    assert_eq!(clean_transcript(sentence), sentence);
}

#[test]
fn single_spaces_between_words_are_not_altered() {
    let sentence = "one two three four five";
    assert_eq!(clean_transcript(sentence), sentence);
}

#[test]
fn unicode_and_emoji_survive() {
    assert_eq!(clean_transcript("café naïve 🎉"), "café naïve 🎉");
}

/// Not in the Swift suite. Rust has no `.caseInsensitive` replace, so the port
/// matches case-insensitively by hand; this pins that a character whose
/// lowercase form has a different UTF-8 length (İ → i̇) cannot throw the
/// match offsets off or make an uppercase marker slip through.
#[test]
fn artifacts_are_stripped_next_to_length_changing_unicode() {
    assert_eq!(clean_transcript("İstanbul [MUSIC] trip"), "İstanbul trip");
    assert_eq!(clean_transcript("[music] İstanbul"), "İstanbul");
}

// Idempotence

#[test]
fn cleaning_twice_matches_cleaning_once() {
    let raw = "  [MUSIC]  hold on\n\n I'll check [TYPING] now ♪  ";
    let once = clean_transcript(raw);
    assert_eq!(clean_transcript(&once), once);
    assert_eq!(once, "hold on I'll check now");
}
