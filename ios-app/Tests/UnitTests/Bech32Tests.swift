import XCTest
@testable import TenexMVP

final class Bech32Tests: XCTestCase {

    // Known test vector: fiatjaf's pubkey
    private let knownHex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
    private let knownNpub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6"

    // MARK: - npubToHex

    func testNpubToHexWithValidNpub() {
        let hex = Bech32.npubToHex(knownNpub)
        XCTAssertEqual(hex, knownHex)
    }

    func testNpubToHexWithInvalidPrefix() {
        // nsec prefix should be rejected
        let nsec = "nsec1" + String(knownNpub.dropFirst(5))
        XCTAssertNil(Bech32.npubToHex(nsec))

        // arbitrary prefix
        XCTAssertNil(Bech32.npubToHex("lnbc1qqqqqqqqqq"))
    }

    func testNpubToHexWithInvalidCharacters() {
        // 'b', 'i', 'o' are not in the bech32 charset
        let invalid = "npub1" + String(repeating: "b", count: 58)
        XCTAssertNil(Bech32.npubToHex(invalid))

        // uppercase 'O' (also not in bech32 charset after lowercasing -> 'o')
        let withO = "npub1" + String(repeating: "o", count: 58)
        XCTAssertNil(Bech32.npubToHex(withO))
    }

    func testNpubToHexWithCorruptedDataPart() {
        // Flip a character in the data portion (not the checksum) to corrupt actual key data.
        // The implementation strips the last 6 chars as checksum and decodes the rest,
        // so corrupting a data character should produce a different hex or nil.
        var chars = Array(knownNpub)
        // Index 5 is the first data character after "npub1"
        let dataCharIndex = 5
        let originalChar = chars[dataCharIndex]
        // Pick a different valid bech32 character
        chars[dataCharIndex] = originalChar == "q" ? "p" : "q"
        let corrupted = String(chars)

        let result = Bech32.npubToHex(corrupted)
        if let result {
            XCTAssertNotEqual(result, knownHex, "Corrupted data should not produce the original hex")
        }
        // nil is also acceptable (bit conversion might fail)
    }

    func testNpubToHexWithEmptyString() {
        XCTAssertNil(Bech32.npubToHex(""))
    }

    func testNpubToHexWithJustPrefix() {
        XCTAssertNil(Bech32.npubToHex("npub1"))
    }

    func testNpubToHexWithTooShortData() {
        // "npub1" + only 5 chars (less than 6 for checksum)
        XCTAssertNil(Bech32.npubToHex("npub1qqqqq"))
    }

    // MARK: - hexToNpub

    func testHexToNpubWithValidHex() {
        let npub = Bech32.hexToNpub(knownHex)
        XCTAssertEqual(npub, knownNpub)
    }

    func testHexToNpubRoundTrip() {
        let npub = Bech32.hexToNpub(knownHex)
        XCTAssertNotNil(npub)
        let hex = Bech32.npubToHex(npub!)
        XCTAssertEqual(hex, knownHex)
    }

    func testHexToNpubWithEmptyString() {
        XCTAssertNil(Bech32.hexToNpub(""))
    }

    func testHexToNpubWithOddLengthHex() {
        // 63 hex chars (odd length, not 64)
        let oddHex = String(repeating: "a", count: 63)
        XCTAssertNil(Bech32.hexToNpub(oddHex))
    }

    func testHexToNpubWithTooShortHex() {
        XCTAssertNil(Bech32.hexToNpub("abcdef"))
    }

    func testHexToNpubWithTooLongHex() {
        let longHex = String(repeating: "a", count: 66)
        XCTAssertNil(Bech32.hexToNpub(longHex))
    }

    func testHexToNpubWithInvalidHexCharacters() {
        // 'g' is not a valid hex character
        let invalidHex = String(repeating: "g", count: 64)
        XCTAssertNil(Bech32.hexToNpub(invalidHex))
    }

    // MARK: - Edge Cases

    func testAllZerosHex() {
        let zeroHex = String(repeating: "0", count: 64)
        let npub = Bech32.hexToNpub(zeroHex)
        XCTAssertNotNil(npub)
        XCTAssertTrue(npub!.hasPrefix("npub1"))

        // Round-trip
        let backToHex = Bech32.npubToHex(npub!)
        XCTAssertEqual(backToHex, zeroHex)
    }

    func testAllFsHex() {
        let maxHex = String(repeating: "f", count: 64)
        let npub = Bech32.hexToNpub(maxHex)
        XCTAssertNotNil(npub)
        XCTAssertTrue(npub!.hasPrefix("npub1"))

        // Round-trip
        let backToHex = Bech32.npubToHex(npub!)
        XCTAssertEqual(backToHex, maxHex)
    }

    func testCaseSensitivityNpubToHex() {
        // Bech32 is case-insensitive; uppercase npub should work
        let uppercased = knownNpub.uppercased()
        let hex = Bech32.npubToHex(uppercased)
        XCTAssertEqual(hex, knownHex)
    }

    func testCaseSensitivityMixedCase() {
        // Mixed case should also work since the implementation lowercases
        var mixed = Array(knownNpub)
        for i in stride(from: 0, to: mixed.count, by: 2) {
            mixed[i] = Character(mixed[i].uppercased())
        }
        let mixedStr = String(mixed)
        let hex = Bech32.npubToHex(mixedStr)
        XCTAssertEqual(hex, knownHex)
    }

    func testHexToNpubAlwaysProducesLowercase() {
        // Even with uppercase hex input, output should be lowercase npub
        let upperHex = knownHex.uppercased()
        let npub = Bech32.hexToNpub(upperHex)
        // hexToNpub checks hex.count == 64 and UInt8 parsing; uppercase hex is valid
        XCTAssertNotNil(npub)
        XCTAssertEqual(npub, npub!.lowercased())
    }

    func testMultipleRoundTrips() {
        // Test several different hex values round-trip correctly
        let hexValues = [
            "0000000000000000000000000000000000000000000000000000000000000001",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            knownHex,
        ]

        for hex in hexValues {
            let npub = Bech32.hexToNpub(hex)
            XCTAssertNotNil(npub, "hexToNpub should succeed for \(hex)")
            let roundTripped = Bech32.npubToHex(npub!)
            XCTAssertEqual(roundTripped, hex, "Round-trip failed for \(hex)")
        }
    }

    func testHexToNpubOutputStartsWithNpub1() {
        let npub = Bech32.hexToNpub(knownHex)
        XCTAssertNotNil(npub)
        XCTAssertTrue(npub!.hasPrefix("npub1"))
    }

    func testNpubToHexOutputIs64Characters() {
        let hex = Bech32.npubToHex(knownNpub)
        XCTAssertNotNil(hex)
        XCTAssertEqual(hex!.count, 64)
    }
}
