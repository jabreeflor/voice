import XCTest
@testable import VoiceCore

/// Model discovery decides whether the app can transcribe at all. These tests
/// only ever *read* the real models directories; the only files they create
/// live in a temp directory that is deleted in tearDown.
final class ConfigTests: XCTestCase {

    private var dir: URL!

    override func setUp() {
        super.setUp()
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("VoiceConfigTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        unsetenv("THEVOICE_MODEL")
    }

    override func tearDown() {
        unsetenv("THEVOICE_MODEL")
        if let dir = dir { try? FileManager.default.removeItem(at: dir) }
        dir = nil
        super.tearDown()
    }

    private func makeFakeModel(named name: String = "ggml-fake.bin") -> URL {
        let u = dir.appendingPathComponent(name)
        FileManager.default.createFile(atPath: u.path, contents: Data([0x00]))
        return u
    }

    // MARK: - Preference ordering

    /// base.en leads deliberately: it transcribes a sentence in about a second,
    /// which is what makes hold-to-talk feel instant.
    func testBaseEnglishIsTheFirstPreference() {
        XCTAssertEqual(Config.preferredModels.first, "ggml-base.en.bin")
    }

    func testPreferenceOrderRunsFastToAccurateWithTinyLast() {
        let order = Config.preferredModels
        func idx(_ name: String) -> Int {
            guard let i = order.firstIndex(of: name) else {
                XCTFail("\(name) missing from preferredModels"); return .max
            }
            return i
        }
        XCTAssertLessThan(idx("ggml-base.en.bin"), idx("ggml-small.en.bin"))
        XCTAssertLessThan(idx("ggml-small.en.bin"), idx("ggml-medium.en.bin"))
        XCTAssertLessThan(idx("ggml-medium.en.bin"), idx("ggml-large-v3-turbo.bin"))
        XCTAssertLessThan(idx("ggml-large-v3-turbo.bin"), idx("ggml-tiny.en.bin"),
                          "tiny is the last resort, not an early pick")
        XCTAssertEqual(order.last, "ggml-tiny.bin")
    }

    func testEnglishOnlyVariantsAreTriedBeforeMultilingualOnes() {
        let order = Config.preferredModels
        for pair in [("ggml-base.en.bin", "ggml-base.bin"),
                     ("ggml-small.en.bin", "ggml-small.bin"),
                     ("ggml-medium.en.bin", "ggml-medium.bin"),
                     ("ggml-tiny.en.bin", "ggml-tiny.bin")] {
            let en = order.firstIndex(of: pair.0)
            let multi = order.firstIndex(of: pair.1)
            XCTAssertNotNil(en); XCTAssertNotNil(multi)
            XCTAssertLessThan(en!, multi!, "\(pair.0) should be preferred over \(pair.1)")
        }
    }

    func testPreferenceListHasNoDuplicates() {
        XCTAssertEqual(Set(Config.preferredModels).count, Config.preferredModels.count)
    }

    func testEveryDownloadableModelIsAlsoDiscoverable() {
        for spec in ModelCatalog.all {
            XCTAssertTrue(Config.preferredModels.contains(spec.file),
                          "\(spec.file) can be downloaded but would never be auto-found")
        }
    }

    // MARK: - Search directories

    func testModelsDirectoriesAreUnderTheUserHomeInOrder() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        XCTAssertEqual(Config.modelsDirs.map(\.path), [
            home.appendingPathComponent("thevoice/models").path,
            home.appendingPathComponent(".thevoice/models").path,
        ])
    }

    // MARK: - THEVOICE_MODEL override

    func testEnvironmentOverrideTakesPrecedenceOverEverythingElse() {
        let fake = makeFakeModel()
        setenv("THEVOICE_MODEL", fake.path, 1)
        XCTAssertEqual(Config.findModel()?.path, fake.path,
                       "THEVOICE_MODEL should win over the preference scan")
    }

    /// The override is not filtered by the ggml naming convention — whatever
    /// path is given is used as-is.
    func testEnvironmentOverrideAcceptsAnyFilename() {
        let odd = makeFakeModel(named: "my-custom-weights.gguf")
        setenv("THEVOICE_MODEL", odd.path, 1)
        XCTAssertEqual(Config.findModel()?.path, odd.path)
    }

