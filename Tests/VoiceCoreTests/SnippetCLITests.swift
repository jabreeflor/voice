import XCTest
@testable import VoiceCore

/// The bundled `voice` CLI is how agents deliver snippets. These tests drive
/// `SnippetCLI` against a throwaway directory — never the real
/// ~/Library/Application Support/Voice.
final class SnippetCLITests: XCTestCase {

    private var dir: URL!
    private var store: SnippetStore!

    override func setUp() {
        super.setUp()
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("VoiceCLITests-\(UUID().uuidString)")
        store = SnippetStore(directory: dir)
    }

    override func tearDown() {
        store = nil
        if let dir = dir { try? FileManager.default.removeItem(at: dir) }
        dir = nil
        super.tearDown()
    }

    private func run(_ args: [String],
                     stdin: String? = nil,
                     tty: Bool = true) -> (code: Int32, out: String, err: String) {
        var out = ""
        var err = ""
        let code = SnippetCLI.execute(
            arguments: ["voice"] + args,
            store: store,
            stdinTTY: tty,
            readStdin: { stdin },
            stdout: { out += $0 },
            stderr: { err += $0 })
        return (code, out, err)
    }

    // MARK: - parse

    func testBareInvocationIsAMissingCommand() {
        guard case .failure(let message) = SnippetCLI.parse([]) else {
            return XCTFail("expected failure")
        }
        XCTAssertTrue(message.contains("missing command"), message)
    }

    func testUnknownCommandFails() {
        guard case .failure(let message) = SnippetCLI.parse(["frobnicate"]) else {
            return XCTFail("expected failure")
        }
        XCTAssertTrue(message.contains("unknown command"), message)
    }

    func testAddWithoutTriggerFails() {
        guard case .failure(let message) = SnippetCLI.parse(["add"]) else {
            return XCTFail("expected failure")
        }
        XCTAssertTrue(message.contains("trigger"), message)
    }

    func testAddWithOnlyTriggerMeansReadStdin() {
        XCTAssertEqual(SnippetCLI.parse(["add", "brb"]),
                       .success(.add(trigger: "brb", text: nil)))
    }

    func testAddJoinsRemainingWordsAsSnippetText() {
        XCTAssertEqual(SnippetCLI.parse(["add", "brb", "be", "right", "back"]),
                       .success(.add(trigger: "brb", text: "be right back")))
    }

    func testSnippetPrefixIsOptionalSugar() {
        XCTAssertEqual(SnippetCLI.parse(["snippet", "add", "brb", "x"]),
                       .success(.add(trigger: "brb", text: "x")))
        XCTAssertEqual(SnippetCLI.parse(["snippets", "list"]), .success(.list))
    }

    func testHelpFlags() {
        for flag in ["help", "-h", "--help"] {
            XCTAssertEqual(SnippetCLI.parse([flag]), .success(.help), flag)
        }
    }

    func testRemoveJoinsAMultiWordTrigger() {
        XCTAssertEqual(SnippetCLI.parse(["remove", "my", "email"]),
                       .success(.remove(trigger: "my email")))
        XCTAssertEqual(SnippetCLI.parse(["rm", "brb"]),
                       .success(.remove(trigger: "brb")))
    }

    // MARK: - add

    func testAddWritesTheSnippetAndPrintsTheCanonicalTrigger() {
        let r = run(["add", "BRB", "be right back"])
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "saved: brb\n")
        XCTAssertEqual(store.expand("okay brb"), "okay be right back")
    }

    func testAddFromStdinWhenTextIsOmitted() {
        let r = run(["add", "sig"], stdin: "Best,\nJabree\n", tty: false)
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "saved: sig\n")
        XCTAssertEqual(store.expand("sig"), "Best,\nJabree\n")
    }

    func testAddWithoutTextOnATtyFailsRatherThanHanging() {
        let r = run(["add", "brb"], tty: true)
        XCTAssertEqual(r.code, 1)
        XCTAssertTrue(r.err.contains("missing snippet text"), r.err)
        XCTAssertTrue(store.snippets.isEmpty)
    }

    func testAddEmptyTextIsRejected() {
        let r = run(["add", "brb", ""], tty: true)
        XCTAssertEqual(r.code, 1)
        XCTAssertTrue(r.err.contains("non-empty"), r.err)
        XCTAssertTrue(store.snippets.isEmpty)
    }

    func testAddReplacesTheSameTrigger() {
        _ = run(["add", "brb", "old"])
        let r = run(["add", "brb", "new"])
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(store.snippets.count, 1)
        XCTAssertEqual(store.expand("brb"), "new")
    }

    func testAddSurvivesAReloadSoTheAppSeesIt() {
        _ = run(["add", "my email", "you@example.com"])
        let app = SnippetStore(directory: dir)
        XCTAssertEqual(app.expand("my email."), "you@example.com")
    }

    // MARK: - list / remove

    func testListEmptyStoreIsAnEmptyJSONArray() {
        let r = run(["list"])
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out.trimmingCharacters(in: .whitespacesAndNewlines), "[]")
    }

    func testListPrintsJSONTheAppCanRoundTrip() {
        _ = run(["add", "brb", "be right back"])
        _ = run(["add", "sig", "Best,\nJabree"])
        let r = run(["list"])
        XCTAssertEqual(r.code, 0)
        let data = r.out.data(using: .utf8)!
        let decoded = try! JSONDecoder().decode([Snippet].self, from: data)
        XCTAssertEqual(decoded, [
            Snippet(trigger: "brb", text: "be right back"),
            Snippet(trigger: "sig", text: "Best,\nJabree"),
        ])
    }

    func testRemoveDeletesByTrigger() {
        _ = run(["add", "brb", "be right back"])
        _ = run(["add", "sig", "Best,"])
        let r = run(["remove", "BRB"])
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "removed: brb\n")
        XCTAssertEqual(store.snippets.map(\.trigger), ["sig"])
    }

    func testRemoveMissingTriggerExitsOne() {
        let r = run(["remove", "nope"])
        XCTAssertEqual(r.code, 1)
        XCTAssertTrue(r.err.contains("no snippet named 'nope'"), r.err)
    }

    func testHelpPrintsUsageAndExitsZero() {
        let r = run(["help"])
        XCTAssertEqual(r.code, 0)
        XCTAssertTrue(r.out.contains("voice add <trigger>"), r.out)
        XCTAssertEqual(r.err, "")
    }

    func testUnknownCommandPrintsUsageToStderr() {
        let r = run(["frobnicate"])
        XCTAssertEqual(r.code, 1)
        XCTAssertTrue(r.err.contains("unknown command"), r.err)
        XCTAssertTrue(r.err.contains("Usage:"), r.err)
    }
}
