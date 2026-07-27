import XCTest
@testable import VoiceCore

/// `SnippetStore.expand` rewrites the transcript before it is pasted, so a
/// mistake here silently corrupts the user's text. These tests always use a
/// throwaway directory — never the real ~/Library/Application Support/Voice.
final class SnippetTests: XCTestCase {

    private var dir: URL!
    private var store: SnippetStore!

    override func setUp() {
        super.setUp()
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("VoiceSnippetTests-\(UUID().uuidString)")
        store = SnippetStore(directory: dir)
    }

    override func tearDown() {
        store = nil
        if let dir = dir { try? FileManager.default.removeItem(at: dir) }
        dir = nil
        super.tearDown()
    }

    // MARK: - Passthrough

    func testEmptyStoreReturnsTranscriptUnchanged() {
        XCTAssertEqual(store.expand("nothing to expand here"), "nothing to expand here")
        XCTAssertEqual(store.expand(""), "")
    }

    func testUnrelatedTranscriptIsUnchanged() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("let's meet at noon"), "let's meet at noon")
    }

    func testNonMatchingTranscriptIsReturnedByteForByte() {
        store.add(trigger: "sig", text: "Jabree")
        let text = "Punctuation, casing and  spacing are left alone."
        XCTAssertEqual(store.expand(text), text)
    }

    // MARK: - Whole-utterance triggers

    func testWholeUtteranceTriggerReturnsSnippetVerbatim() {
        store.add(trigger: "my email", text: "jabreenicholas@gmail.com")
        XCTAssertEqual(store.expand("my email"), "jabreenicholas@gmail.com")
    }

    func testWholeUtteranceMatchIgnoresCase() {
        store.add(trigger: "my email", text: "jabreenicholas@gmail.com")
        XCTAssertEqual(store.expand("My Email"), "jabreenicholas@gmail.com")
        XCTAssertEqual(store.expand("MY EMAIL"), "jabreenicholas@gmail.com")
    }

    /// Whisper punctuates a lone phrase as a sentence, so "my email." has to
    /// count as the bare trigger.
    func testWholeUtteranceMatchStripsTrailingPunctuation() {
        store.add(trigger: "my email", text: "jabreenicholas@gmail.com")
        for spoken in ["my email.", "my email!", "my email?", "my email,", "my email..."] {
            XCTAssertEqual(store.expand(spoken), "jabreenicholas@gmail.com",
                           "failed for \(spoken)")
        }
    }

    func testWholeUtteranceMatchIgnoresSurroundingWhitespace() {
        store.add(trigger: "my email", text: "jabreenicholas@gmail.com")
        XCTAssertEqual(store.expand("   my email  "), "jabreenicholas@gmail.com")
        XCTAssertEqual(store.expand("\nmy email\n"), "jabreenicholas@gmail.com")
    }

    /// The whole-utterance path returns the stored text directly, so multi-line
    /// snippets and regex-looking characters come through untouched.
    func testWholeUtteranceSnippetIsNotEscapedOrReflowed() {
        let block = "Best,\nJabree\n\nP.S. cost is $1 (50% off) [see notes]"
        store.add(trigger: "signoff", text: block)
        XCTAssertEqual(store.expand("signoff."), block)
    }

    // MARK: - Mid-sentence replacement

    func testTriggerInsideSentenceIsReplacedInPlace() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("okay brb see you soon"), "okay be right back see you soon")
    }

    func testMidSentenceReplacementIgnoresCase() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("okay BRB soon"), "okay be right back soon")
        XCTAssertEqual(store.expand("okay Brb soon"), "okay be right back soon")
    }

    func testAllOccurrencesAreReplaced() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("brb and then brb again"),
                       "be right back and then be right back again")
    }

    func testReplacementRespectsWordBoundaries() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("abrb"), "abrb")
        XCTAssertEqual(store.expand("brbx"), "brbx")
        XCTAssertEqual(store.expand("hyperbrbole"), "hyperbrbole")
    }

    func testTriggerAdjacentToPunctuationStillExpands() {
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.expand("wait, brb, then we talk"),
                       "wait, be right back, then we talk")
    }

    /// The replacement runs through the regex engine, so a snippet containing
    /// "$1" must not be read as a capture-group reference.
    func testDollarSignsInSnippetTextAreNotTreatedAsTemplateReferences() {
        store.add(trigger: "price", text: "cost $1 per seat")
        XCTAssertEqual(store.expand("the price is fixed"), "the cost $1 per seat is fixed")
    }

    // MARK: - Longest trigger wins

    func testLongerTriggerIsAppliedBeforeItsShorterPrefix() {
        store.add(trigger: "my", text: "MY")
        store.add(trigger: "my email", text: "EMAIL")
        XCTAssertEqual(store.expand("send my email now"), "send EMAIL now")
    }

    func testShorterTriggerStillAppliesWhereTheLongOneDoesNot() {
        store.add(trigger: "my", text: "MY")
        store.add(trigger: "my email", text: "EMAIL")
        XCTAssertEqual(store.expand("this is my house"), "this is MY house")
    }

    // MARK: - Regex-special characters in triggers

    func testTriggerWithPlusSignsDoesNotCrashAndMatchesWholeUtterance() {
        store.add(trigger: "c++", text: "C plus plus")
        XCTAssertEqual(store.expand("c++"), "C plus plus")
        XCTAssertEqual(store.expand("C++."), "C plus plus")
    }

    /// KNOWN LIMITATION (see report): the trigger is wrapped in \b...\b, and a
    /// trigger ending in a non-word character such as "+" can never satisfy the
    /// trailing boundary. "c++" therefore does not expand mid-sentence. This
    /// asserts current behavior so a future fix trips the test deliberately.
    func testTriggerEndingInNonWordCharacterExpandsMidSentence() {
        store.add(trigger: "c++", text: "C plus plus")
        XCTAssertEqual(store.expand("I write c++ every day"), "I write C plus plus every day")
    }

    func testSymbolEdgedTriggerStillRespectsWordBoundaries() {
        store.add(trigger: "c++", text: "C plus plus")
        XCTAssertEqual(store.expand("see c++x compile"), "see c++x compile")
    }

    func testTriggerWithAmpersandExpandsMidSentence() {
        store.add(trigger: "q&a", text: "questions and answers")
        XCTAssertEqual(store.expand("the q&a session starts now"),
                       "the questions and answers session starts now")
        XCTAssertEqual(store.expand("q&a"), "questions and answers")
    }

    /// A "." in a trigger must be a literal dot, not the regex any-character
    /// wildcard.
    func testDotInTriggerIsLiteralNotAWildcard() {
        store.add(trigger: "a.b", text: "MATCHED")
        XCTAssertEqual(store.expand("axb"), "axb")
        XCTAssertEqual(store.expand("a.b"), "MATCHED")
    }

    func testAssortedRegexMetacharactersInTriggersAreSafe() {
        let triggers = ["c++", "q&a", "a.b", "(paren)", "[bracket]", "a|b", "x*y", "^caret",
                        "dollar$", "back\\slash", "a?b", "{brace}"]
        for (i, t) in triggers.enumerated() {
            store.add(trigger: t, text: "EXPANSION\(i)")
        }
        // Whole-utterance matching works for all of them, and nothing throws.
        for (i, t) in triggers.enumerated() {
            XCTAssertEqual(store.expand(t), "EXPANSION\(i)", "whole-utterance failed for \(t)")
        }
        // An unrelated sentence must survive every one of those patterns.
        XCTAssertEqual(store.expand("a perfectly ordinary sentence"),
                       "a perfectly ordinary sentence")
    }

    func testMetacharacterTriggersDoNotMatchArbitraryText() {
        store.add(trigger: "x*y", text: "STAR")
        XCTAssertEqual(store.expand("xy"), "xy")
        XCTAssertEqual(store.expand("xxxy"), "xxxy")
    }

    // MARK: - add / remove

    func testAddLowercasesAndTrimsTheTrigger() {
        store.add(trigger: "  \"My Email\" ", text: "x@y.com")
        XCTAssertEqual(store.snippets.first?.trigger, "my email")
    }

    func testAddRejectsEmptyTriggerOrText() {
        store.add(trigger: "", text: "something")
        store.add(trigger: "   ", text: "something")
        store.add(trigger: "trigger", text: "")
        XCTAssertTrue(store.snippets.isEmpty)
    }

    func testAddingSameTriggerReplacesTheOldSnippet() {
        store.add(trigger: "brb", text: "be right back")
        store.add(trigger: "brb", text: "back in a bit")
        XCTAssertEqual(store.snippets.count, 1)
        XCTAssertEqual(store.expand("brb"), "back in a bit")
    }

    func testRemoveDeletesByIndexAndIgnoresOutOfRange() {
        store.add(trigger: "a", text: "AAA")
        store.add(trigger: "b", text: "BBB")
        store.remove(at: 0)
        XCTAssertEqual(store.snippets.map(\.trigger), ["b"])
        store.remove(at: 99)
        store.remove(at: -1)
        XCTAssertEqual(store.snippets.count, 1)
    }

    func testStampAdvancesOnMutationOnly() {
        let start = store.stamp
        store.add(trigger: "brb", text: "be right back")
        XCTAssertEqual(store.stamp, start + 1)
        store.add(trigger: "", text: "")          // rejected
        XCTAssertEqual(store.stamp, start + 1)
        store.remove(at: 0)
        XCTAssertEqual(store.stamp, start + 2)
    }

    // MARK: - Persistence

    func testSnippetsSurviveAReload() {
        store.add(trigger: "brb", text: "be right back")
        store.add(trigger: "my email", text: "x@y.com")

        let reloaded = SnippetStore(directory: dir)
        XCTAssertEqual(reloaded.snippets, store.snippets)
        XCTAssertEqual(reloaded.expand("okay brb"), "okay be right back")
        XCTAssertEqual(reloaded.expand("my email."), "x@y.com")
    }

    func testRemovalIsPersisted() {
        store.add(trigger: "brb", text: "be right back")
        store.remove(at: 0)
        XCTAssertTrue(SnippetStore(directory: dir).snippets.isEmpty)
    }

    func testFreshDirectoryStartsEmpty() {
        let other = FileManager.default.temporaryDirectory
            .appendingPathComponent("VoiceSnippetTests-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: other) }
        XCTAssertTrue(SnippetStore(directory: other).snippets.isEmpty)
    }

    func testCorruptSnippetFileLeavesStoreEmptyRatherThanCrashing() {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? Data("not json".utf8).write(to: dir.appendingPathComponent("snippets.json"))
        let broken = SnippetStore(directory: dir)
        XCTAssertTrue(broken.snippets.isEmpty)
        XCTAssertEqual(broken.expand("hello"), "hello")
    }
}
