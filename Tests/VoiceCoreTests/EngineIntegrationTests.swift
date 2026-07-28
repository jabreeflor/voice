import Foundation
import XCTest
@testable import VoiceCore

/// Tier-2 integration tests: a real `whisper-server` process, real audio, real
/// transcripts. Nothing here is mocked.
///
/// These tests skip (rather than fail) when `whisper-server` or a ggml model is
/// missing, so machines without whisper.cpp installed stay green.
///
/// The server is booted once for the whole class and shared by every test —
/// model load dominates the runtime, so per-test boots would be wasteful. Tests
/// are named `testN_` so XCTest's alphabetical ordering runs them in a sensible
/// sequence. SwiftPM's `--parallel` shards by test class into separate
/// processes, so this class never overlaps itself; `reclaimTestPort()` below
/// covers the remaining case of a server orphaned by an earlier crashed run.
final class EngineIntegrationTests: XCTestCase {

    /// Deliberately not `Config.serverPort` (8178): the user's installed
    /// Voice.app owns that port on a dev machine and must not be disturbed.
    private static let testPort: UInt16 = 18178

    /// Cold model load plus Metal shader compilation. Measured at ~0.5s warm,
    /// but seen as high as 43s on a machine under heavy parallel build load —
    /// hence the wide margin. Exceeding it skips rather than fails.
    private static let bootTimeout: TimeInterval = 60

    /// Per-request ceiling. Far above the observed ~0.4s so a stalled request
    /// surfaces as a clear failure instead of hanging the suite.
    private static let requestTimeout: TimeInterval = 60

    private static let foxPhrase = "The quick brown fox jumps over the lazy dog."
    private static let secondPhrase = "Hello world, this is a test of the voice engine."

    private static var engine: WhisperEngine?
    private static var skipReason: String?
    private static var bootSeconds: TimeInterval = 0
    private static var modelPath = ""
    private static var foxWAV = Data()
    private static var foxDuration: TimeInterval = 0
    private static var secondWAV = Data()
    private static var prepared = false

    // MARK: - Lifecycle

    override func setUpWithError() throws {
        try Self.prepareOnce()
        if let reason = Self.skipReason { throw XCTSkip(reason) }
    }

    override class func tearDown() {
        engine?.stop()
        engine = nil
        // The child is terminated above, but a SIGKILL'd test runner can leak
        // one; make sure the port is free for the next run either way.
        reclaimTestPort()
        FixtureAudio.removeDirectory()
        super.tearDown()
    }

    /// Prerequisite detection, fixture generation and the single server boot.
    /// Records a skip reason instead of failing when the environment can't run.
    private static func prepareOnce() throws {
        guard !prepared else { return }
        prepared = true

        guard WhisperEngine.findBinary("whisper-server") != nil else {
            skipReason = "whisper-server not installed (brew install whisper-cpp)"
            return
        }
        guard let model = resolveModel() else {
            skipReason = "no ggml model found in \(Config.modelsDirs.map(\.path).joined(separator: ", "))"
            return
        }
        modelPath = model.path

        do {
            let foxSamples = try FixtureAudio.speechSamples(foxPhrase)
            foxDuration = FixtureAudio.duration(ofSamples: foxSamples)
            foxWAV = Recorder.wavData(samples: foxSamples)
            secondWAV = try FixtureAudio.speechWAV(secondPhrase)
        } catch {
            skipReason = "could not synthesise speech fixtures: \(error)"
            return
        }
        guard foxWAV.count > 44, secondWAV.count > 44 else {
            skipReason = "speech fixtures came out empty"
            return
        }

        reclaimTestPort()

        let engine = WhisperEngine(modelURL: model, port: testPort)
        self.engine = engine
        let start = Date()
        engine.start()

        let becameReady = spinRunLoop(timeout: bootTimeout) { engine.ready }
        bootSeconds = Date().timeIntervalSince(start)
        guard becameReady else {
            skipReason = """
                whisper-server did not become ready on port \(testPort) within \
                \(Int(bootTimeout))s (status: \(engine.statusText), \
                model: \(model.lastPathComponent))
                """
            engine.stop()
            self.engine = nil
            return
        }

        // `start()` kicks off an internal warm-up transcription. Run one more
        // and wait for it so the latency test measures a settled engine rather
        // than racing the warm-up.
        _ = transcribeSync(FixtureAudio.silenceWAV(seconds: 0.5), on: engine, timeout: requestTimeout)
    }

