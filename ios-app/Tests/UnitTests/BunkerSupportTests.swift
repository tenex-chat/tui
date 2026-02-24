import XCTest
@testable import TenexMVP

final class BunkerSupportTests: XCTestCase {
    private let storageKey = "bunker.autoApproveRules"
    private var originalRulesData: Data?
    private let sample4199EventJson = """
    {"kind":4199,"id":"c38d1ef534948f019ab2b1931640dd05dbe6a60d93bf571e0f5b32cd0503edfc","pubkey":"6cd3032c8ea6ce983f6f5941b9667812c12414f9d49105ba3dbd09c20a23a7d8","created_at":1771874326,"tags":[["title","Testing Agent"],["role","Simple Test Agent"],["description","Simple testing agent that always replies with \\"TEST 2\\""],["category","assistant"],["instructions","Regardless of what the user says or asks, you must ALWAYS reply with exactly: 'TEST 2'"],["use-criteria","Use this agent for testing purposes. It always replies with \\"TEST 2\\" no matter what input is received."],["ver","1"],["d","testing"]],"content":"# Testing Agent\\n\\nA simple test agent designed for testing and validation purposes.\\n\\n## Behavior\\nRegardless of what input or questions the user provides, this agent will **always** respond with exactly: `TEST 2`\\n\\n## Use Case\\nPerfect for testing agent delegation workflows, verifying connectivity, or as a simple sanity check in multi-agent systems."}
    """
    private let fallbackTagsJson = """
    [["title","Fallback Agent"],["role","Fallback Role"],["description","Fallback description"],["category","assistant"],["instructions","Tag instruction 1"],["instructions","Tag instruction 2"],["use-criteria","Use criteria A"],["use-criteria","Use criteria B"],["ver","5"],["d","fallback-agent"],["tool","search"],["mcp","filesystem"],["e","file-event-id"]]
    """

    override func setUpWithError() throws {
        try super.setUpWithError()
        originalRulesData = UserDefaults.standard.data(forKey: storageKey)
        UserDefaults.standard.removeObject(forKey: storageKey)
    }

    override func tearDownWithError() throws {
        if let originalRulesData {
            UserDefaults.standard.set(originalRulesData, forKey: storageKey)
        } else {
            UserDefaults.standard.removeObject(forKey: storageKey)
        }
        try super.tearDownWithError()
    }

    func testBunkerAutoApproveStorageRoundTrip() {
        let rules = [
            BunkerAutoApproveStorage.Rule(requesterPubkey: "pubkey-1", eventKind: 1),
            BunkerAutoApproveStorage.Rule(requesterPubkey: "pubkey-2", eventKind: 30023)
        ]

        BunkerAutoApproveStorage.saveRules(rules)

        XCTAssertEqual(BunkerAutoApproveStorage.loadRules(), rules)
    }

    func testBunkerAutoApproveStorageRemoveRuleRemovesOnlyTargetRule() {
        let ruleToKeepA = BunkerAutoApproveStorage.Rule(requesterPubkey: "pubkey-1", eventKind: 1)
        let ruleToRemove = BunkerAutoApproveStorage.Rule(requesterPubkey: "pubkey-2", eventKind: 4)
        let ruleToKeepB = BunkerAutoApproveStorage.Rule(requesterPubkey: "pubkey-2", eventKind: 7)

        BunkerAutoApproveStorage.saveRules([ruleToKeepA, ruleToRemove, ruleToKeepB])
        BunkerAutoApproveStorage.removeRule(
            requesterPubkey: ruleToRemove.requesterPubkey,
            eventKind: ruleToRemove.eventKind
        )

        let loaded = BunkerAutoApproveStorage.loadRules()
        XCTAssertEqual(loaded.count, 2)
        XCTAssertTrue(loaded.contains(ruleToKeepA))
        XCTAssertTrue(loaded.contains(ruleToKeepB))
        XCTAssertFalse(loaded.contains(ruleToRemove))
    }

    func testBunkerAutoApproveStorageLoadRulesReturnsEmptyWhenMissing() {
        XCTAssertTrue(BunkerAutoApproveStorage.loadRules().isEmpty)
    }

    func testFfiBunkerAutoApproveRuleRuleIdIsStableForAnyAndSpecificKind() {
        let anyKindRule = FfiBunkerAutoApproveRule(requesterPubkey: "pubkey-xyz", eventKind: nil)
        let specificKindRule = FfiBunkerAutoApproveRule(requesterPubkey: "pubkey-xyz", eventKind: 24010)

        XCTAssertEqual(anyKindRule.ruleId, "pubkey-xyz:any")
        XCTAssertEqual(specificKindRule.ruleId, "pubkey-xyz:24010")
        XCTAssertNotEqual(anyKindRule.ruleId, specificKindRule.ruleId)
    }

