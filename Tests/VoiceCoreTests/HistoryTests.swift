import XCTest
@testable import VoiceCore

/// Every test here runs against a throwaway directory and a throwaway
/// UserDefaults suite. Nothing touches ~/Library/Application Support/Voice or
/// UserDefaults.standard, both of which hold the user's real dictation history.
final class HistoryTests: XCTestCase {

    private var dir: URL!
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        let id = UUID().uuidString
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("VoiceHistoryTests-\(id)")
        suiteName = "test-voice-history-\(id)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
        defaults = nil
        suiteName = nil
        if let dir = dir { try? FileManager.default.removeItem(at: dir) }
        dir = nil
        super.tearDown()
    }

    private func makeStore() -> HistoryStore {
        HistoryStore(directory: dir, defaults: defaults)
    }

    private func entry(_ text: String,
                       date: Date = Date(),
                       duration: Double = 0,
                       latency: Double = 0) -> DictationEntry {
        DictationEntry(text: text, date: date, duration: duration, latency: latency)
    }

    private func daysAgo(_ n: Int) -> Date {
        Calendar.current.date(byAdding: .day, value: -n, to: Date())!
    }

    // MARK: - add

    func testNewEntriesGoToTheFront() {
        let store = makeStore()
        store.add(entry("first"))
        store.add(entry("second"))
        store.add(entry("third"))
        XCTAssertEqual(store.entries.map(\.text), ["third", "second", "first"])
    }

    func testStampAdvancesOnEachAdd() {
        let store = makeStore()
        let start = store.stamp
        store.add(entry("one"))
        store.add(entry("two"))
        XCTAssertEqual(store.stamp, start + 2)
    }

    func testHistoryIsCappedAtThreeHundredEntries() {
        let store = makeStore()
        for i in 0..<305 { store.add(entry("entry \(i)")) }
        XCTAssertEqual(store.entries.count, 300)
        XCTAssertEqual(store.entries.first?.text, "entry 304", "newest entry should survive")
        XCTAssertEqual(store.entries.last?.text, "entry 5", "oldest entries are dropped")
    }

    // MARK: - totalWords

    func testTotalWordsAccumulatesAcrossAdds() {
        let store = makeStore()
        XCTAssertEqual(store.totalWords, 0)
        store.add(entry("one two three"))
        XCTAssertEqual(store.totalWords, 3)
        store.add(entry("four five"))
        XCTAssertEqual(store.totalWords, 5)
    }

    func testTotalWordsIgnoresRepeatedSpacesAndEmptyText() {
        let store = makeStore()
        store.add(entry("one   two"))
        XCTAssertEqual(store.totalWords, 2)
        store.add(entry(""))
        XCTAssertEqual(store.totalWords, 2)
    }

    /// totalWords is a lifetime counter kept in defaults, so trimming the entry
    /// list at 300 must not roll it back.
    func testTotalWordsIsNotReducedWhenOldEntriesAreTrimmed() {
        let store = makeStore()
        for _ in 0..<305 { store.add(entry("two words")) }
        XCTAssertEqual(store.entries.count, 300)
        XCTAssertEqual(store.totalWords, 610)
    }

    func testTotalWordsSurvivesAReload() {
        let store = makeStore()
        store.add(entry("one two three"))
        XCTAssertEqual(makeStore().totalWords, 3)
    }

    func testTotalWordsIsReadFromTheInjectedSuiteOnly() {
        defaults.set(42, forKey: "wordsTotal")
        XCTAssertEqual(makeStore().totalWords, 42)
    }

    // MARK: - Persistence

    func testEntriesRoundTripThroughJSON() {
        let fixed = Date(timeIntervalSinceReferenceDate: 700_000_000)
        let store = makeStore()
        store.add(entry("hello world", date: fixed, duration: 1.25, latency: 0.4))
        store.add(entry("second one", date: fixed.addingTimeInterval(60), duration: 2.5, latency: 0.2))

        let reloaded = makeStore()
        XCTAssertEqual(reloaded.entries, store.entries)
        XCTAssertEqual(reloaded.entries.first?.text, "second one")
        XCTAssertEqual(reloaded.entries.first?.duration, 2.5)
        XCTAssertEqual(reloaded.entries.first?.latency, 0.2)
        XCTAssertEqual(reloaded.entries.first?.date, fixed.addingTimeInterval(60))
    }

    func testHistoryFileIsWrittenIntoTheInjectedDirectory() {
        let store = makeStore()
        store.add(entry("hello"))
        let file = dir.appendingPathComponent("history.json")
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
        let decoded = try? JSONDecoder().decode([DictationEntry].self,
                                                from: Data(contentsOf: file))
        XCTAssertEqual(decoded?.map(\.text), ["hello"])
    }

    func testFreshDirectoryStartsEmpty() {
        XCTAssertTrue(makeStore().entries.isEmpty)
    }

    func testCorruptHistoryFileLeavesStoreEmptyRatherThanCrashing() {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? Data("{{ not json".utf8).write(to: dir.appendingPathComponent("history.json"))
        let store = makeStore()
        XCTAssertTrue(store.entries.isEmpty)
        store.add(entry("recovered"))
        XCTAssertEqual(makeStore().entries.map(\.text), ["recovered"])
    }

    // MARK: - grouped()

    func testGroupedOnEmptyHistoryReturnsNoGroups() {
        XCTAssertTrue(makeStore().grouped().isEmpty)
    }

    func testGroupedLabelsTodayYesterdayAndOlderDays() {
        let store = makeStore()
        let old = daysAgo(5)
        store.add(entry("old one", date: old))
        store.add(entry("yesterday one", date: daysAgo(1)))
        store.add(entry("today one", date: Date()))

        let groups = store.grouped()
        XCTAssertEqual(groups.count, 3)
        XCTAssertEqual(groups[0].0, "Today")
        XCTAssertEqual(groups[1].0, "Yesterday")

        let fmt = DateFormatter()
        fmt.dateFormat = "EEEE, MMM d"
        XCTAssertEqual(groups[2].0, fmt.string(from: old))
        XCTAssertEqual(groups[2].1.map(\.text), ["old one"])
    }

    func testConsecutiveEntriesFromTheSameDayShareOneGroup() {
        let store = makeStore()
        store.add(entry("a", date: Date()))
        store.add(entry("b", date: Date()))
        store.add(entry("c", date: Date()))

        let groups = store.grouped()
        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].0, "Today")
        XCTAssertEqual(groups[0].1.map(\.text), ["c", "b", "a"])
    }

    /// Grouping only merges *adjacent* entries, so a day that reappears later in
    /// the list opens a second group with the same title. Entries are normally
    /// added in chronological order, so this only shows up with back-dated data.
    func testSameDayReappearingLaterOpensASecondGroup() {
        let store = makeStore()
        store.add(entry("early today", date: Date()))
        store.add(entry("yesterday", date: daysAgo(1)))
        store.add(entry("late today", date: Date()))

        let titles = store.grouped().map(\.0)
        XCTAssertEqual(titles, ["Today", "Yesterday", "Today"])
    }

    func testGroupedHonorsTheMaxEntryLimit() {
        let store = makeStore()
        for i in 0..<10 { store.add(entry("entry \(i)", date: Date())) }
        let groups = store.grouped(max: 4)
        XCTAssertEqual(groups.reduce(0) { $0 + $1.1.count }, 4)
        XCTAssertEqual(groups[0].1.map(\.text), ["entry 9", "entry 8", "entry 7", "entry 6"])
    }

    func testGroupedDefaultsToFortyEntries() {
        let store = makeStore()
        for i in 0..<60 { store.add(entry("entry \(i)", date: Date())) }
        XCTAssertEqual(store.grouped().reduce(0) { $0 + $1.1.count }, 40)
    }

    // MARK: - averageWPM

    func testAverageWPMOverTimedEntries() {
        let store = makeStore()
        // 10 words in 6 seconds = 100 wpm.
        store.add(entry("one two three four five six seven eight nine ten", duration: 6.0))
        XCTAssertEqual(store.averageWPM, 100)
    }

    func testAverageWPMPoolsWordsAndSecondsAcrossEntries() {
        let store = makeStore()
        store.add(entry("one two three four five", duration: 3.0))
        store.add(entry("six seven eight nine ten", duration: 3.0))
        XCTAssertEqual(store.averageWPM, 100)
    }

    /// Entries shorter than half a second are keystroke-length noise; letting
    /// them in would inflate the rate wildly.
    func testAverageWPMIgnoresEntriesUnderHalfASecond() {
        let store = makeStore()
        store.add(entry("one two three four five six seven eight nine ten", duration: 6.0))
        store.add(entry(String(repeating: "word ", count: 200), duration: 0.3))
        XCTAssertEqual(store.averageWPM, 100, "sub-0.5s entry should not affect the rate")
    }

    func testAverageWPMExcludesEntriesAtExactlyHalfASecond() {
        let store = makeStore()
        store.add(entry("one two three four five six seven eight nine ten", duration: 6.0))
        store.add(entry("a b c d e f g h i j k l", duration: 0.5))
        XCTAssertEqual(store.averageWPM, 100, "the threshold is strictly greater than 0.5")
    }

    func testAverageWPMIsZeroWithoutAtLeastASecondOfSpeech() {
        let store = makeStore()
        store.add(entry("one two three", duration: 0.6))
        XCTAssertEqual(store.averageWPM, 0)
    }

    func testAverageWPMIsZeroWhenNoEntryIsTimed() {
        let store = makeStore()
        store.add(entry("untimed legacy entry", duration: 0))
        XCTAssertEqual(store.averageWPM, 0)
    }

    func testAverageWPMIsZeroOnEmptyHistory() {
        XCTAssertEqual(makeStore().averageWPM, 0)
    }

    func testAverageWPMRoundsToNearestWholeNumber() {
        let store = makeStore()
        // 10 words in 7 seconds = 85.71… wpm.
        store.add(entry("one two three four five six seven eight nine ten", duration: 7.0))
        XCTAssertEqual(store.averageWPM, 86)
    }

    // MARK: - averageLatency

    func testAverageLatencyOverRecentEntries() {
        let store = makeStore()
        store.add(entry("a", latency: 0.1))
        store.add(entry("b", latency: 0.2))
        store.add(entry("c", latency: 0.3))
        XCTAssertEqual(store.averageLatency, 0.2, accuracy: 0.0001)
    }

    func testAverageLatencyIgnoresEntriesWithoutATiming() {
        let store = makeStore()
        store.add(entry("a", latency: 0.2))
        store.add(entry("b", latency: 0))
        store.add(entry("c", latency: 0.4))
        XCTAssertEqual(store.averageLatency, 0.3, accuracy: 0.0001)
    }

    func testAverageLatencyIsZeroWhenNothingIsTimed() {
        let store = makeStore()
        store.add(entry("a", latency: 0))
        XCTAssertEqual(store.averageLatency, 0)
        XCTAssertEqual(makeStore().averageLatency, 0)
    }

    func testAverageLatencyOnlyConsidersTheFiftyNewestEntries() {
        let store = makeStore()
        for _ in 0..<10 { store.add(entry("old", latency: 10.0)) }
        for _ in 0..<50 { store.add(entry("recent", latency: 0.5)) }
        XCTAssertEqual(store.entries.count, 60)
        XCTAssertEqual(store.averageLatency, 0.5, accuracy: 0.0001,
                       "the 10 slow older entries sit outside the 50-entry window")
    }

    /// Untimed entries must not shrink the sample — the window is the last 50
    /// *timed* entries, so older timed ones are pulled in past untimed noise.
    func testUntimedRecentEntriesDoNotCrowdOutTimedOnes() {
        let store = makeStore()
        for _ in 0..<5 { store.add(entry("timed", latency: 0.4)) }
        for _ in 0..<50 { store.add(entry("untimed", latency: 0)) }
        XCTAssertEqual(store.averageLatency, 0.4, accuracy: 0.0001)
    }

    // MARK: - Legacy migration

    func testLegacyStringHistoryIsImportedOnFirstLoad() {
        defaults.set(["hello world", "second entry"], forKey: "history")
        let store = makeStore()
        XCTAssertEqual(store.entries.map(\.text), ["hello world", "second entry"])
        XCTAssertEqual(store.totalWords, 4)
    }

    func testMigratedEntriesHaveNoTimings() {
        defaults.set(["hello world"], forKey: "history")
        let migrated = makeStore().entries[0]
        XCTAssertEqual(migrated.duration, 0)
        XCTAssertEqual(migrated.latency, 0)
    }

    func testMigratedEntriesAreBackDatedAMinuteApart() {
        defaults.set(["newest", "middle", "oldest"], forKey: "history")
        let entries = makeStore().entries
        XCTAssertEqual(entries.count, 3)
        XCTAssertEqual(entries[0].date.timeIntervalSince(entries[1].date), 60, accuracy: 1)
        XCTAssertEqual(entries[1].date.timeIntervalSince(entries[2].date), 60, accuracy: 1)
    }

    func testLegacyKeyIsClearedSoMigrationRunsOnlyOnce() {
        defaults.set(["hello world"], forKey: "history")
        _ = makeStore()
        XCTAssertNil(defaults.stringArray(forKey: "history"))
        XCTAssertEqual(makeStore().entries.count, 1, "a second load must not re-import")
        XCTAssertEqual(makeStore().totalWords, 2, "word count must not be double-charged")
    }

    func testMigrationIsPersistedToDisk() {
        defaults.set(["hello world"], forKey: "history")
        _ = makeStore()
        let file = dir.appendingPathComponent("history.json")
        let decoded = try? JSONDecoder().decode([DictationEntry].self,
                                                from: Data(contentsOf: file))
        XCTAssertEqual(decoded?.map(\.text), ["hello world"])
    }

    func testLegacyEntriesAreAppendedAfterExistingHistory() {
        let store = makeStore()
        store.add(entry("modern entry"))
        defaults.set(["legacy entry"], forKey: "history")
        XCTAssertEqual(makeStore().entries.map(\.text), ["modern entry", "legacy entry"])
    }

    func testEmptyLegacyHistoryIsANoOp() {
        defaults.set([String](), forKey: "history")
        let store = makeStore()
        XCTAssertTrue(store.entries.isEmpty)
        XCTAssertEqual(store.totalWords, 0)
    }

    func testMissingLegacyHistoryIsANoOp() {
        XCTAssertTrue(makeStore().entries.isEmpty)
    }
}
