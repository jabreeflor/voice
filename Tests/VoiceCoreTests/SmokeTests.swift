import XCTest
@testable import VoiceCore

final class SmokeTests: XCTestCase {
    func testPackageBuildsAndLinks() {
        XCTAssertEqual(Config.serverPort, 8178)
    }
}