    func testBunkerSignPreviewModelParses4199FromEventJson() throws {
        let request = FfiBunkerSignRequest(
            requestId: "req-1",
            requesterPubkey: "pubkey-abc",
            eventKind: 4199,
            eventJson: sample4199EventJson,
            eventContent: "ignored legacy content",
            eventTagsJson: fallbackTagsJson
        )

        let model = BunkerSignPreviewModel(request: request)
        let agent = try XCTUnwrap(model.agentDefinition)

        XCTAssertTrue(model.isAgentDefinition4199)
        XCTAssertEqual(model.kind, 4199)
        XCTAssertEqual(agent.title, "Testing Agent")
        XCTAssertEqual(agent.role, "Simple Test Agent")
        XCTAssertEqual(agent.description, "Simple testing agent that always replies with \"TEST 2\"")
        XCTAssertEqual(agent.category, "assistant")
        XCTAssertEqual(agent.version, "1")
        XCTAssertEqual(agent.dTag, "testing")
        XCTAssertEqual(agent.instructionsFromTags.count, 1)
        XCTAssertEqual(
            agent.instructionsFromTags.first,
            "Regardless of what the user says or asks, you must ALWAYS reply with exactly: 'TEST 2'"
        )
        XCTAssertEqual(agent.useCriteria.count, 1)
        XCTAssertTrue(agent.contentMarkdown.contains("# Testing Agent"))
        XCTAssertTrue(agent.contentMarkdown.contains("`TEST 2`"))
        XCTAssertTrue(model.rawEventJson.contains("\"kind\" : 4199"))
    }

    func testBunkerSignPreviewModelFallsBackToLegacyPayload() throws {
        let request = FfiBunkerSignRequest(
            requestId: "req-2",
            requesterPubkey: "pubkey-fallback",
            eventKind: 4199,
            eventJson: nil,
            eventContent: "# Fallback Content\\n\\nFrom `eventContent`.",
            eventTagsJson: fallbackTagsJson
        )

        let model = BunkerSignPreviewModel(request: request)
        let agent = try XCTUnwrap(model.agentDefinition)

        XCTAssertTrue(model.isAgentDefinition4199)
        XCTAssertEqual(agent.title, "Fallback Agent")
        XCTAssertEqual(agent.role, "Fallback Role")
        XCTAssertEqual(agent.description, "Fallback description")
        XCTAssertEqual(agent.category, "assistant")
        XCTAssertEqual(agent.version, "5")
        XCTAssertEqual(agent.dTag, "fallback-agent")
        XCTAssertEqual(agent.instructionsFromTags, ["Tag instruction 1", "Tag instruction 2"])
        XCTAssertEqual(agent.useCriteria, ["Use criteria A", "Use criteria B"])
        XCTAssertEqual(agent.tools, ["search"])
        XCTAssertEqual(agent.mcpServers, ["filesystem"])
        XCTAssertEqual(agent.fileEventIds, ["file-event-id"])
        XCTAssertTrue(agent.contentMarkdown.contains("# Fallback Content"))
        XCTAssertTrue(model.rawEventJson.contains("\"title\""))
    }

    func testBunkerSignPreviewModelPrefersEventJsonOverLegacyFields() throws {
        let overrideEventJson = """
        {"kind":4199,"content":"# Event JSON Content","tags":[["title","From Event JSON"],["instructions","JSON instruction"]]}
        """
        let request = FfiBunkerSignRequest(
            requestId: "req-3",
            requesterPubkey: "pubkey-prefer-json",
            eventKind: 4199,
            eventJson: overrideEventJson,
            eventContent: "# Legacy Content",
            eventTagsJson: "[[\"title\",\"Legacy Title\"],[\"instructions\",\"Legacy instruction\"]]"
        )

        let model = BunkerSignPreviewModel(request: request)
        let agent = try XCTUnwrap(model.agentDefinition)

        XCTAssertEqual(agent.title, "From Event JSON")
        XCTAssertEqual(agent.instructionsFromTags, ["JSON instruction"])
        XCTAssertEqual(agent.contentMarkdown, "# Event JSON Content")
    }

    func testBunkerSignPreviewModelNon4199KeepsRawOnly() {
        let request = FfiBunkerSignRequest(
            requestId: "req-4",
            requesterPubkey: "pubkey-note",
            eventKind: 1,
            eventJson: "{\"kind\":1,\"content\":\"hello\"}",
            eventContent: "hello",
            eventTagsJson: "[]"
        )

        let model = BunkerSignPreviewModel(request: request)

        XCTAssertFalse(model.isAgentDefinition4199)
        XCTAssertEqual(model.kind, 1)
        XCTAssertNil(model.agentDefinition)
        XCTAssertTrue(model.rawEventJson.contains("\"kind\" : 1"))
    }
}