    // MARK: - Tests

    func test1_serverBootsAndBecomesReady() throws {
        let engine = try Self.requireEngine()
        XCTAssertTrue(engine.ready, "engine should be ready after boot")
        XCTAssertEqual(engine.statusText, "ready")
        XCTAssertEqual(engine.port, Self.testPort, "must not touch the app's port 8178")
        XCTAssertLessThan(Self.bootSeconds, Self.bootTimeout,
                          "boot exceeded the \(Int(Self.bootTimeout))s budget")
        log("booted \(URL(fileURLWithPath: Self.modelPath).lastPathComponent) "
            + "on port \(Self.testPort) in \(fmt(Self.bootSeconds))s")
    }

    func test2_transcribesSpeechFixture() throws {
        let engine = try Self.requireEngine()

        let fox = try XCTUnwrap(Self.transcribeSync(Self.foxWAV, on: engine, timeout: Self.requestTimeout))
        let raw = try fox.get()
        log("fox transcript: \(raw.debugDescription)")
        let normalized = Self.normalize(raw)
        XCTAssertTrue(normalized.contains("quick brown fox"),
                      "expected 'quick brown fox' in transcript, got: \(raw.debugDescription)")

        let second = try XCTUnwrap(Self.transcribeSync(Self.secondWAV, on: engine, timeout: Self.requestTimeout))
        let secondRaw = try second.get()
        log("second transcript: \(secondRaw.debugDescription)")
        let secondNormalized = Self.normalize(secondRaw)
        XCTAssertTrue(secondNormalized.contains("hello world"),
                      "expected 'hello world' in transcript, got: \(secondRaw.debugDescription)")
    }

    func test3_cleanTranscriptProducesUsableText() throws {
        let engine = try Self.requireEngine()
        let result = try XCTUnwrap(Self.transcribeSync(Self.foxWAV, on: engine, timeout: Self.requestTimeout))
        let cleaned = cleanTranscript(try result.get())

        XCTAssertFalse(cleaned.isEmpty, "cleanTranscript stripped a real utterance to nothing")
        XCTAssertFalse(cleaned.contains("["), "bracketed artifact survived cleanup: \(cleaned)")
        XCTAssertFalse(cleaned.contains("]"), "bracketed artifact survived cleanup: \(cleaned)")
        XCTAssertFalse(cleaned.contains("\n"), "newline survived cleanup: \(cleaned.debugDescription)")
        XCTAssertFalse(cleaned.contains("  "), "double space survived cleanup: \(cleaned.debugDescription)")
        XCTAssertEqual(cleaned, cleaned.trimmingCharacters(in: .whitespacesAndNewlines),
                       "cleanTranscript left surrounding whitespace")
        XCTAssertTrue(Self.normalize(cleaned).contains("quick brown fox"),
                      "cleanup damaged the words: \(cleaned.debugDescription)")
        log("cleaned transcript: \(cleaned.debugDescription)")
    }

    func test4_warmTranscriptionMeetsLatencyBudget() throws {
        let engine = try Self.requireEngine()
        XCTAssertGreaterThan(Self.foxDuration, 1.5,
                             "latency fixture should be a few seconds of speech")

        // Budget observed ~0.3s warm on Apple Silicon; 5s is the local bar.
        // GitHub's shared macOS VMs run whisper ~50x slower (measured 11-12s
        // for 2.5s of audio). CI uses a tighter hang/regression ceiling than
        // the old 60s guard so large slowdowns still fail.
        let isCI = ProcessInfo.processInfo.environment["CI"] != nil
        let budget: TimeInterval = isCI ? 25.0 : 5.0
        var elapsed: TimeInterval = 0
        for attempt in 1...3 {
            let start = Date()
            let result = try XCTUnwrap(Self.transcribeSync(Self.foxWAV, on: engine, timeout: Self.requestTimeout))
            elapsed = Date().timeIntervalSince(start)
            _ = try result.get()
            log("latency run \(attempt): \(fmt(elapsed))s "
                + "for \(fmt(Self.foxDuration))s of audio "
                + "(\(fmt(Self.foxDuration / max(elapsed, 0.001)))x realtime)")
        }
        XCTAssertLessThan(elapsed, budget,
                          "warm transcription of \(fmt(Self.foxDuration))s of audio took "
                          + "\(fmt(elapsed))s, over the \(fmt(budget))s budget")
    }

