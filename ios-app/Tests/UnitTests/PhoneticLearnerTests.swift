import XCTest
@testable import TenexMVP

final class PhoneticLearnerTests: XCTestCase {
    private var learner: PhoneticLearner!

    override func setUp() {
        super.setUp()
        UserDefaults.standard.removeObject(forKey: "phonetic_corrections")
        learner = PhoneticLearner()
    }

    override func tearDown() {
        UserDefaults.standard.removeObject(forKey: "phonetic_corrections")
        learner = nil
        super.tearDown()
    }

    // MARK: - Soundex (tested via isPhoneticallySimilar)

    func testKnownSoundexValues() {
        // Robert = R163, Rupert = R163 -> same
        XCTAssertTrue(learner.isPhoneticallySimilar("Robert", "Rupert"))

        // Smith = S530, Smyth = S530 -> same
        XCTAssertTrue(learner.isPhoneticallySimilar("Smith", "Smyth"))

        // Ashcraft and Ashcroft should both map to A261
        XCTAssertTrue(learner.isPhoneticallySimilar("Ashcraft", "Ashcroft"))

        // Tymczak = T522, Pfister = P236 -> different
        XCTAssertFalse(learner.isPhoneticallySimilar("Tymczak", "Pfister"))
    }

    func testSoundexRobertAndSmithAreNotSimilar() {
        // Robert = R163, Smith = S530 -> different
        XCTAssertFalse(learner.isPhoneticallySimilar("Robert", "Smith"))
    }

    func testSoundexEmptyStrings() {
        // Two empty strings produce the same soundex ("") so they are "similar"
        XCTAssertTrue(learner.isPhoneticallySimilar("", ""))
    }

    func testSoundexEmptyVsNonEmpty() {
        XCTAssertFalse(learner.isPhoneticallySimilar("", "Robert"))
        XCTAssertFalse(learner.isPhoneticallySimilar("Smith", ""))
    }

    func testSoundexSingleCharacter() {
        // Single letter: A -> A000, A -> A000
        XCTAssertTrue(learner.isPhoneticallySimilar("A", "A"))
        // Different single letters with different codes
        XCTAssertFalse(learner.isPhoneticallySimilar("A", "B"))
        // Single letter padded to 4 chars
        XCTAssertTrue(learner.isPhoneticallySimilar("R", "R"))
    }

    func testSoundexAllVowels() {
        // Vowels (A, E, I, O, U) are not in the mapping, so they are skipped after the first letter.
        // "AEI" -> A000, "AOU" -> A000 (all have A as first letter, rest are vowels = no codes)
        XCTAssertTrue(learner.isPhoneticallySimilar("AEI", "AOU"))
        // "AEIOU" -> A000, "A" -> A000
        XCTAssertTrue(learner.isPhoneticallySimilar("AEIOU", "A"))
    }

    func testSoundexAllVowelsDifferentFirstLetter() {
        // "EAIO" -> E000, "IOUA" -> I000
        XCTAssertFalse(learner.isPhoneticallySimilar("EAIO", "IOUA"))
    }

    func testSoundexWithNumbers() {
        // Numbers are stripped by filter { $0.isLetter }, so "R2D2" -> letters "RD" -> R300
        // "RD" -> R300, "Red" -> R300 (E is vowel, D=3 -> R300)
        XCTAssertTrue(learner.isPhoneticallySimilar("R2D2", "RD"))
    }

    func testSoundexCaseInsensitivity() {
        XCTAssertTrue(learner.isPhoneticallySimilar("robert", "ROBERT"))
        XCTAssertTrue(learner.isPhoneticallySimilar("smith", "SMITH"))
        XCTAssertTrue(learner.isPhoneticallySimilar("Smith", "sMiTh"))
    }

