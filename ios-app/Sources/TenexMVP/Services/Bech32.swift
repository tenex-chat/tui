import Foundation

/// Bech32 encoding/decoding utilities for Nostr keys.
/// Implements the minimal subset needed for npub <-> hex conversion.
enum Bech32 {
    // Bech32 character set
    private static let charset = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    private static let charsetMap: [Character: UInt8] = {
        var map = [Character: UInt8]()
        for (i, c) in charset.enumerated() {
            map[c] = UInt8(i)
        }
        return map
    }()

    /// Convert a bech32-encoded nostr key to hex.
    /// Works for both npub and nsec prefixes.
    private static func bech32ToHex(_ input: String, expectedPrefix: String) -> String? {
        guard input.lowercased().hasPrefix(expectedPrefix) else {
            return nil
        }

        let data = input.lowercased()
        guard let sepIndex = data.lastIndex(of: "1") else {
            return nil
        }

        let dataPartStart = data.index(after: sepIndex)
        let dataPart = String(data[dataPartStart...])

        // Decode bech32 data part
        var values = [UInt8]()
        for char in dataPart {
            guard let value = charsetMap[char] else {
                return nil
            }
            values.append(value)
        }

        // Remove checksum (last 6 characters)
        guard values.count > 6 else {
            return nil
        }
        let dataValues = Array(values.dropLast(6))

        // Convert 5-bit values to 8-bit bytes
        guard let bytes = convertBits(data: dataValues, fromBits: 5, toBits: 8, pad: false) else {
            return nil
        }

        // Convert bytes to hex string
        return bytes.map { String(format: "%02x", $0) }.joined()
    }

    /// Convert an npub (bech32-encoded) to a hex pubkey string.
    /// Returns nil if the input is not a valid npub.
    static func npubToHex(_ npub: String) -> String? {
        bech32ToHex(npub, expectedPrefix: "npub1")
    }

    /// Convert an nsec (bech32-encoded) to a hex private key string.
    /// Returns nil if the input is not a valid nsec.
    static func nsecToHex(_ nsec: String) -> String? {
        bech32ToHex(nsec, expectedPrefix: "nsec1")
    }

    /// Convert hex pubkey to npub (bech32-encoded).
    /// Returns nil if the input is not a valid 32-byte hex string.
    static func hexToNpub(_ hex: String) -> String? {
        guard let bytes = hexToBytes(hex, expectedByteCount: 32) else { return nil }
        return encodeBech32(hrp: "npub", payload: bytes)
    }

    /// Convert hex event ID to nevent (bech32-encoded, NIP-19).
    /// Encodes as TLV with required type 0 (event id).
    static func hexEventIdToNevent(_ eventIdHex: String) -> String? {
        guard let eventIdBytes = hexToBytes(eventIdHex, expectedByteCount: 32) else {
            return nil
        }

        var tlv = [UInt8]()
        tlv.append(0) // type: event id
        tlv.append(UInt8(eventIdBytes.count))
        tlv.append(contentsOf: eventIdBytes)

        return encodeBech32(hrp: "nevent", payload: tlv)
    }

    // MARK: - Private Helpers

    private static func hexToBytes(_ hex: String, expectedByteCount: Int? = nil) -> [UInt8]? {
        guard hex.count.isMultiple(of: 2) else {
            return nil
        }
        if let expectedByteCount, hex.count != expectedByteCount * 2 {
            return nil
        }

        var bytes = [UInt8]()
        var index = hex.startIndex

        while index < hex.endIndex {
            let nextIndex = hex.index(index, offsetBy: 2)
            guard let byte = UInt8(hex[index..<nextIndex], radix: 16) else {
                return nil
            }
            bytes.append(byte)
            index = nextIndex
        }

        return bytes
    }

    private static func encodeBech32(hrp: String, payload: [UInt8]) -> String? {
        guard let values = convertBits(data: payload, fromBits: 8, toBits: 5, pad: true) else {
            return nil
        }

        let checksum = createChecksum(hrp: hrp, values: values)
        let combined = values + checksum
        let dataString = combined.map { charset[charset.index(charset.startIndex, offsetBy: Int($0))] }
        return "\(hrp)1\(String(dataString))"
    }

    private static func convertBits(data: [UInt8], fromBits: Int, toBits: Int, pad: Bool) -> [UInt8]? {
        var acc = 0
        var bits = 0
        var result = [UInt8]()
        let maxv = (1 << toBits) - 1

        for value in data {
            if Int(value) >> fromBits != 0 {
                return nil
            }
            acc = (acc << fromBits) | Int(value)
            bits += fromBits
            while bits >= toBits {
                bits -= toBits
                result.append(UInt8((acc >> bits) & maxv))
            }
        }

        if pad {
            if bits > 0 {
                result.append(UInt8((acc << (toBits - bits)) & maxv))
            }
        } else if bits >= fromBits || ((acc << (toBits - bits)) & maxv) != 0 {
            return nil
        }

        return result
    }

    private static func polymod(_ values: [Int]) -> Int {
        let generator = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
        var chk = 1
        for value in values {
            let top = chk >> 25
            chk = ((chk & 0x1ffffff) << 5) ^ value
            for i in 0..<5 {
                if (top >> i) & 1 != 0 {
                    chk ^= generator[i]
                }
            }
        }
        return chk
    }

    private static func hrpExpand(_ hrp: String) -> [Int] {
        var result = [Int]()
        for c in hrp.unicodeScalars {
            result.append(Int(c.value) >> 5)
        }
        result.append(0)
        for c in hrp.unicodeScalars {
            result.append(Int(c.value) & 31)
        }
        return result
    }

    private static func createChecksum(hrp: String, values: [UInt8]) -> [UInt8] {
        let valuesInt = values.map { Int($0) }
        let polymodInput = hrpExpand(hrp) + valuesInt + [0, 0, 0, 0, 0, 0]
        let polymodResult = polymod(polymodInput) ^ 1
        var checksum = [UInt8]()
        for i in 0..<6 {
            checksum.append(UInt8((polymodResult >> (5 * (5 - i))) & 31))
        }
        return checksum
    }
}
