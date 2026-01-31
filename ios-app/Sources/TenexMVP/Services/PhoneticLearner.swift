import Foundation

/// Stores and retrieves user corrections for phonetically similar words.
/// Uses Soundex algorithm to detect phonetic similarity.
final class PhoneticLearner {
    private let storageKey = "phonetic_corrections"

    var corrections: [String: String] {
        get { UserDefaults.standard.dictionary(forKey: storageKey) as? [String: String] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: storageKey) }
    }

    /// Checks if two words sound similar using Soundex algorithm
    func isPhoneticallySimilar(_ word1: String, _ word2: String) -> Bool {
        soundex(word1) == soundex(word2)
    }

    /// Records a user correction if the words are phonetically similar
    func recordCorrection(original: String, replacement: String) {
        guard isPhoneticallySimilar(original, replacement) else { return }
        var current = corrections
        current[original.lowercased()] = replacement
        corrections = current
    }

    /// Applies known corrections to a text string
    func applyCorrections(to text: String) -> String {
        var result = text
        for (original, replacement) in corrections {
            // Case-insensitive replacement while preserving word boundaries
            let pattern = "\\b\(NSRegularExpression.escapedPattern(for: original))\\b"
            if let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive) {
                result = regex.stringByReplacingMatches(
                    in: result,
                    options: [],
                    range: NSRange(result.startIndex..., in: result),
                    withTemplate: replacement
                )
            }
        }
        return result
    }

    /// Soundex algorithm implementation for phonetic comparison.
    /// Returns a 4-character code representing how a word sounds.
    private func soundex(_ word: String) -> String {
        let input = word.uppercased().filter { $0.isLetter }
        guard let firstLetter = input.first else { return "" }

        // Soundex character mappings
        let mapping: [Character: Character] = [
            "B": "1", "F": "1", "P": "1", "V": "1",
            "C": "2", "G": "2", "J": "2", "K": "2", "Q": "2", "S": "2", "X": "2", "Z": "2",
            "D": "3", "T": "3",
            "L": "4",
            "M": "5", "N": "5",
            "R": "6"
        ]

        var code = String(firstLetter)
        var previousCode: Character?

        for char in input.dropFirst() {
            guard let charCode = mapping[char] else {
                previousCode = nil
                continue
            }

            // Skip duplicates and adjacent letters with same code
            if charCode != previousCode {
                code.append(charCode)
                previousCode = charCode
            }

            if code.count >= 4 { break }
        }

        // Pad with zeros to ensure 4 characters
        while code.count < 4 {
            code.append("0")
        }

        return String(code.prefix(4))
    }
}