    func testSoundexAdjacentDuplicateCodesAreCollapsed() {
        // "BB" -> first letter B, second B maps to 1, but it's same as what B would produce
        // Actually: first letter is B, rest "B" maps to 1, previous is nil so code = B1, padded = B100
        // "BP" -> first letter B, P maps to 1, previous nil -> 1, code = B100
        XCTAssertTrue(learner.isPhoneticallySimilar("BB", "BP"))
    }

    func testSoundexTruncatesAtFourCharacters() {
        // "Washington" and "Washingtn" should produce the same 4-char code
        XCTAssertTrue(learner.isPhoneticallySimilar("Washington", "Washingtn"))
    }

    // MARK: - isPhoneticallySimilar

    func testSimilarSoundingNames() {
        XCTAssertTrue(learner.isPhoneticallySimilar("Robert", "Rupert"))
        XCTAssertTrue(learner.isPhoneticallySimilar("Smith", "Smyth"))
        // Catherine (C365) vs Kathryn (K365) differ in first letter,
        // so Soundex correctly reports them as different codes
        XCTAssertFalse(learner.isPhoneticallySimilar("Catherine", "Kathryn"))
        // But same first letter variants do match
        XCTAssertTrue(learner.isPhoneticallySimilar("Catherine", "Catharine"))
    }

    func testDissimilarWords() {
        XCTAssertFalse(learner.isPhoneticallySimilar("Apple", "Banana"))
        XCTAssertFalse(learner.isPhoneticallySimilar("Hello", "World"))
        XCTAssertFalse(learner.isPhoneticallySimilar("Cat", "Dog"))
    }

    func testIsPhoneticallySimilarCaseInsensitive() {
        XCTAssertTrue(learner.isPhoneticallySimilar("robert", "RUPERT"))
        XCTAssertTrue(learner.isPhoneticallySimilar("SMITH", "smyth"))
    }

    func testIsPhoneticallySimilarBothEmpty() {
        XCTAssertTrue(learner.isPhoneticallySimilar("", ""))
    }

    func testIsPhoneticallySimilarOneEmpty() {
        XCTAssertFalse(learner.isPhoneticallySimilar("", "Word"))
        XCTAssertFalse(learner.isPhoneticallySimilar("Word", ""))
    }

    // MARK: - recordCorrection + applyCorrections

