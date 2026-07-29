import XCTest
import AppKit
import CoreGraphics
@testable import VoiceCore

/// The talk key is bindable to anything the keyboard can produce, so the
/// interesting surface is wider than the three presets: labelling arbitrary
/// keycodes, refusing bindings that cannot work, round-tripping through
/// `UserDefaults`, and — the part users actually feel — the event tap's
/// press/release bookkeeping for non-modifier keys.
final class HotkeyTests: XCTestCase {

    private var suite: String!

    override func setUp() {
        super.setUp()
        suite = "voice-hotkey-tests-\(UUID().uuidString)"
        Config.defaults = UserDefaults(suiteName: suite)!
    }

    override func tearDown() {
        UserDefaults.standard.removePersistentDomain(forName: suite)
        Config.defaults = .standard
        suite = nil
        super.tearDown()
    }

    // MARK: - Modifier keys

    func testModifierKeyCodesAreDistinct() {
        let codes = ModifierKey.allCases.map(\.rawValue)
        XCTAssertEqual(Set(codes).count, codes.count)
    }

    func testEveryModifierKeyIsNamed() {
        for m in ModifierKey.allCases {
            XCTAssertFalse(m.shortName.isEmpty, "\(m) has no short name")
            XCTAssertFalse(m.longName.isEmpty, "\(m) has no long name")
        }
    }

    func testLeftAndRightOfAPairShareAFlagButNotAKeyCode() {
        XCTAssertEqual(ModifierKey.leftOption.flag, ModifierKey.rightOption.flag)
        XCTAssertNotEqual(ModifierKey.leftOption.rawValue, ModifierKey.rightOption.rawValue)
        XCTAssertEqual(ModifierKey.leftCommand.flag, ModifierKey.rightCommand.flag)
        XCTAssertNotEqual(ModifierKey.leftCommand.rawValue, ModifierKey.rightCommand.rawValue)
    }

    // MARK: - Construction

    func testBoundModifierKeyIsNotAlsoStoredAsARequiredModifier() {
        // Right ⌥ always sets .maskAlternate itself; requiring it separately
        // would be redundant and would break equality with the preset.
        let hk = Hotkey(keyCode: ModifierKey.rightOption.rawValue,
                        modifiers: [.maskAlternate])
        XCTAssertEqual(hk.modifiers, [])
        XCTAssertEqual(hk, Hotkey.rightOption)
    }

    func testInsignificantFlagsAreDropped() {
        // Caps Lock, fn and the numeric-pad bit are set by the system on keys
        // that have nothing to do with the binding.
        let hk = Hotkey(keyCode: 2, modifiers: [.maskControl, .maskAlphaShift,
                                                .maskSecondaryFn, .maskNumericPad])
        XCTAssertEqual(hk.modifiers, [.maskControl])
    }

    func testSignificantModifiersExcludeCapsLockAndFn() {
        XCTAssertFalse(Hotkey.significantModifiers.contains(.maskAlphaShift))
        XCTAssertFalse(Hotkey.significantModifiers.contains(.maskSecondaryFn))
    }

    func testPresetsAreDistinctAndAllModifierKeys() {
        XCTAssertEqual(Set(Hotkey.presets.map(\.keyCode)).count, Hotkey.presets.count)
        for hk in Hotkey.presets {
            XCTAssertTrue(hk.isModifierKey, "\(hk.label) should be a modifier key")
            XCTAssertNil(hk.issue, "\(hk.label) should be offerable without a warning")
        }
    }

    func testFallbackIsRightOption() {
        XCTAssertEqual(Hotkey.fallback, Hotkey.rightOption)
        XCTAssertEqual(Hotkey.fallback.keyCode, 61)
    }

    // MARK: - Labels

    func testModifierOnlyBindingsKeepTheirSpelledOutNames() {
        XCTAssertEqual(Hotkey.rightOption.label, "Right ⌥ Option")
        XCTAssertEqual(Hotkey.rightOption.shortLabel, "Right ⌥")
        XCTAssertEqual(Hotkey.rightCommand.label, "Right ⌘ Command")
        XCTAssertEqual(Hotkey.fn.label, "fn")
    }

