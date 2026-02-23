import XCTest
@testable import TenexMVP

final class PushNotificationBackendSelectionTests: XCTestCase {

    func testNormalizedBackendPubkeysTrimsAndDeduplicates() {
        let input = [
            "  backend-b  ",
            "backend-a",
            "",
            "backend-b",
            "  "
        ]

        let normalized = TenexCoreManager.normalizedBackendPubkeys(input)

        XCTAssertEqual(normalized, ["backend-a", "backend-b"])
    }

    func testNormalizedBackendPubkeysReturnsEmptyForOnlyBlankValues() {
        let input = ["", "   ", "\n\t"]

        let normalized = TenexCoreManager.normalizedBackendPubkeys(input)

        XCTAssertTrue(normalized.isEmpty)
    }
}
