import XCTest
@testable import VoiceCore

/// `voicectl` is how scripts and coding agents edit snippets, so its output and
/// exit codes are a contract. `VoiceCLI.run` is pure over (args, stdin, dir);
/// every test here uses a throwaway directory, never the real Store.dir.
final class CLITests: XCTestCase {

    private var dir: URL!

    override func setUp() {
        super.setUp()
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("VoiceCLITests-\(UUID().uuidString)")
    }

    override func tearDown() {
        if let dir = dir { try? FileManager.default.removeItem(at: dir) }
        dir = nil
        super.tearDown()
    }

    @discardableResult
    private func run(_ args: String..., stdin: String = "") -> CLIResult {
        VoiceCLI.run(args, directory: dir, stdin: { stdin })
    }

    // MARK: - Top level

    func testHelpAndVersionExitZero() {
        for flags in [["--help"], ["-h"], ["help"]] {
            let r = VoiceCLI.run(flags, directory: dir)
            XCTAssertEqual(r.status, 0, "\(flags)")
            XCTAssertTrue(r.stdout.contains("voicectl snippets add"), "\(flags)")
            XCTAssertEqual(r.stderr, "")
        }
        XCTAssertEqual(run("--version").stdout, "voicectl \(VoiceCLI.version)\n")
    }

    /// No command is a usage error (2) so a script can tell it from "not found".
    func testNoCommandIsUsageError() {
        // Explicit call: a bare `run()` would resolve to XCTestCase.run().
        let r = VoiceCLI.run([], directory: dir)
        XCTAssertEqual(r.status, 2)
        XCTAssertTrue(r.stderr.contains("Usage:"))
    }

    func testUnknownCommandAndOptionAreUsageErrors() {
        XCTAssertEqual(run("bogus").status, 2)
        XCTAssertEqual(run("--bogus").status, 2)
        XCTAssertEqual(run("snippets").status, 2)
        XCTAssertEqual(run("snippets", "bogus").status, 2)
        XCTAssertEqual(run("snippets", "list", "--bogus").status, 2)
        XCTAssertEqual(run("snippets", "list", "extra").status, 2)
    }

    // MARK: - add / list / get / remove

    func testAddThenListAndGet() {
        let add = run("snippets", "add", "brb", "be right back")
        XCTAssertEqual(add.status, 0)
        XCTAssertEqual(add.stdout, "Added snippet: brb\n")

        XCTAssertEqual(run("snippets", "list").stdout, "brb\tbe right back\n")
        XCTAssertEqual(run("snippets", "get", "brb").stdout, "be right back\n")
        XCTAssertEqual(run("snippets", "get", "BRB").stdout, "be right back\n",
                       "lookup normalizes the trigger the way add does")
    }

    func testAddExistingTriggerReportsUpdate() {
        run("snippets", "add", "brb", "be right back")
        let r = run("snippets", "add", "BRB", "back in a bit")
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(r.stdout, "Updated snippet: brb\n")
        XCTAssertEqual(SnippetStore(directory: dir).snippets,
                       [Snippet(trigger: "brb", text: "back in a bit")])
    }

    /// `-` reads the text from stdin so multi-line snippets (sign-offs,
    /// addresses) can be piped in. Only the trailing newline is dropped.
    func testAddReadsTextFromStdinAndKeepsInnerNewlines() {
        let r = run("snippets", "add", "signoff", "-", stdin: "Best,\nJabree\n")
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(SnippetStore(directory: dir).snippet(for: "signoff")?.text, "Best,\nJabree")
        // Multi-line text is escaped in the human listing so one snippet is one row.
        XCTAssertEqual(run("snippets", "list").stdout, "signoff\tBest,\\nJabree\n")
    }

    func testAddRejectsEmptyTriggerOrText() {
        XCTAssertEqual(run("snippets", "add", "  ", "text").status, 1)
        XCTAssertEqual(run("snippets", "add", "trigger", "-", stdin: "\n").status, 1)
        XCTAssertEqual(run("snippets", "add", "onlyone").status, 2)
        XCTAssertTrue(SnippetStore(directory: dir).snippets.isEmpty)
    }

    func testListOnEmptyStore() {
        XCTAssertEqual(run("snippets", "list").stdout, "No snippets.\n")
        let json = run("snippets", "list", "--json").stdout
        XCTAssertEqual(try? JSONDecoder().decode([Snippet].self, from: Data(json.utf8)), [])
    }

    func testGetMissingTriggerExitsOne() {
        let r = run("snippets", "get", "nope")
        XCTAssertEqual(r.status, 1)
        XCTAssertEqual(r.stdout, "")
        XCTAssertTrue(r.stderr.contains("nope"))
    }

    func testRemoveByTriggerAndMissingTrigger() {
        run("snippets", "add", "a", "AAA")
        run("snippets", "add", "b", "BBB")
        let r = run("snippets", "remove", "A")
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(r.stdout, "Removed snippet: a\n")
        XCTAssertEqual(SnippetStore(directory: dir).snippets.map(\.trigger), ["b"])
        XCTAssertEqual(run("snippets", "remove", "a").status, 1)
    }