    func testRegularKeyLabelsCombineSymbolsAndKeyName() {
        let hk = Hotkey(keyCode: 2, modifiers: [.maskControl, .maskAlternate])
        XCTAssertEqual(hk.shortLabel, "⌃⌥D")
        XCTAssertEqual(hk.label, "⌃⌥D")
    }

    func testSpelledOutKeyNamesGetASpaceAfterTheSymbols() {
        // "⌃Right ⌥" is unreadable; "⌃ Right ⌥" is not.
        let hk = Hotkey(keyCode: ModifierKey.rightOption.rawValue,
                        modifiers: [.maskControl])
        XCTAssertEqual(hk.shortLabel, "⌃ Right ⌥")
        XCTAssertEqual(hk.label, "⌃ Right ⌥ Option")
    }

    func testEveryKeyCodeProducesANonEmptyLabel() {
        for code in Int64(0)...200 {
            XCTAssertFalse(Hotkey(keyCode: code).shortLabel.isEmpty,
                           "keycode \(code) has no label")
        }
    }

    func testSymbolsUseAppleOrdering() {
        let all: CGEventFlags = [.maskCommand, .maskShift, .maskAlternate, .maskControl]
        XCTAssertEqual(Hotkey.symbols(for: all), "⌃⌥⇧⌘")
    }

    func testUnknownKeyCodeStillGetsAUsableName() {
        // Binding "anything" includes keys this table has never heard of.
        XCTAssertEqual(Hotkey.keyName(for: 999), "Key 999")
        XCTAssertFalse(Hotkey(keyCode: 999).shortLabel.isEmpty)
    }

    func testCommonKeysAreNamed() {
        XCTAssertEqual(Hotkey.keyName(for: 49), "Space")
        XCTAssertEqual(Hotkey.keyName(for: 122), "F1")
        XCTAssertEqual(Hotkey.keyName(for: 105), "F13")
        XCTAssertEqual(Hotkey.keyName(for: 123), "←")
        XCTAssertEqual(Hotkey.keyName(for: 0), "A")
    }

    func testKeyNameDefersToModifierNamesForModifierKeyCodes() {
        XCTAssertEqual(Hotkey.keyName(for: ModifierKey.rightShift.rawValue), "Right ⇧")
    }

    // MARK: - Validation

    func testCapsLockIsRefused() {
        let hk = Hotkey(keyCode: ModifierKey.capsLock.rawValue)
        guard case .unusable(let why)? = hk.issue else {
            return XCTFail("Caps Lock should be unusable, got \(String(describing: hk.issue))")
        }
        XCTAssertFalse(why.isEmpty)
    }

    func testBareLetterIsRiskyButNotRefused() {
        guard case .risky(let why)? = Hotkey(keyCode: 0).issue else {
            return XCTFail("A bare letter should be flagged as risky")
        }
        XCTAssertTrue(why.contains("A"), "the warning should name the key: \(why)")
    }

    func testLetterWithAModifierIsFine() {
        XCTAssertNil(Hotkey(keyCode: 0, modifiers: [.maskControl]).issue)
        XCTAssertNil(Hotkey(keyCode: 0, modifiers: [.maskAlternate, .maskShift]).issue)
    }

    func testFunctionKeysAreFineOnTheirOwn() {
        for code in Hotkey.standaloneSafeKeyCodes {
            XCTAssertNil(Hotkey(keyCode: code).issue,
                         "\(Hotkey.keyName(for: code)) should be bindable bare")
        }
    }

    func testEveryModifierKeyExceptCapsLockIsBindableAlone() {
        for m in ModifierKey.allCases where m != .capsLock {
            XCTAssertNil(Hotkey(keyCode: m.rawValue).issue,
                         "\(m.longName) should be bindable alone")
        }
    }

    // MARK: - Persistence

