import AppKit
import XCTest
@testable import VoiceCore

/// Decision-table coverage for `computeStatus`. The branches are checked in a
/// fixed order, so most cases here pin down one branch plus the precedence that
/// keeps the branches above it from stealing the result.
final class StatusTests: XCTestCase {

    // MARK: - Fixture

    /// Healthy, fully-ready app. Each test overrides only what it cares about.
    private func inputs(
        micDenied: Bool = false,
        tapRunning: Bool = true,
        axTrusted: Bool = true,
        recentlyRelaunched: Bool = false,
        onboardingVisible: Bool = false,
        setupProgress: Double? = nil,
        setupFailed: Bool = false,
        engineExists: Bool = true,
        engineReady: Bool = true,
        engineStatusText: String = "",
        hotkeyLabel: String = "Right ⌥ Option"
    ) -> StatusInputs {
        StatusInputs(
            micDenied: micDenied,
            tapRunning: tapRunning,
            axTrusted: axTrusted,
            recentlyRelaunched: recentlyRelaunched,
            onboardingVisible: onboardingVisible,
            setupProgress: setupProgress,
            setupFailed: setupFailed,
            engineExists: engineExists,
            engineReady: engineReady,
            engineStatusText: engineStatusText,
            hotkeyLabel: hotkeyLabel)
    }

    private func assertContains(
        _ info: StatusInfo,
        _ needle: String,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertTrue(info.text.contains(needle),
                      "expected status text to contain \"\(needle)\", got \"\(info.text)\"",
                      file: file, line: line)
    }

    // MARK: - Microphone denied

