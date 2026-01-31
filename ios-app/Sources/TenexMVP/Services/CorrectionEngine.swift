import Foundation
import FoundationModels

/// Uses on-device Foundation Models to analyze transcriptions for errors
/// and suggest corrections based on phonetic misheard words.
actor CorrectionEngine {
    @Generable
    struct CorrectionSuggestion {
        @Guide(description: "Whether correction is needed")
        let needsCorrection: Bool
        @Guide(description: "The corrected text if correction needed")
        let correctedText: String
        @Guide(description: "Brief explanation of what was corrected")
        let explanation: String
    }

    /// Analyzes a transcription for potential errors and suggests corrections.
    /// - Parameters:
    ///   - text: The transcribed text to analyze
    ///   - knownCorrections: Dictionary of known user corrections to apply
    /// - Returns: A correction suggestion if errors were detected, nil otherwise
    func analyzeTranscription(
        _ text: String,
        knownCorrections: [String: String]
    ) async throws -> CorrectionSuggestion? {
        let correctionsContext = knownCorrections.isEmpty
            ? "No known corrections yet."
            : knownCorrections.map { "\($0.key) → \($0.value)" }.joined(separator: "\n")

        let instructions = Instructions("""
            You analyze speech-to-text transcriptions for errors.
            Focus on:
            - Phonetic misheard words (e.g., "Asians" instead of "Agents")
            - Nonsensical phrases that could be misheard words
            - Technical terms that may have been transcribed incorrectly
            - Common speech recognition mistakes

            Known user corrections to apply:
            \(correctionsContext)

            If the text looks correct, set needsCorrection to false.
            Only suggest corrections when you're confident there's an error.
            """)

        let session = LanguageModelSession(instructions: instructions)
        let result = try await session.respond(
            to: "Analyze this transcription: \(text)",
            generating: CorrectionSuggestion.self
        )

        return result.content.needsCorrection ? result.content : nil
    }
}