    /// Documents the "No speech detected" path: whisper emits an artifact token
    /// for silence, and `cleanTranscript` must reduce it to the empty string so
    /// the app pastes nothing rather than "[BLANK_AUDIO]".
    func test5_silenceCleansToEmptyString() throws {
        let engine = try Self.requireEngine()

        for (label, wav) in [("digital silence", FixtureAudio.silenceWAV(seconds: 2)),
                             ("near-silence", FixtureAudio.silenceWAV(seconds: 2, amplitude: 0.0005))] {
            let result = try XCTUnwrap(Self.transcribeSync(wav, on: engine, timeout: Self.requestTimeout))
            let raw = try result.get()
            let cleaned = cleanTranscript(raw)
            log("\(label): raw=\(raw.debugDescription) cleaned=\(cleaned.debugDescription)")
            XCTAssertTrue(cleaned.isEmpty,
                          "\(label) should clean to \"\", got \(cleaned.debugDescription) "
                          + "from raw \(raw.debugDescription)")
        }
    }

    // MARK: - Helpers

    private static func requireEngine() throws -> WhisperEngine {
        try XCTUnwrap(engine, "engine unavailable: \(skipReason ?? "unknown")")
    }

    /// Runs a transcription and pumps the run loop until the completion fires.
    /// `WhisperEngine` calls back on the main queue, so the caller must not block it.
    private static func transcribeSync(_ wav: Data,
                                       on engine: WhisperEngine,
                                       timeout: TimeInterval) -> Result<String, Error>? {
        var outcome: Result<String, Error>?
        engine.transcribe(wav: wav) { outcome = $0 }
        _ = spinRunLoop(timeout: timeout) { outcome != nil }
        return outcome
    }

    /// Waits for `condition` while keeping the main run loop (and therefore the
    /// main dispatch queue) draining.
    @discardableResult
    private static func spinRunLoop(timeout: TimeInterval, until condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() >= deadline { return condition() }
            if Thread.isMainThread {
                RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.02))
            } else {
                Thread.sleep(forTimeInterval: 0.02)
            }
        }
        return true
    }

    /// Frees the test port if a previous crashed run orphaned a server on it.
    /// Only ever kills a process whose executable is `whisper-server`.
    private static func reclaimTestPort() {
        guard let pids = shell("/usr/sbin/lsof",
                               ["-nP", "-iTCP:\(testPort)", "-sTCP:LISTEN", "-t"]) else { return }
        for line in pids.split(whereSeparator: \.isNewline) {
            guard let pid = pid_t(line.trimmingCharacters(in: .whitespaces)) else { continue }
            guard let comm = shell("/bin/ps", ["-o", "comm=", "-p", String(pid)]),
                  comm.contains("whisper-server") else { continue }
            kill(pid, SIGTERM)
        }
        // Give the socket a moment to be released before a rebind.
        _ = spinRunLoop(timeout: 2) {
            (shell("/usr/sbin/lsof", ["-nP", "-iTCP:\(testPort)", "-sTCP:LISTEN", "-t"]) ?? "")
                .trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
    }

    private static func shell(_ path: String, _ args: [String]) -> String? {
        guard FileManager.default.isExecutableFile(atPath: path) else { return nil }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: path)
        p.arguments = args
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return nil }
        let out = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return String(data: out, encoding: .utf8)
    }

    private static func resolveModel() -> URL? {
        // Prefer base.en: it is the app's default and what the latency budget
        // is calibrated against.
        if let base = ModelCatalog.installedURL(of: "ggml-base.en.bin") { return base }
        return Config.findModel()
    }

    /// Lowercase, punctuation-free form for fuzzy matching — whisper varies
    /// casing and punctuation between runs, so exact matches would be flaky.
    private static func normalize(_ s: String) -> String {
        let stripped = s.lowercased().map { ch -> Character in
            ch.isLetter || ch.isNumber || ch.isWhitespace ? ch : " "
        }
        return String(stripped).split(separator: " ").joined(separator: " ")
    }

    private func log(_ message: String) {
        XCTContext.runActivity(named: message) { _ in }
        print("[EngineIntegration] \(message)")
    }

    private func fmt(_ t: TimeInterval) -> String { String(format: "%.2f", t) }
}
