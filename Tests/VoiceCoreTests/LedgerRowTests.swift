import AppKit
import XCTest
@testable import VoiceCore

final class LedgerRowTests: XCTestCase {

    func testRowHasNoCopyButton() {
        let row = LedgerRow(time: "9:38 PM", text: "hello from voice")
        XCTAssertTrue(row.subviews.contains(where: { $0 is NSTextField }))
        XCTAssertFalse(row.subviews.contains(where: { $0 is NSButton }),
                       "dictation rows should not show a Copy capsule")
    }

    func testRowIsExposedAsAButtonForAccessibility() {
        let row = LedgerRow(time: "9:38 PM", text: "hello from voice")
        XCTAssertEqual(row.accessibilityRole(), .button)
        XCTAssertEqual(row.accessibilityLabel(), "Copy dictation")
    }

    func testCopyWritesTheDictationToThePasteboard() {
        let pb = NSPasteboard.general
        let previous = pb.string(forType: .string)
        defer {
            pb.clearContents()
            if let previous { pb.setString(previous, forType: .string) }
        }

        let row = LedgerRow(time: "9:38 PM", text: "hello from voice")
        row.copyToClipboard()
        XCTAssertEqual(pb.string(forType: .string), "hello from voice")
        XCTAssertTrue(row.subviews.contains(where: { $0 is GreenConfetti }),
                      "green confetti should burst on the Copied hint")
    }

    func testAccessibilityPressCopies() {
        let pb = NSPasteboard.general
        let previous = pb.string(forType: .string)
        defer {
            pb.clearContents()
            if let previous { pb.setString(previous, forType: .string) }
        }

        let row = LedgerRow(time: "9:27 PM", text: "wait wait wait")
        XCTAssertTrue(row.accessibilityPerformPress())
        XCTAssertEqual(pb.string(forType: .string), "wait wait wait")
    }
}