    func testRecordAndApplyRoundTrip() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        let result = learner.applyCorrections(to: "Hello Rupert")
        XCTAssertEqual(result, "Hello Robert")
    }

    func testRecordCorrectionIgnoresDissimilarWords() {
        learner.recordCorrection(original: "Apple", replacement: "Banana")
        // Should not record since they are not phonetically similar
        XCTAssertTrue(learner.corrections.isEmpty)
        let result = learner.applyCorrections(to: "I like Apple")
        XCTAssertEqual(result, "I like Apple")
    }

    func testApplyCorrectionRespectsWordBoundaries() {
        learner.recordCorrection(original: "Smyth", replacement: "Smith")
        // "Smyth" should be replaced, but "Smythe" should not (word boundary)
        let result = learner.applyCorrections(to: "Dr. Smyth and Smythe")
        XCTAssertEqual(result, "Dr. Smith and Smythe")
    }

    func testApplyCorrectionIsCaseInsensitive() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        // recordCorrection lowercases the key, so "rupert" is stored
        let result = learner.applyCorrections(to: "Ask RUPERT about it")
        XCTAssertEqual(result, "Ask Robert about it")
    }

    func testMultipleCorrections() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        learner.recordCorrection(original: "Smyth", replacement: "Smith")
        let result = learner.applyCorrections(to: "Rupert and Smyth")
        XCTAssertEqual(result, "Robert and Smith")
    }

    func testRecordCorrectionOverwritesPreviousCorrection() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        learner.recordCorrection(original: "Rupert", replacement: "Rubert")
        // The second recording should overwrite the first
        // "Rupert" and "Rubert" are both R163
        XCTAssertEqual(learner.corrections["rupert"], "Rubert")
    }

    func testApplyCorrectionsWithNoCorrections() {
        let result = learner.applyCorrections(to: "Nothing to change here")
        XCTAssertEqual(result, "Nothing to change here")
    }

    func testApplyCorrectionsEmptyText() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        let result = learner.applyCorrections(to: "")
        XCTAssertEqual(result, "")
    }

    func testCorrectionPersistsInUserDefaults() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")

        // Create a new learner instance; corrections should be loaded from UserDefaults
        let newLearner = PhoneticLearner()
        XCTAssertEqual(newLearner.corrections["rupert"], "Robert")

        let result = newLearner.applyCorrections(to: "Hello Rupert")
        XCTAssertEqual(result, "Hello Robert")
    }

    // MARK: - Edge cases

    func testNonAlphaCharactersAreStripped() {
        // "O'Brien" -> letters "OBrien" -> soundex O165
        // "Obrien" -> soundex O165
        XCTAssertTrue(learner.isPhoneticallySimilar("O'Brien", "Obrien"))
    }

    func testHyphensAndSpacesAreStripped() {
        // "Mary-Jane" -> letters "MaryJane" -> M625
        // "Maryjane" -> M625
        XCTAssertTrue(learner.isPhoneticallySimilar("Mary-Jane", "Maryjane"))
    }

    func testUnicodeAccentedCharacters() {
        // Characters like accented letters: isLetter returns true for them,
        // but they won't be in the mapping so they act like vowels (separators).
        // "Rene" -> R500, "Rene" -> R500
        XCTAssertTrue(learner.isPhoneticallySimilar("Rene", "Rene"))
    }

    func testVeryLongStringSoundexStillWorks() {
        // Soundex only uses the first letter + up to 3 consonant codes,
        // so even very long strings should work fine
        let longA = "R" + String(repeating: "a", count: 10000) + "bert"
        let longB = "R" + String(repeating: "e", count: 10000) + "bert"
        // Both start with R, then many vowels (skipped), then "bert" -> B=1, R=6, T=3
        // Code = R163 for both
        XCTAssertTrue(learner.isPhoneticallySimilar(longA, longB))
    }

    func testVeryLongStringDoesNotCrash() {
        let longWord = String(repeating: "ABCDEFG", count: 5000)
        // Just verifying it doesn't hang or crash
        _ = learner.isPhoneticallySimilar(longWord, longWord)
    }

    func testOnlyNumbers() {
        // "12345" -> no letters -> empty soundex
        XCTAssertTrue(learner.isPhoneticallySimilar("12345", "67890"))
    }

    func testSpecialCharactersOnly() {
        // No letters -> empty soundex for both
        XCTAssertTrue(learner.isPhoneticallySimilar("@#$%", "!&*()"))
    }

    func testRecordCorrectionWithSpecialCharsInOriginal() {
        // "O'Brien" and "Obrien" are phonetically similar
        learner.recordCorrection(original: "Obrien", replacement: "O'Brien")
        // The stored key is "obrien" (lowercased)
        XCTAssertEqual(learner.corrections["obrien"], "O'Brien")
        let result = learner.applyCorrections(to: "Ask Obrien")
        XCTAssertEqual(result, "Ask O'Brien")
    }

    func testMultipleOccurrencesInText() {
        learner.recordCorrection(original: "Smyth", replacement: "Smith")
        let result = learner.applyCorrections(to: "Smyth met Smyth at the Smyth house")
        XCTAssertEqual(result, "Smith met Smith at the Smith house")
    }

    func testWordBoundaryDoesNotMatchSubstring() {
        learner.recordCorrection(original: "Rupert", replacement: "Robert")
        // "Rupertson" contains "Rupert" but it's not a whole word
        let result = learner.applyCorrections(to: "Hello Rupertson")
        XCTAssertEqual(result, "Hello Rupertson")
    }
}
