import XCTest
@testable import VoiceCore

/// `cleanTranscript` is the last thing that touches whisper output before it is
/// pasted into the user's document, so its job is to strip the model's
/// non-speech annotations without disturbing real dictation.
final class TranscriptTests: XCTestCase {

    // MARK: - Bracketed artifacts

    func testRemovesBlankAudioMarker() {
        XCTAssertEqual(cleanTranscript("[BLANK_AUDIO]"), "")
    }

    func testRemovesEveryKnownArtifact() {
        let artifacts = [
            "[BLANK_AUDIO]", "[INAUDIBLE]", "[MUSIC]", "[SILENCE]", "[NOISE]", "[TYPING]",
            "(blank audio)", "(silence)", "(music)", "(noise)", "(typing)",
            "[MUSIC PLAYING]", "(music playing)", "[SOUND]", "♪",
        ]
        for a in artifacts {
            XCTAssertEqual(cleanTranscript(a), "", "artifact \(a) survived cleaning")
            XCTAssertEqual(cleanTranscript("hello \(a) world"), "hello world",
                           "artifact \(a) was not stripped from mid-sentence")
        }
    }

    func testArtifactMatchingIsCaseInsensitive() {
        XCTAssertEqual(cleanTranscript("[blank_audio]"), "")
        XCTAssertEqual(cleanTranscript("[Music Playing]"), "")
        XCTAssertEqual(cleanTranscript("(SILENCE)"), "")
    }

    func testRemovesRepeatedAndMixedArtifacts() {
        XCTAssertEqual(cleanTranscript("[BLANK_AUDIO][BLANK_AUDIO][MUSIC]"), "")
        XCTAssertEqual(cleanTranscript("[MUSIC] send the file [TYPING] tomorrow ♪"),
                       "send the file tomorrow")
    }

    /// Only the listed markers are stripped. Anything else in brackets is left
    /// alone rather than guessed at, so unknown annotations reach the user.
    func testUnknownBracketedTextIsPreserved() {
        XCTAssertEqual(cleanTranscript("[LAUGHTER] that was good"), "[LAUGHTER] that was good")
        XCTAssertEqual(cleanTranscript("see item [3] below"), "see item [3] below")
    }

    // MARK: - Whitespace

    func testCollapsesRunsOfSpaces() {
        XCTAssertEqual(cleanTranscript("hello     world"), "hello world")
        XCTAssertEqual(cleanTranscript("a  b   c    d"), "a b c d")
    }

    func testNewlinesBecomeSpaces() {
        XCTAssertEqual(cleanTranscript("first line\nsecond line"), "first line second line")
        XCTAssertEqual(cleanTranscript("a\n\n\nb"), "a b")
    }

    func testTrimsLeadingAndTrailingWhitespace() {
        XCTAssertEqual(cleanTranscript("   hello world   "), "hello world")
        XCTAssertEqual(cleanTranscript("\n\thello\n"), "hello")
    }

    /// Whisper emits a leading space on nearly every segment; removing an
    /// artifact then leaves a double space where the marker used to be.
    func testGapLeftByRemovedArtifactIsClosed() {
        XCTAssertEqual(cleanTranscript(" [BLANK_AUDIO] send it now"), "send it now")
        XCTAssertEqual(cleanTranscript("send it [SILENCE] now"), "send it now")
    }

    // MARK: - Empty and whitespace-only input

    func testEmptyInputStaysEmpty() {
        XCTAssertEqual(cleanTranscript(""), "")
    }

    func testWhitespaceOnlyInputBecomesEmpty() {
        XCTAssertEqual(cleanTranscript("   "), "")
        XCTAssertEqual(cleanTranscript("\n \t \n"), "")
    }

    // MARK: - Real dictation is untouched

    func testOrdinarySentenceIsUnchanged() {
        let sentence = "Let's ship the release on Friday."
        XCTAssertEqual(cleanTranscript(sentence), sentence)
    }

    func testPunctuationAndCasingArePreserved() {
        let sentence = "Hey Dr. Chen — can you review PR #42? Thanks!"
        XCTAssertEqual(cleanTranscript(sentence), sentence)
    }

    func testSingleSpacesBetweenWordsAreNotAltered() {
        let sentence = "one two three four five"
        XCTAssertEqual(cleanTranscript(sentence), sentence)
    }

    func testUnicodeAndEmojiSurvive() {
        XCTAssertEqual(cleanTranscript("café naïve 🎉"), "café naïve 🎉")
    }

    // MARK: - Idempotence

    func testCleaningTwiceMatchesCleaningOnce() {
        let raw = "  [MUSIC]  hold on\n\n I'll check [TYPING] now ♪  "
        let once = cleanTranscript(raw)
        XCTAssertEqual(cleanTranscript(once), once)
        XCTAssertEqual(once, "hold on I'll check now")
    }
}