    func testStoredValueRoundTrips() {
        let cases: [Hotkey] = [
            .rightOption, .rightCommand, .fn,
            Hotkey(keyCode: 2, modifiers: [.maskControl]),
            Hotkey(keyCode: 49, modifiers: [.maskCommand, .maskShift]),
            Hotkey(keyCode: 105),
            Hotkey(keyCode: 999, modifiers: [.maskAlternate]),
        ]
        for hk in cases {
            XCTAssertEqual(Hotkey(stored: hk.storedValue), hk,
                           "\(hk.storedValue) did not round-trip")
        }
    }

    func testLegacyNamesMigrateToTheEquivalentBinding() {
        XCTAssertEqual(Hotkey(stored: "rightOption"), .rightOption)
        XCTAssertEqual(Hotkey(stored: "rightCommand"), .rightCommand)
        XCTAssertEqual(Hotkey(stored: "fn"), .fn)
    }

    func testGarbageStoredValuesDoNotResolve() {
        for bad in ["", "leftPinky", "61", "61:", ":0", "a:b", "61:0:0"] {
            XCTAssertNil(Hotkey(stored: bad), "\"\(bad)\" should not parse")
        }
    }

    func testConfigFallsBackWhenNothingIsStored() {
        XCTAssertEqual(Config.hotkey, Hotkey.fallback)
    }

    func testConfigFallsBackWhenTheStoredValueIsUnreadable() {
        Config.defaults.set("leftPinky", forKey: "hotkey")
        XCTAssertEqual(Config.hotkey, Hotkey.fallback)
    }

    func testConfigWritesTheEncodedFormNotADescription() {
        Config.hotkey = Hotkey(keyCode: 40, modifiers: [.maskControl])
        XCTAssertEqual(Config.defaults.string(forKey: "hotkey"), "40:262144")
    }

    /// The status line's default in `StatusInputs` is written out by hand;
    /// this is the assertion that keeps the two in step.
    func testFallbackLabelMatchesTheStatusLineDefault() {
        XCTAssertEqual(Hotkey.fallback.label, "Right ⌥ Option")
    }

    func testConfigPersistsAnArbitraryBinding() {
        let hk = Hotkey(keyCode: 2, modifiers: [.maskControl, .maskAlternate])
        Config.hotkey = hk
        XCTAssertEqual(Config.hotkey, hk)
    }

    func testConfigUpgradesAnOldInstallInPlace() {
        Config.defaults.set("rightCommand", forKey: "hotkey")
        XCTAssertEqual(Config.hotkey, Hotkey.rightCommand)
    }

    func testNoLegacyNameCanBeMistakenForTheEncodedForm() {
        for name in ["rightOption", "rightCommand", "fn"] {
            XCTAssertFalse(name.contains(":"), "\(name) would collide with the n:m grammar")
        }
    }

    // MARK: - Telling the two sides of the keyboard apart

    func testDeviceFlagsAreDistinctWithinAPair() {
        for m in ModifierKey.allCases {
            guard let (mine, sibling) = m.deviceFlags else { continue }
            XCTAssertNotEqual(mine, sibling, "\(m) cannot distinguish its sibling")
        }
    }

    func testTheOtherSideOfAPairDoesNotCountAsHeld() {
        // Left ⌥ down: .maskAlternate is set, but the left device bit is the
        // one that's high. Right ⌥ must not read as held.
        let leftOptionDown: CGEventFlags = [.maskAlternate, CGEventFlags(rawValue: 0x20)]
        XCTAssertTrue(ModifierKey.leftOption.isHeld(in: leftOptionDown))
        XCTAssertFalse(ModifierKey.rightOption.isHeld(in: leftOptionDown))
    }

    func testSynthesizedEventsWithoutDeviceBitsStillRegister() {
        // Scripted events (the e2e smoke test) set only the shared flag.
        XCTAssertTrue(ModifierKey.rightOption.isHeld(in: [.maskAlternate]))
        XCTAssertTrue(ModifierKey.leftOption.isHeld(in: [.maskAlternate]))
    }