    // MARK: - JSON output (what agents parse)

    func testJsonListMatchesFileFormat() throws {
        run("snippets", "add", "brb", "be right back")
        let out = run("snippets", "list", "--json").stdout
        let decoded = try JSONDecoder().decode([Snippet].self, from: Data(out.utf8))
        XCTAssertEqual(decoded, [Snippet(trigger: "brb", text: "be right back")])
        XCTAssertEqual(run("snippets", "export").stdout, out,
                       "export and list --json are the same document")
    }

    func testJsonGet() throws {
        run("snippets", "add", "brb", "be right back")
        let out = run("snippets", "get", "brb", "--json").stdout
        let decoded = try JSONDecoder().decode(Snippet.self, from: Data(out.utf8))
        XCTAssertEqual(decoded, Snippet(trigger: "brb", text: "be right back"))
    }

    // MARK: - import

    func testImportMergesByDefault() {
        run("snippets", "add", "keep", "KEEP")
        run("snippets", "add", "brb", "old")
        let json = #"[{"trigger":"BRB","text":"new"},{"trigger":"sig","text":"Jabree"}]"#
        let r = run("snippets", "import", "-", stdin: json)
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(r.stdout, "Imported 2 snippets\n")
        let store = SnippetStore(directory: dir)
        XCTAssertEqual(store.snippet(for: "keep")?.text, "KEEP")
        XCTAssertEqual(store.snippet(for: "brb")?.text, "new")
        XCTAssertEqual(store.snippet(for: "sig")?.text, "Jabree")
    }

    func testImportReplaceDropsExisting() {
        run("snippets", "add", "keep", "KEEP")
        let r = run("snippets", "import", "-", "--replace",
                    stdin: #"[{"trigger":"sig","text":"Jabree"}]"#)
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(r.stdout, "Imported 1 snippet (replaced existing)\n")
        XCTAssertEqual(SnippetStore(directory: dir).snippets, [Snippet(trigger: "sig", text: "Jabree")])
    }

    func testImportFromFileSkipsInvalidEntries() throws {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("in.json")
        try Data(#"[{"trigger":"","text":"x"},{"trigger":"ok","text":""},{"trigger":"ok","text":"fine"}]"#.utf8)
            .write(to: file)
        let r = run("snippets", "import", file.path)
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(r.stdout, "Imported 1 snippet, skipped 2 with an empty trigger or text\n")
        XCTAssertEqual(SnippetStore(directory: dir).snippets, [Snippet(trigger: "ok", text: "fine")])
    }

    func testImportRejectsBadJsonAndMissingFile() {
        XCTAssertEqual(run("snippets", "import", "-", stdin: "not json").status, 1)
        XCTAssertEqual(run("snippets", "import", dir.appendingPathComponent("missing.json").path).status, 1)
        XCTAssertTrue(SnippetStore(directory: dir).snippets.isEmpty)
    }

    /// Round trip: what export prints, import accepts unchanged.
    func testExportImportRoundTrip() {
        run("snippets", "add", "c++", "C plus plus")
        run("snippets", "add", "signoff", "-", stdin: "Best,\nJabree\n")
        let exported = run("snippets", "export").stdout
        let other = dir.appendingPathComponent("other")
        let r = VoiceCLI.run(["--dir", other.path, "snippets", "import", "-"],
                             directory: dir, stdin: { exported })
        XCTAssertEqual(r.status, 0)
        XCTAssertEqual(SnippetStore(directory: other).snippets, SnippetStore(directory: dir).snippets)
    }

    // MARK: - expand / path / --dir

    func testExpandPreviewsDictationRewrite() {
        run("snippets", "add", "brb", "be right back")
        XCTAssertEqual(run("snippets", "expand", "okay brb soon").stdout, "okay be right back soon\n")
    }

    func testPathPointsIntoTheDirectory() {
        XCTAssertEqual(run("snippets", "path").stdout, dir.appendingPathComponent("snippets.json").path + "\n")
    }

    func testDirOptionOverridesDirectory() {
        let other = dir.appendingPathComponent("other")
        let r = VoiceCLI.run(["--dir", other.path, "snippets", "add", "x", "X"], directory: dir)
        XCTAssertEqual(r.status, 0)
        XCTAssertTrue(SnippetStore(directory: dir).snippets.isEmpty)
        XCTAssertEqual(SnippetStore(directory: other).snippets.count, 1)
        XCTAssertEqual(VoiceCLI.run(["--dir"], directory: dir).status, 2)
    }

    /// The whole point of the CLI: an already-loaded store (the running app)
    /// sees what voicectl wrote.
    func testRunningStoreSeesCLIEdits() {
        let appStore = SnippetStore(directory: dir)
        XCTAssertEqual(appStore.expand("brb"), "brb")
        run("snippets", "add", "brb", "be right back")
        XCTAssertEqual(appStore.expand("brb"), "be right back")
        run("snippets", "remove", "brb")
        XCTAssertEqual(appStore.expand("brb"), "brb")
    }
}
