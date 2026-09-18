//! Transcript cleanup: the last thing that touches whisper output before it is
//! pasted. Reference: `cleanTranscript` in Sources/VoiceCore/core.swift.

/// Non-speech markers whisper emits. Only these are stripped — unknown
/// `[brackets]` are left alone rather than guessed at, so unexpected
/// annotations reach the user instead of silently disappearing.
const ARTIFACTS: [&str; 15] = [
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

/// Strips the listed markers (case-insensitive), turns newlines into spaces,
/// collapses runs of spaces and trims.
pub fn clean_transcript(raw: &str) -> String {
    let mut t = raw.to_string();
    for artifact in ARTIFACTS {
        t = remove_case_insensitive(&t, artifact);
    }
    t = t.replace('\n', " ");
    while t.contains("  ") {
        t = t.replace("  ", " ");
    }
    t.trim().to_string()
}

/// Removes every case-insensitive occurrence of `needle` from `haystack`.
/// Case folding is ASCII-only, on bytes: the artifacts are ASCII apart from
/// "♪" (which has no case), and folding the haystack with `to_lowercase` would
/// shift byte offsets wherever a character's lowercase form has a different
/// UTF-8 length (e.g. "İ"). Every needle starts and ends on ASCII or on a
/// complete non-ASCII char, so a match always lands on char boundaries.
fn remove_case_insensitive(haystack: &str, needle: &str) -> String {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return haystack.to_string();
    }
    let bytes = haystack.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut from = 0;
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if bytes[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            out.push_str(&haystack[from..i]);
            i += needle.len();
            from = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&haystack[from..]);
    out
}