    func testAModifierIsNotHeldWhenItsSharedFlagIsClear() {
        XCTAssertFalse(ModifierKey.rightOption.isHeld(in: []))
        XCTAssertFalse(ModifierKey.rightOption.isHeld(in: [.maskCommand]))
    }

    func testFnAndCapsLockHaveNoSibling() {
        XCTAssertNil(ModifierKey.function.deviceFlags)
        XCTAssertNil(ModifierKey.capsLock.deviceFlags)
        XCTAssertTrue(ModifierKey.function.isHeld(in: [.maskSecondaryFn]))
    }

    // MARK: - AppKit bridging

    func testAppKitFlagsMapToEventFlags() {
        XCTAssertEqual(Hotkey.flags(from: [.control, .option]),
                       [.maskControl, .maskAlternate])
        XCTAssertEqual(Hotkey.flags(from: [.shift, .command]),
                       [.maskShift, .maskCommand])
        // fn and Caps Lock are deliberately not carried across.
        XCTAssertEqual(Hotkey.flags(from: [.function, .capsLock]), [])
    }

    func testModifierKeysAgreeAcrossBothFlagVocabularies() {
        for m in ModifierKey.allCases {
            XCTAssertEqual(Hotkey.flags(from: m.appKitFlag),
                           m.flag.intersection(Hotkey.significantModifiers),
                           "\(m) disagrees between AppKit and CoreGraphics")
        }
    }
}

// MARK: - Event tap behaviour

/// `HotkeyController.handle` is the whole hold-to-talk contract, and it is
/// reachable without an installed tap: synthesized `CGEvent`s can be handed
/// to it directly. Creating events needs no permissions — only posting does.
final class HotkeyControllerTests: XCTestCase {

    private var suite: String!
    private var controller: HotkeyController!
    private var events: [String] = []
    private var recording = false

    override func setUp() {
        super.setUp()
        suite = "voice-hotkey-tap-tests-\(UUID().uuidString)"
        Config.defaults = UserDefaults(suiteName: suite)!
        events = []
        recording = false
        controller = HotkeyController()
        controller.onDown = { [weak self] in
            self?.events.append("down")
            self?.recording = true
        }
        controller.onUp = { [weak self] in
            self?.events.append("up")
            self?.recording = false
        }
        controller.onCancel = { [weak self] in
            self?.events.append("cancel")
            self?.recording = false
        }
        controller.isActive = { [weak self] in self?.recording ?? false }
    }

    override func tearDown() {
        controller = nil
        UserDefaults.standard.removePersistentDomain(forName: suite)
        Config.defaults = .standard
        suite = nil
        super.tearDown()
    }

