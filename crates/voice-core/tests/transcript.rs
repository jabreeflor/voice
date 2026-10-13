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

// MARK: - remove_fillers

use voice_core::remove_fillers;

/// (whisper output after `clean_transcript`, what gets pasted). The PR's
/// before/after table is printed from this list.
const FILLER_CASES: &[(&str, &str)] = &[
    // Plain removals.
    (
        "Um, so I think we should ship it.",
        "So I think we should ship it.",
    ),
    (
        "I, uh, think we should ship it.",
        "I think we should ship it.",
    ),
    ("We should go, um.", "We should go."),
    ("Is it, um, ready?", "Is it ready?"),
    ("So um I think", "So I think"),
    ("I was um, going to say", "I was going to say"),
    ("Uh, um, okay.", "Okay."),
    ("Umm, let's start.", "Let's start."),
    ("Uhh, wait.", "Wait."),
    ("Uhm, maybe.", "Maybe."),
    ("Erm, not sure.", "Not sure."),
    ("Ummmm, yes.", "Yes."),
    ("That's it. Uh, next item.", "That's it. Next item."),
    ("That's it. Um. Next item.", "That's it. Next item."),
    ("Um... I think so.", "I think so."),
    ("Uh… fine.", "Fine."),
    ("It costs, uh, five dollars.", "It costs five dollars."),
    ("Did you, uh, eat? Um, I did.", "Did you eat? I did."),
    ("UM, so, uh, what now?", "UM, so what now?"),
    ("um", ""),
    ("Um.", ""),
    ("Uh, um.", ""),
    ("", ""),
    // Capitals after removal.
    ("Um, the plan is simple.", "The plan is simple."),
    ("Um, iPhone sales are up.", "iPhone sales are up."),
    ("Uh, eBay listed it.", "eBay listed it."),
    ("Um, 42 is the answer.", "42 is the answer."),
    ("Um, I agree.", "I agree."),
    ("Done! Uh, what's next?", "Done! What's next?"),
    // Words that contain the letters stay.
    (
        "Uh, the umbrella is in the album.",
        "The umbrella is in the album.",
    ),
    ("The drummer hummed a tune.", "The drummer hummed a tune."),
    ("Summer, humor and Uhura.", "Summer, humor and Uhura."),
    ("Ermine and term papers.", "Ermine and term papers."),
    ("The vacuum is on.", "The vacuum is on."),
    // Hyphenated, quoted and two-word forms are speech, not hesitation.
    ("Uh-oh, that broke.", "Uh-oh, that broke."),
    ("Uh-huh, sounds good.", "Uh-huh, sounds good."),
    ("Uh huh, sounds good.", "Uh huh, sounds good."),
    ("Uh oh.", "Uh oh."),
    ("He said \"um\" twice.", "He said \"um\" twice."),
    ("Stop saying 'uh'.", "Stop saying 'uh'."),
    ("The um's are gone.", "The um's are gone."),
    // All-caps tokens are acronyms.
    ("I went to UM for college.", "I went to UM for college."),
    ("ERM is our risk framework.", "ERM is our risk framework."),
    // Unrelated text is untouched.
    ("Like, you know, I mean it.", "Like, you know, I mean it."),
    ("Hmm, let me think.", "Hmm, let me think."),
    ("Take her to the ER.", "Take her to the ER."),
];

#[test]
fn filler_cases() {
    for (before, after) in FILLER_CASES {
        assert_eq!(remove_fillers(before), *after, "input {before:?}");
    }
}

#[test]
fn removing_fillers_twice_changes_nothing_more() {
    for (before, _) in FILLER_CASES {
        let once = remove_fillers(before);
        assert_eq!(remove_fillers(&once), once, "input {before:?}");
    }
}

#[test]
fn fillers_text_without_fillers_is_returned_verbatim() {
    let text = "Ship the Tauri build tonight, then ping Jabree.";
    assert_eq!(remove_fillers(text), text);
}

#[test]
fn fillers_composes_with_clean_transcript() {
    let raw = " Um, [BLANK_AUDIO] so\nuh, we ship it. ";
    assert_eq!(remove_fillers(&clean_transcript(raw)), "So we ship it.");
}

#[test]
fn fillers_non_ascii_neighbours_are_safe() {
    // Multi-byte characters right next to removal points must not split a
    // char or panic.
    assert_eq!(remove_fillers("Uh, élan vital."), "Élan vital.");
    assert_eq!(remove_fillers("Café, um, au lait."), "Café au lait.");
    assert_eq!(remove_fillers("Um, 日本語 text."), "日本語 text.");
}

/// Pinned so nobody mistakes it for a bug: the cleaner is English-only.
/// "um" is a real word in Portuguese and German, which is why
/// `removeFillers` is a setting (README says to turn it off for those).
#[test]
fn fillers_is_english_only_by_design() {
    assert_eq!(remove_fillers("Comprei um carro."), "Comprei carro.");
    assert_eq!(
        remove_fillers("Wir treffen uns um 5 Uhr."),
        "Wir treffen uns 5 Uhr."
    );
}
