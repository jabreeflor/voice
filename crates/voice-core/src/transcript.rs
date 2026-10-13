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

// MARK: - Filler words

/// Hesitation sounds `remove_fillers` drops: um, umm, uh, uhh, uhm, erm and
/// their stretched spellings. Words that are filler only *sometimes* are
/// deliberately absent: "like", "you know", "I mean", "so", "well", "ah",
/// "oh", "hmm" (often intentional in writing), "er" ("the ER"), and "eh".
/// Removing those needs meaning, not pattern matching.
const FILLER_PATTERN: &str = r"(?i)\b(?:u+m+|u+h+m*|e+r+m+)\b";

fn filler_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(FILLER_PATTERN).expect("filler pattern is valid"))
}

/// Characters that make a neighbouring "um"/"uh" part of something else:
/// "uh-oh", "um's", a quoted "um" (a mention, not a hesitation).
fn attaches_filler(c: char) -> bool {
    matches!(c, '-' | '\'' | '’' | '"' | '“' | '”')
}

/// Removes hesitation fillers from a cleaned transcript and repairs the
/// punctuation and capitals around them, deterministically:
///
/// - "Um, so I think" → "So I think" (a sentence that lost its first word
///   gets a capital, unless the next word is mixed-case like "iPhone").
/// - "I, uh, think" → "I think" (a filler wrapped in commas takes both).
/// - "We should go, um." → "We should go." and "Um." → "".
/// - "Um... okay" → "Okay" (an ellipsis right after a filler goes with it).
///
/// Never touched: words that merely contain the letters ("umbrella",
/// "album", "drum"), hyphenated or quoted forms ("uh-oh", "uh-huh"),
/// "uh huh" / "uh oh" written as two words, and all-caps tokens ("UM", the
/// university; "ERM"). English-only by design: in Portuguese "um" is a real
/// word, so the setting exists to turn this off.
pub fn remove_fillers(text: &str) -> String {
    let re = filler_regex();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0; // text[..cursor] is already accounted for in `out`
    let mut capitalize = false;
    let mut pos = 0;
    while let Some(m) = re.find_at(text, pos) {
        pos = m.end();
        if !is_removable_filler(text, m.start(), m.end()) {
            continue;
        }
        push_chunk(&mut out, &text[cursor..m.start()], &mut capitalize);

        // Punctuation that belongs to the filler itself.
        let mut end = m.end();
        let mut wrapped = false;
        if text[end..].starts_with("...") {
            end += 3;
        } else if text[end..].starts_with('…') {
            end += '…'.len_utf8();
        } else if text[end..].starts_with(',') {
            end += 1;
            wrapped = true;
        }
        end += leading_whitespace_len(&text[end..]);

        let kept = out.trim_end().len();
        out.truncate(kept);
        let next = text[end..].chars().next();
        let clause_ends = matches!(next, None | Some('.' | '!' | '?' | ',' | ';' | ':'));
        if (wrapped || clause_ends) && out.ends_with(',') {
            out.pop();
        }
        if out.is_empty() || out.ends_with(['.', '!', '?']) {
            // The filler opened a sentence: punctuation that only closed
            // the filler ("Um." / "Um, ...") goes too, and the next word
            // inherits the capital.
            if let Some(c @ ('.' | '!' | '?' | ',')) = next {
                end += c.len_utf8();
                end += leading_whitespace_len(&text[end..]);
            }
            capitalize = true;
        }
        let next = text[end..].chars().next();
        let joins_word = next.is_some_and(|c| !matches!(c, '.' | '!' | '?' | ',' | ';' | ':'));
        if !out.is_empty() && joins_word {
            out.push(' ');
        }
        cursor = end;
        pos = end;
    }
    push_chunk(&mut out, &text[cursor..], &mut capitalize);
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().to_string()
}

/// A regex hit is a filler only when it stands alone as a lower- or
/// title-case word and is not the first half of "uh huh" / "uh oh".
fn is_removable_filler(text: &str, start: usize, end: usize) -> bool {
    let word = &text[start..end];
    if word.len() >= 2 && word.chars().all(|c| c.is_ascii_uppercase()) {
        return false;
    }
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(attaches_filler)
        || text[end..].chars().next().is_some_and(attaches_filler)
    {
        return false;
    }
    let following: String = text[end..]
        .trim_start_matches([' ', ','])
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    let lower = word.to_ascii_lowercase();
    !(lower.starts_with("uh") && (following == "huh" || following == "oh"))
}

fn leading_whitespace_len(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// Appends `chunk`, upper-casing its first word when a removed filler left
/// it at the start of a sentence. A word with capitals already inside it
/// ("iPhone", "eBay") is left as is. An empty chunk (two fillers in a row)
/// keeps the request pending for the next one.
fn push_chunk(out: &mut String, chunk: &str, capitalize: &mut bool) {
    if !*capitalize {
        out.push_str(chunk);
        return;
    }
    let lead = leading_whitespace_len(chunk);
    let body = &chunk[lead..];
    let Some(first) = body.chars().next() else {
        out.push_str(chunk);
        return;
    };
    *capitalize = false;
    let mixed_case = body[first.len_utf8()..]
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .any(char::is_uppercase);
    out.push_str(&chunk[..lead]);
    if first.is_lowercase() && !mixed_case {
        out.extend(first.to_uppercase());
        out.push_str(&body[first.len_utf8()..]);
    } else {
        out.push_str(body);
    }
}