    /// The callbacks hop to the main queue, so the queue has to be drained
    /// before the recorded events can be inspected.
    private func drain() {
        let done = expectation(description: "main queue drained")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 2)
    }

    private func makeEvent(keyCode: Int64, type: CGEventType,
                           flags: CGEventFlags = [],
                           autorepeat: Bool = false) -> CGEvent {
        let src = CGEventSource(stateID: .hidSystemState)
        let e = CGEvent(keyboardEventSource: src,
                        virtualKey: CGKeyCode(keyCode),
                        keyDown: type != .keyUp)!
        e.type = type
        e.flags = flags
        if autorepeat { e.setIntegerValueField(.keyboardEventAutorepeat, value: 1) }
        return e
    }

    @discardableResult
    private func send(keyCode: Int64, _ type: CGEventType,
                      flags: CGEventFlags = [],
                      autorepeat: Bool = false) -> Bool {
        let e = makeEvent(keyCode: keyCode, type: type,
                          flags: flags, autorepeat: autorepeat)
        let passed = controller.handle(type: type, event: e) != nil
        drain()
        return passed
    }

    // MARK: modifier bindings

    func testModifierHotkeyFiresOnPressAndRelease() {
        Config.hotkey = .rightOption
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        send(keyCode: 61, .flagsChanged, flags: [])
        XCTAssertEqual(events, ["down", "up"])
    }

    func testModifierHotkeyIgnoresTheOtherSideOfTheKeyboard() {
        Config.hotkey = .rightOption
        send(keyCode: 58, .flagsChanged, flags: [.maskAlternate])   // left ⌥
        XCTAssertEqual(events, [])
    }

    func testHoldingTheOtherSideDoesNotLatchTheRecordingOpen() {
        // Left ⌥ held down throughout; right ⌥ tapped. Because both set
        // .maskAlternate, the release used to look like "still held" and the
        // recording never ended.
        Config.hotkey = .rightOption
        let left = CGEventFlags(rawValue: 0x20)
        let right = CGEventFlags(rawValue: 0x40)
        send(keyCode: 58, .flagsChanged, flags: [.maskAlternate, left])
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate, left, right])
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate, left])
        XCTAssertEqual(events, ["down", "up"])
    }

    func testModifierHotkeyEventsStillReachTheApp() {
        Config.hotkey = .rightOption
        XCTAssertTrue(send(keyCode: 61, .flagsChanged, flags: [.maskAlternate]),
                      "a modifier press should not be swallowed")
    }

    func testTypingWhileHoldingAModifierHotkeyCancels() {
        Config.hotkey = .rightOption
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        send(keyCode: 2, .keyDown, flags: [.maskAlternate])         // ⌥D
        XCTAssertEqual(events, ["down", "cancel"])
    }

    func testEscapeCancelsAndIsSwallowed() {
        Config.hotkey = .rightOption
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        XCTAssertFalse(send(keyCode: 53, .keyDown), "Esc should be swallowed")
        XCTAssertEqual(events, ["down", "cancel"])
    }

    // MARK: arbitrary-key bindings

    func testRegularKeyHotkeyFiresOnKeyDownAndKeyUp() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])  // ⌃D
        send(keyCode: 2, .keyDown, flags: [.maskControl])
        send(keyCode: 2, .keyUp, flags: [.maskControl])
        XCTAssertEqual(events, ["down", "up"])
    }

    func testRegularKeyHotkeyIsSwallowedSoItDoesNotAlsoType() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        XCTAssertFalse(send(keyCode: 2, .keyDown, flags: [.maskControl]))
        XCTAssertFalse(send(keyCode: 2, .keyUp, flags: [.maskControl]))
    }

    func testAutorepeatDoesNotRestartTheRecording() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        send(keyCode: 2, .keyDown, flags: [.maskControl])
        send(keyCode: 2, .keyDown, flags: [.maskControl], autorepeat: true)
        send(keyCode: 2, .keyDown, flags: [.maskControl], autorepeat: true)
        send(keyCode: 2, .keyUp, flags: [.maskControl])
        XCTAssertEqual(events, ["down", "up"])
    }

    func testExtraModifiersDoNotMatchTheBinding() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        send(keyCode: 2, .keyDown, flags: [.maskControl, .maskShift])   // ⇧⌃D
        XCTAssertEqual(events, [])
    }

    func testMissingModifiersDoNotMatchTheBinding() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        send(keyCode: 2, .keyDown)
        XCTAssertEqual(events, [])
    }

    func testBareFunctionKeyBindingWorks() {
        Config.hotkey = Hotkey(keyCode: 105)   // F13
        send(keyCode: 105, .keyDown)
        send(keyCode: 105, .keyUp)
        XCTAssertEqual(events, ["down", "up"])
    }

    func testTheHotkeysOwnModifiersDoNotCancelIt() {
        // Pressing ⌃ before D arrives as .flagsChanged; it must not be read
        // as "the user reached for a shortcut".
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        send(keyCode: 59, .flagsChanged, flags: [.maskControl])
        send(keyCode: 2, .keyDown, flags: [.maskControl])
        send(keyCode: 59, .flagsChanged, flags: [.maskControl])
        XCTAssertEqual(events, ["down"])
    }

    func testReleasingTheKeyAfterItsModifierStillEndsTheRecording() {
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskControl])
        send(keyCode: 2, .keyDown, flags: [.maskControl])
        send(keyCode: 59, .flagsChanged, flags: [])      // ⌃ released first
        send(keyCode: 2, .keyUp, flags: [])              // then D
        XCTAssertEqual(events, ["down", "up"])
    }

    func testTypingWhileHoldingARegularKeyHotkeyCancels() {
        Config.hotkey = Hotkey(keyCode: 105)   // F13
        send(keyCode: 105, .keyDown)
        send(keyCode: 0, .keyDown)             // A
        XCTAssertEqual(events, ["down", "cancel"])
    }

    func testAModifierBindingWithExtraModifiersFiresRegardlessOfPressOrder() {
        Config.hotkey = Hotkey(keyCode: 61, modifiers: [.maskControl])   // ⌃ Right ⌥
        // ⌥ down first, so the bound key is *not* the last one pressed. Only
        // the bound key's own events say whether it is down, but the verdict
        // has to be recomputed when a required modifier moves too.
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        XCTAssertEqual(events, [], "⌃ isn't down yet")
        send(keyCode: 59, .flagsChanged, flags: [.maskAlternate, .maskControl])
        XCTAssertEqual(events, ["down"])
        send(keyCode: 59, .flagsChanged, flags: [.maskAlternate])
        XCTAssertEqual(events, ["down", "up"], "letting go of ⌃ ends it")
    }

    func testChangingTheBindingMidHoldDoesNotStrandTheRecording() {
        // Recording a new key means pressing one while the old binding may
        // still be down. The branch that would clear the press can stop being
        // reachable, which used to wedge the tap until relaunch.
        Config.hotkey = .rightOption
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        Config.hotkey = Hotkey(keyCode: 2, modifiers: [.maskAlternate])   // ⌥D
        send(keyCode: 61, .flagsChanged, flags: [])
        XCTAssertEqual(events, ["down", "cancel"],
                       "the orphaned press should be dropped, not transcribed")

        // And the tap still works under the new binding.
        send(keyCode: 2, .keyDown, flags: [.maskAlternate])
        send(keyCode: 2, .keyUp, flags: [.maskAlternate])
        XCTAssertEqual(events, ["down", "cancel", "down", "up"])
    }

    func testASuspendedTapIgnoresTheTalkKey() {
        Config.hotkey = .rightOption
        controller.suspended = true
        XCTAssertTrue(send(keyCode: 61, .flagsChanged, flags: [.maskAlternate]),
                      "suspended means inert, not swallowing")
        XCTAssertEqual(events, [])
        controller.suspended = false
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        XCTAssertEqual(events, ["down"])
    }

    private func selfPosted(keyCode: Int64, flags: CGEventFlags) -> CGEvent {
        let e = makeEvent(keyCode: keyCode, type: .keyDown, flags: flags)
        e.setIntegerValueField(.eventSourceUserData,
                               value: HotkeyController.selfPostedTag)
        return e
    }

    func testTheAppsOwnPasteDoesNotCancelARecording() {
        Config.hotkey = .rightOption
        send(keyCode: 61, .flagsChanged, flags: [.maskAlternate])
        let paste = selfPosted(keyCode: 9, flags: [.maskCommand])
        XCTAssertNotNil(controller.handle(type: .keyDown, event: paste))
        drain()
        XCTAssertEqual(events, ["down"], "our own ⌘V is not the user typing")
    }

    func testTheAppsOwnPasteIsNotMistakenForACommandVBinding() {
        // Binding ⌘V would otherwise make the app swallow its own paste, so
        // dictation would quietly stop landing anywhere.
        Config.hotkey = Hotkey(keyCode: 9, modifiers: [.maskCommand])
        let paste = selfPosted(keyCode: 9, flags: [.maskCommand])
        XCTAssertNotNil(controller.handle(type: .keyDown, event: paste),
                        "the paste must reach the app in front")
        drain()
        XCTAssertEqual(events, [])
    }

    func testUnrelatedKeysArePassedThroughWhenIdle() {
        Config.hotkey = .rightOption
        XCTAssertTrue(send(keyCode: 0, .keyDown))
        XCTAssertTrue(send(keyCode: 0, .keyUp))
        XCTAssertEqual(events, [])
    }
}
