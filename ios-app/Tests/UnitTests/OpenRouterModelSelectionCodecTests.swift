import XCTest
@testable import TenexMVP

final class OpenRouterModelSelectionCodecTests: XCTestCase {

    // MARK: - decodeSelectedModelIds

    func testDecodeValidV1EncodedData() {
        let encoded = "tenex:openrouter_models:v1:[\"openai/gpt-4\",\"anthropic/claude-3\"]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["openai/gpt-4", "anthropic/claude-3"])
    }

    func testDecodeV1WithSingleElementArray() {
        let encoded = "tenex:openrouter_models:v1:[\"openai/gpt-4\"]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["openai/gpt-4"])
    }

    func testDecodeV1WithEmptyArray() {
        let encoded = "tenex:openrouter_models:v1:[]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, [])
    }

    func testDecodeV1FiltersEmptyStringsAndTrimsWhitespace() {
        let encoded = "tenex:openrouter_models:v1:[\" openai/gpt-4 \",\"\",\"  \"]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["openai/gpt-4"])
    }

    func testDecodeLegacyPlainModelId() {
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: "openai/gpt-4")
        XCTAssertEqual(result, ["openai/gpt-4"])
    }

    func testDecodeLegacyCommaSeparatedTreatedAsLiteralModelId() {
        // The codec does NOT split on commas for legacy values; the whole
        // trimmed string becomes a single model ID.
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: "openai/gpt-4,anthropic/claude-3")
        XCTAssertEqual(result, ["openai/gpt-4,anthropic/claude-3"])
    }

    func testDecodeEmptyString() {
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: "")
        XCTAssertEqual(result, [])
    }

    func testDecodeWhitespaceOnlyString() {
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: "   \n\t  ")
        XCTAssertEqual(result, [])
    }

    func testDecodeNil() {
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: nil)
        XCTAssertEqual(result, [])
    }

    func testDecodeMalformedJSONAfterPrefix() {
        // Malformed JSON after the v1 prefix -- falls through to returning
        // the entire trimmed string as a single-element set.
        let malformed = "tenex:openrouter_models:v1:{not json array}"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: malformed)
        XCTAssertEqual(result, [malformed])
    }

    func testDecodePrefixOnlyNoPayload() {
        let prefixOnly = "tenex:openrouter_models:v1:"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: prefixOnly)
        // Empty data after prefix is not valid JSON, falls through to
        // returning the whole string as a model ID.
        XCTAssertEqual(result, [prefixOnly])
    }

    func testDecodeTrimsOuterWhitespace() {
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: "  openai/gpt-4  ")
        XCTAssertEqual(result, ["openai/gpt-4"])
    }

    func testDecodeV1WithDuplicateModelIds() {
        let encoded = "tenex:openrouter_models:v1:[\"openai/gpt-4\",\"openai/gpt-4\"]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["openai/gpt-4"])
    }

    func testDecodeRoleSelectionsFromValidRoleEncoding() {
        let encoded = "tenex:openrouter_roles:v1:{\"agent_prompt_rewrite\":\"anthropic/claude-3\",\"audio_notifications\":\"openai/gpt-4\",\"response_prediction\":\"google/gemini-pro\"}"
        let result = OpenRouterModelSelectionCodec.decodeRoleSelections(from: encoded)
        XCTAssertEqual(result[.audioNotifications], "openai/gpt-4")
        XCTAssertEqual(result[.agentPromptRewrite], "anthropic/claude-3")
        XCTAssertEqual(result[.responsePrediction], "google/gemini-pro")
    }

    func testDecodeRoleSelectionsMigratesLegacySingleModelToAllRoles() {
        let result = OpenRouterModelSelectionCodec.decodeRoleSelections(from: "openai/gpt-4")
        XCTAssertEqual(result[.audioNotifications], "openai/gpt-4")
        XCTAssertEqual(result[.agentPromptRewrite], "openai/gpt-4")
        XCTAssertEqual(result[.responsePrediction], "openai/gpt-4")
    }

    func testDecodeRoleSelectionsMigratesLegacyMultiModelToPreferredModel() {
        let legacy = OpenRouterModelSelectionCodec.encodeSelectedModelIds([
            "openai/gpt-4",
            "anthropic/claude-3"
        ])
        let result = OpenRouterModelSelectionCodec.decodeRoleSelections(from: legacy)
        XCTAssertEqual(result[.audioNotifications], "anthropic/claude-3")
        XCTAssertEqual(result[.agentPromptRewrite], "anthropic/claude-3")
        XCTAssertEqual(result[.responsePrediction], "anthropic/claude-3")
    }

    func testDecodeSelectedModelIdsFromRoleEncodingReturnsSelectedRoleValues() {
        let encoded = "tenex:openrouter_roles:v1:{\"audio_notifications\":\"openai/gpt-4\",\"response_prediction\":\"openai/gpt-4\",\"agent_prompt_rewrite\":\"anthropic/claude-3\"}"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["openai/gpt-4", "anthropic/claude-3"])
    }

    // MARK: - encodeSelectedModelIds

    func testEncodeEmptySet() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds([])
        XCTAssertNil(result)
    }

    func testEncodeSingleModel() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["openai/gpt-4"])
        XCTAssertEqual(result, "openai/gpt-4")
    }

    func testEncodeSingleModelDoesNotAddPrefix() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["anthropic/claude-3"])!
        XCTAssertFalse(result.hasPrefix(OpenRouterModelSelectionCodec.multiModelPrefix))
    }

    func testEncodeMultipleModels() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["openai/gpt-4", "anthropic/claude-3"])!
        XCTAssertTrue(result.hasPrefix(OpenRouterModelSelectionCodec.multiModelPrefix))
        // The JSON payload should contain a sorted array.
        let payload = String(result.dropFirst(OpenRouterModelSelectionCodec.multiModelPrefix.count))
        let decoded = try! JSONSerialization.jsonObject(with: payload.data(using: .utf8)!) as! [String]
        XCTAssertEqual(decoded, ["anthropic/claude-3", "openai/gpt-4"])
    }

    func testEncodeFiltersEmptyAndWhitespaceOnlyIds() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["openai/gpt-4", "", "  "])
        XCTAssertEqual(result, "openai/gpt-4")
    }

    func testEncodeTrimsWhitespace() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["  openai/gpt-4  "])
        XCTAssertEqual(result, "openai/gpt-4")
    }

    func testEncodeSetOfOnlyEmptyStringsReturnsNil() {
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["", "  ", "\n"])
        XCTAssertNil(result)
    }

    func testEncodeMultipleModelsProducesSortedJSON() {
        let models: Set<String> = ["z-model", "a-model", "m-model"]
        let result = OpenRouterModelSelectionCodec.encodeSelectedModelIds(models)!
        let payload = String(result.dropFirst(OpenRouterModelSelectionCodec.multiModelPrefix.count))
        let decoded = try! JSONSerialization.jsonObject(with: payload.data(using: .utf8)!) as! [String]
        XCTAssertEqual(decoded, ["a-model", "m-model", "z-model"])
    }

    func testEncodeRoleSelections() {
        let result = OpenRouterModelSelectionCodec.encodeRoleSelections([
            .audioNotifications: "openai/gpt-4",
            .agentPromptRewrite: "anthropic/claude-3",
            .responsePrediction: "google/gemini-pro"
        ])!
        XCTAssertTrue(result.hasPrefix(OpenRouterModelSelectionCodec.roleModelPrefix))

        let payload = String(result.dropFirst(OpenRouterModelSelectionCodec.roleModelPrefix.count))
        let decoded = try! JSONSerialization.jsonObject(with: payload.data(using: .utf8)!) as! [String: String]
        XCTAssertEqual(decoded["audio_notifications"], "openai/gpt-4")
        XCTAssertEqual(decoded["agent_prompt_rewrite"], "anthropic/claude-3")
        XCTAssertEqual(decoded["response_prediction"], "google/gemini-pro")
    }

    func testEncodeRoleSelectionsFiltersEmptyValues() {
        let result = OpenRouterModelSelectionCodec.encodeRoleSelections([
            .audioNotifications: "  openai/gpt-4  ",
            .agentPromptRewrite: " ",
            .responsePrediction: ""
        ])!
        let payload = String(result.dropFirst(OpenRouterModelSelectionCodec.roleModelPrefix.count))
        let decoded = try! JSONSerialization.jsonObject(with: payload.data(using: .utf8)!) as! [String: String]
        XCTAssertEqual(decoded, ["audio_notifications": "openai/gpt-4"])
    }

    func testEncodeEmptyRoleSelectionsReturnsNil() {
        let result = OpenRouterModelSelectionCodec.encodeRoleSelections([:])
        XCTAssertNil(result)
    }

    // MARK: - Round-trip encode/decode

    func testRoundTripSingleModel() {
        let original: Set<String> = ["openai/gpt-4"]
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(original)
        let decoded = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(decoded, original)
    }

    func testRoundTripMultipleModels() {
        let original: Set<String> = ["openai/gpt-4", "anthropic/claude-3", "google/gemini-pro"]
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(original)
        let decoded = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(decoded, original)
    }

    func testRoundTripEmptySet() {
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds([])
        let decoded = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(decoded, [])
    }

    func testRoundTripNormalizesWhitespace() {
        let original: Set<String> = ["  openai/gpt-4  ", " anthropic/claude-3 "]
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(original)
        let decoded = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(decoded, ["openai/gpt-4", "anthropic/claude-3"])
    }

    // MARK: - preferredModel

    func testPreferredModelFromValidMultiModelEncoding() {
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["openai/gpt-4", "anthropic/claude-3"])
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: encoded)
        // sorted().first -> "anthropic/claude-3" comes before "openai/gpt-4"
        XCTAssertEqual(preferred, "anthropic/claude-3")
    }

    func testPreferredModelFromSingleModel() {
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: "openai/gpt-4")
        XCTAssertEqual(preferred, "openai/gpt-4")
    }

    func testPreferredModelFromNil() {
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: nil)
        XCTAssertNil(preferred)
    }

    func testPreferredModelFromEmptyString() {
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: "")
        XCTAssertNil(preferred)
    }

    func testPreferredModelFromWhitespaceOnly() {
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: "   ")
        XCTAssertNil(preferred)
    }

    // MARK: - Edge cases: special characters in model IDs

    func testRoundTripWithSpecialCharactersInModelIds() {
        let original: Set<String> = ["vendor/model:latest", "org/model-v2.1", "provider/model@beta"]
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(original)
        let decoded = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(decoded, original)
    }

    func testDecodeV1WithUnicodeModelIds() {
        let encoded = "tenex:openrouter_models:v1:[\"vendor/\u{00E9}model\",\"org/\u{00FC}ber\"]"
        let result = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: encoded)
        XCTAssertEqual(result, ["vendor/\u{00E9}model", "org/\u{00FC}ber"])
    }

    func testPreferredModelPicksLexicographicallyFirst() {
        let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(["z-last", "a-first", "m-middle"])
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: encoded)
        XCTAssertEqual(preferred, "a-first")
    }

    func testSelectedModelForRoleFromRoleEncoding() {
        let encoded = OpenRouterModelSelectionCodec.encodeRoleSelections([
            .audioNotifications: "openai/gpt-4",
            .agentPromptRewrite: "anthropic/claude-3"
        ])

        XCTAssertEqual(
            OpenRouterModelSelectionCodec.selectedModel(for: .agentPromptRewrite, from: encoded),
            "anthropic/claude-3"
        )
        XCTAssertNil(OpenRouterModelSelectionCodec.selectedModel(for: .responsePrediction, from: encoded))
    }

    func testPreferredModelFromRoleEncodingUsesRoleOrder() {
        let encoded = OpenRouterModelSelectionCodec.encodeRoleSelections([
            .responsePrediction: "google/gemini-pro",
            .agentPromptRewrite: "anthropic/claude-3"
        ])
        let preferred = OpenRouterModelSelectionCodec.preferredModel(from: encoded)
        XCTAssertEqual(preferred, "anthropic/claude-3")
    }
}