    func testEnvironmentOverridePointingAtAMissingFileIsIgnored() {
        let missing = dir.appendingPathComponent("not-there.bin").path
        setenv("THEVOICE_MODEL", missing, 1)
        XCTAssertNotEqual(Config.findModel()?.path, missing,
                          "a dangling override must fall through to the normal search")
    }

    func testEnvironmentOverrideExpandsATilde() throws {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let installed = ModelCatalog.all
            .compactMap { ModelCatalog.installedURL(of: $0.file) }
            .first { $0.path.hasPrefix(home + "/") }
        try XCTSkipIf(installed == nil, "no installed model under $HOME to point the override at")
        let real = installed!

        setenv("THEVOICE_MODEL", "~" + real.path.dropFirst(home.count), 1)
        XCTAssertEqual(Config.findModel()?.path, real.path)
    }

    func testEmptyEnvironmentOverrideIsIgnored() {
        setenv("THEVOICE_MODEL", "", 1)
        XCTAssertNotEqual(Config.findModel()?.path, "")
    }

    // MARK: - findModel invariants

    /// Whatever findModel returns must actually be on disk, or the engine is
    /// launched against a path that cannot be opened.
    func testFindModelOnlyReturnsPathsThatExist() {
        guard let found = Config.findModel() else { return }
        XCTAssertTrue(FileManager.default.fileExists(atPath: found.path))
    }

    func testFindModelIsStableAcrossCalls() {
        XCTAssertEqual(Config.findModel()?.path, Config.findModel()?.path)
    }

    // MARK: - ModelCatalog

    func testDefaultSpecIsBaseEnglish() {
        XCTAssertEqual(ModelCatalog.defaultSpec.file, "ggml-base.en.bin")
    }

    func testDownloadURLsPointAtTheWhisperCppRepo() {
        for spec in ModelCatalog.all {
            let url = ModelCatalog.downloadURL(for: spec)
            XCTAssertEqual(url.absoluteString,
                           "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/\(spec.file)")
            XCTAssertEqual(url.scheme, "https")
        }
    }

    func testCatalogEntriesAreUniqueAndLabelled() {
        XCTAssertEqual(Set(ModelCatalog.all.map(\.file)).count, ModelCatalog.all.count)
        for spec in ModelCatalog.all {
            XCTAssertFalse(spec.label.isEmpty)
            XCTAssertTrue(spec.file.hasPrefix("ggml"))
            XCTAssertEqual((spec.file as NSString).pathExtension, "bin")
        }
    }

    func testInstalledURLIsNilForAModelThatIsNotThere() {
        XCTAssertNil(ModelCatalog.installedURL(of: "ggml-not-a-real-model-\(UUID().uuidString).bin"))
    }

    func testInstalledURLReturnsAPathInsideASearchDirectory() {
        let dirs = Set(Config.modelsDirs.map(\.path))
        for spec in ModelCatalog.all {
            guard let u = ModelCatalog.installedURL(of: spec.file) else { continue }
            XCTAssertTrue(dirs.contains(u.deletingLastPathComponent().path))
            XCTAssertEqual(u.lastPathComponent, spec.file)
            XCTAssertTrue(FileManager.default.fileExists(atPath: u.path))
        }
    }

    // MARK: - Hotkey

    func testHotkeyKeyCodesAreDistinct() {
        let codes = Hotkey.allCases.map(\.keyCode)
        XCTAssertEqual(Set(codes).count, codes.count)
    }

    func testHotkeyRawValuesRoundTrip() {
        for hk in Hotkey.allCases {
            XCTAssertEqual(Hotkey(rawValue: hk.rawValue), hk)
            XCTAssertFalse(hk.label.isEmpty)
            XCTAssertFalse(hk.shortLabel.isEmpty)
        }
    }

    func testUnknownHotkeyRawValueDoesNotResolve() {
        XCTAssertNil(Hotkey(rawValue: "leftPinky"))
    }
}