    func testMicDeniedIsRedAndDoesNotAskForAccessibility() {
        let s = computeStatus(inputs(micDenied: true))
        assertContains(s, "Microphone access is off")
        XCTAssertEqual(s.color, .systemRed)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testMicDeniedBeatsSetupFailed() {
        let s = computeStatus(inputs(micDenied: true, setupFailed: true))
        assertContains(s, "Microphone access is off")
    }

    func testMicDeniedBeatsStoppedTap() {
        let s = computeStatus(inputs(micDenied: true, tapRunning: false, axTrusted: false))
        assertContains(s, "Microphone access is off")
        XCTAssertFalse(s.needsAccessibility)
    }

    func testMicDeniedBeatsSetupProgress() {
        let s = computeStatus(inputs(micDenied: true, setupProgress: 0.5))
        assertContains(s, "Microphone access is off")
        XCTAssertEqual(s.color, .systemRed)
    }

    func testMicDeniedBeatsEngineReady() {
        let s = computeStatus(inputs(micDenied: true, engineReady: true))
        assertContains(s, "Microphone access is off")
        XCTAssertNotEqual(s.color, .systemGreen)
    }

    // MARK: - Event tap not running

    func testTrustedButBlockedAfterRelaunchAsksForToggle() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: true, recentlyRelaunched: true))
        assertContains(s, "Permission granted but blocked")
        assertContains(s, "toggle Voice off and on")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertTrue(s.needsAccessibility)
    }

    func testTrustedAndNotYetRelaunchedSaysRestarting() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: true, recentlyRelaunched: false))
        assertContains(s, "restarting Voice")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testTrustedDuringOnboardingSaysSetupWillFinishWhenItCloses() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: true,
                                     recentlyRelaunched: false, onboardingVisible: true))
        XCTAssertEqual(s.text,
                       "Permission granted — Voice will finish applying it when setup closes")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testRecentlyRelaunchedBeatsOnboardingVisible() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: true,
                                     recentlyRelaunched: true, onboardingVisible: true))
        assertContains(s, "Permission granted but blocked")
        assertContains(s, "toggle Voice off and on")
        XCTAssertTrue(s.needsAccessibility)
    }

    func testNotTrustedAsksToGrantPermission() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: false))
        assertContains(s, "Grant Accessibility permission")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertTrue(s.needsAccessibility)
    }

    func testNotTrustedIgnoresRecentlyRelaunched() {
        let fresh = computeStatus(inputs(tapRunning: false, axTrusted: false, recentlyRelaunched: false))
        let relaunched = computeStatus(inputs(tapRunning: false, axTrusted: false, recentlyRelaunched: true))
        XCTAssertEqual(fresh, relaunched)
    }

    func testStoppedTapBeatsEngineReady() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: false, engineReady: true))
        assertContains(s, "Grant Accessibility permission")
        XCTAssertNotEqual(s.color, .systemGreen)
    }

    func testStoppedTapBeatsSetupProgress() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: false, setupProgress: 0.3))
        assertContains(s, "Grant Accessibility permission")
        XCTAssertTrue(s.needsAccessibility)
    }

    func testStoppedTapBeatsSetupFailed() {
        let s = computeStatus(inputs(tapRunning: false, axTrusted: true, recentlyRelaunched: true, setupFailed: true))
        assertContains(s, "Permission granted but blocked")
        XCTAssertEqual(s.color, .systemOrange)
    }

    func testRunningTapNeverAsksForAccessibility() {
        for axTrusted in [true, false] {
            for relaunched in [true, false] {
                let s = computeStatus(inputs(tapRunning: true,
                                             axTrusted: axTrusted,
                                             recentlyRelaunched: relaunched))
                XCTAssertFalse(s.needsAccessibility,
                               "axTrusted=\(axTrusted) recentlyRelaunched=\(relaunched)")
            }
        }
    }

    // MARK: - Setup progress

    func testProgressZeroShowsZeroPercent() {
        let s = computeStatus(inputs(setupProgress: 0.0))
        assertContains(s, "0%")
        assertContains(s, "Setting up")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testProgressFortyTwoPercent() {
        let s = computeStatus(inputs(setupProgress: 0.42))
        assertContains(s, "42%")
        XCTAssertEqual(s.color, .systemOrange)
    }

    func testProgressOneShowsHundredPercent() {
        let s = computeStatus(inputs(setupProgress: 1.0))
        assertContains(s, "100%")
        XCTAssertEqual(s.color, .systemOrange)
    }

    /// The percentage truncates rather than rounds, so 99.9% reads as 99%.
    func testProgressTruncatesTowardZero() {
        let s = computeStatus(inputs(setupProgress: 0.999))
        assertContains(s, "99%")
    }

    func testProgressBeatsSetupFailed() {
        let s = computeStatus(inputs(setupProgress: 0.6, setupFailed: true))
        assertContains(s, "60%")
        XCTAssertEqual(s.color, .systemOrange)
    }

    func testProgressBeatsMissingEngine() {
        let s = computeStatus(inputs(setupProgress: 0.1, engineExists: false, engineReady: false))
        assertContains(s, "10%")
    }

    func testProgressBeatsEngineReady() {
        let s = computeStatus(inputs(setupProgress: 0.25, engineReady: true))
        assertContains(s, "25%")
        XCTAssertNotEqual(s.color, .systemGreen)
    }

    // MARK: - Setup failed

    func testSetupFailedIsRed() {
        let s = computeStatus(inputs(setupProgress: nil, setupFailed: true))
        assertContains(s, "Setup failed")
        XCTAssertEqual(s.color, .systemRed)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testSetupFailedBeatsMissingEngine() {
        let s = computeStatus(inputs(setupFailed: true, engineExists: false, engineReady: false))
        assertContains(s, "Setup failed")
        XCTAssertEqual(s.color, .systemRed)
    }

    func testSetupFailedBeatsEngineReady() {
        let s = computeStatus(inputs(setupFailed: true, engineReady: true))
        assertContains(s, "Setup failed")
        XCTAssertEqual(s.color, .systemRed)
    }

    // MARK: - Engine missing

    func testMissingEngineIsPreparing() {
        let s = computeStatus(inputs(engineExists: false, engineReady: false))
        XCTAssertEqual(s.text, "Preparing")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertFalse(s.needsAccessibility)
    }

    /// A nonexistent engine cannot be ready; the guard runs first either way.
    func testMissingEngineBeatsReadyFlag() {
        let s = computeStatus(inputs(engineExists: false, engineReady: true))
        XCTAssertEqual(s.text, "Preparing")
    }

    func testMissingEngineBeatsNotInstalledText() {
        let s = computeStatus(inputs(engineExists: false,
                                     engineReady: false,
                                     engineStatusText: "whisper-server not installed"))
        XCTAssertEqual(s.text, "Preparing")
        XCTAssertEqual(s.color, .systemOrange)
    }

    // MARK: - Engine ready

    func testReadyIsGreenAndMentionsDefaultHotkey() {
        let s = computeStatus(inputs())
        assertContains(s, "Ready")
        assertContains(s, "Right ⌥ Option")
        XCTAssertEqual(s.color, .systemGreen)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testReadyRoundTripsCustomHotkeyLabel() {
        let label = "Fn + ⌃ Control"
        let s = computeStatus(inputs(hotkeyLabel: label))
        assertContains(s, label)
        XCTAssertEqual(s.color, .systemGreen)
    }

    func testReadyBeatsNotInstalledText() {
        let s = computeStatus(inputs(engineReady: true,
                                     engineStatusText: "whisper-server not installed"))
        assertContains(s, "Ready")
        XCTAssertEqual(s.color, .systemGreen)
    }

    // MARK: - whisper-server missing

    func testNotInstalledSuggestsBrew() {
        let s = computeStatus(inputs(engineReady: false,
                                     engineStatusText: "whisper-server not installed"))
        assertContains(s, "brew install whisper-cpp")
        XCTAssertEqual(s.color, .systemRed)
        XCTAssertFalse(s.needsAccessibility)
    }

    /// The match is an exact string compare, so near-misses fall through.
    func testNotInstalledMatchIsExact() {
        let s = computeStatus(inputs(engineReady: false,
                                     engineStatusText: "whisper-server not installed."))
        assertContains(s, "Starting the speech engine")
        XCTAssertEqual(s.color, .systemOrange)
    }

    // MARK: - Fallback

    func testFallbackIsStartingEngine() {
        let s = computeStatus(inputs(engineReady: false, engineStatusText: ""))
        XCTAssertEqual(s.text, "Starting the speech engine")
        XCTAssertEqual(s.color, .systemOrange)
        XCTAssertFalse(s.needsAccessibility)
    }

    func testFallbackForUnrecognizedEngineStatus() {
        let s = computeStatus(inputs(engineReady: false, engineStatusText: "loading model"))
        XCTAssertEqual(s.text, "Starting the speech engine")
        XCTAssertEqual(s.color, .systemOrange)
    }

    // MARK: - Purity

    func testSameInputsProduceSameOutput() {
        let i = inputs(tapRunning: false, axTrusted: true, recentlyRelaunched: true)
        XCTAssertEqual(computeStatus(i), computeStatus(i))
    }
}
