import XCTest
@testable import TenexMVP

final class BunkerSupportTests: XCTestCase {
    private let storageKey = "bunker.autoApproveRules"
    private var originalRulesData: Data?

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
}
