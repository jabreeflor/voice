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
/// Matching is done on a lowercased copy; the artifacts are ASCII apart from
/// "♪", so lowercasing never changes byte offsets relative to the original.
fn remove_case_insensitive(haystack: &str, needle: &str) -> String {
    let lower = haystack.to_lowercase();
    let needle = needle.to_lowercase();
    if lower.len() != haystack.len() || needle.is_empty() {
        // Lowercasing changed a non-ASCII length; fall back to exact removal
        // rather than risk slicing at a wrong offset.
        return haystack.replace(&needle, "");
    }
    let mut out = String::with_capacity(haystack.len());
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&needle) {
        let start = from + pos;
        out.push_str(&haystack[from..start]);
        from = start + needle.len();
    }
    out.push_str(&haystack[from..]);
    out
}
