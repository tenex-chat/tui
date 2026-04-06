import XCTest
import SwiftUI
#if os(macOS)
import AppKit
#endif
@testable import TenexMVP

final class PlatformColorsTests: XCTestCase {

    // MARK: - conversationStatus(for:isActive:) — isActive overrides everything

    func testConversationStatusIsActiveOverridesNilStatus() {
        XCTAssertEqual(Color.conversationStatus(for: nil, isActive: true), .statusActive)
    }

    func testConversationStatusIsActiveOverridesWaitingStatus() {
        XCTAssertEqual(Color.conversationStatus(for: "waiting", isActive: true), .statusActive)
    }

    func testConversationStatusIsActiveOverridesCompletedStatus() {
        XCTAssertEqual(Color.conversationStatus(for: "completed", isActive: true), .statusActive)
    }

    func testConversationStatusIsActiveOverridesUnknownStatus() {
        XCTAssertEqual(Color.conversationStatus(for: "unknown", isActive: true), .statusActive)
    }

    // MARK: - conversationStatus(for:isActive:) — active statuses

    func testConversationStatusActiveLowercase() {
        XCTAssertEqual(Color.conversationStatus(for: "active"), .statusActive)
    }

    func testConversationStatusActiveUppercase() {
        XCTAssertEqual(Color.conversationStatus(for: "Active"), .statusActive)
    }

    func testConversationStatusActiveMixedCase() {
        XCTAssertEqual(Color.conversationStatus(for: "ACTIVE"), .statusActive)
    }

    func testConversationStatusInProgress() {
        XCTAssertEqual(Color.conversationStatus(for: "in progress"), .statusActive)
    }

    func testConversationStatusInProgressMixedCase() {
        XCTAssertEqual(Color.conversationStatus(for: "In Progress"), .statusActive)
    }

    // MARK: - conversationStatus(for:isActive:) — waiting statuses

    func testConversationStatusWaiting() {
        XCTAssertEqual(Color.conversationStatus(for: "waiting"), .statusWaiting)
    }

    func testConversationStatusWaitingUppercase() {
        XCTAssertEqual(Color.conversationStatus(for: "WAITING"), .statusWaiting)
    }

    func testConversationStatusBlocked() {
        XCTAssertEqual(Color.conversationStatus(for: "blocked"), .statusWaiting)
    }

    func testConversationStatusBlockedMixedCase() {
        XCTAssertEqual(Color.conversationStatus(for: "Blocked"), .statusWaiting)
    }

    // MARK: - conversationStatus(for:isActive:) — completed statuses

    func testConversationStatusCompleted() {
        XCTAssertEqual(Color.conversationStatus(for: "completed"), .statusCompleted)
    }

    func testConversationStatusCompletedUppercase() {
        XCTAssertEqual(Color.conversationStatus(for: "COMPLETED"), .statusCompleted)
    }

    func testConversationStatusDone() {
        XCTAssertEqual(Color.conversationStatus(for: "done"), .statusCompleted)
    }

    func testConversationStatusDoneMixedCase() {
        XCTAssertEqual(Color.conversationStatus(for: "Done"), .statusCompleted)
    }

    // MARK: - conversationStatus(for:isActive:) — default / unknown

    func testConversationStatusNilReturnsDefault() {
        XCTAssertEqual(Color.conversationStatus(for: nil), .statusDefault)
    }

    func testConversationStatusEmptyStringReturnsDefault() {
        XCTAssertEqual(Color.conversationStatus(for: ""), .statusDefault)
    }

    func testConversationStatusUnknownStringReturnsDefault() {
        XCTAssertEqual(Color.conversationStatus(for: "something-random"), .statusDefault)
    }

    func testConversationStatusGarbageReturnsDefault() {
        XCTAssertEqual(Color.conversationStatus(for: "🐛"), .statusDefault)
    }

    // MARK: - conversationStatusBackground(for:isActive:) — isActive overrides

    func testConversationStatusBackgroundIsActiveOverridesNil() {
        XCTAssertEqual(Color.conversationStatusBackground(for: nil, isActive: true), .statusActiveBackground)
    }

    func testConversationStatusBackgroundIsActiveOverridesWaiting() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "waiting", isActive: true), .statusActiveBackground)
    }

    func testConversationStatusBackgroundIsActiveOverridesCompleted() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "completed", isActive: true), .statusActiveBackground)
    }

    // MARK: - conversationStatusBackground(for:isActive:) — active statuses

    func testConversationStatusBackgroundActive() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "active"), .statusActiveBackground)
    }

    func testConversationStatusBackgroundInProgress() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "in progress"), .statusActiveBackground)
    }

    func testConversationStatusBackgroundActiveMixedCase() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "Active"), .statusActiveBackground)
    }

    // MARK: - conversationStatusBackground(for:isActive:) — waiting statuses

    func testConversationStatusBackgroundWaiting() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "waiting"), .statusWaitingBackground)
    }

    func testConversationStatusBackgroundBlocked() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "blocked"), .statusWaitingBackground)
    }

    func testConversationStatusBackgroundWaitingUppercase() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "WAITING"), .statusWaitingBackground)
    }

    // MARK: - conversationStatusBackground(for:isActive:) — completed statuses

    func testConversationStatusBackgroundCompleted() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "completed"), .statusCompletedBackground)
    }

    func testConversationStatusBackgroundDone() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "done"), .statusCompletedBackground)
    }

    func testConversationStatusBackgroundDoneMixedCase() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "Done"), .statusCompletedBackground)
    }

    // MARK: - conversationStatusBackground(for:isActive:) — default / unknown

    func testConversationStatusBackgroundNilReturnsDefault() {
        XCTAssertEqual(Color.conversationStatusBackground(for: nil), .statusDefaultBackground)
    }

    func testConversationStatusBackgroundEmptyStringReturnsDefault() {
        XCTAssertEqual(Color.conversationStatusBackground(for: ""), .statusDefaultBackground)
    }

    func testConversationStatusBackgroundUnknownReturnsDefault() {
        XCTAssertEqual(Color.conversationStatusBackground(for: "random-value"), .statusDefaultBackground)
    }

    // MARK: - Foreground/background consistency

    func testActiveStatusAndBackgroundAreConsistentPair() {
        let fg = Color.conversationStatus(for: "active")
        let bg = Color.conversationStatusBackground(for: "active")
        XCTAssertEqual(fg, .statusActive)
        XCTAssertEqual(bg, .statusActiveBackground)
    }

    func testWaitingStatusAndBackgroundAreConsistentPair() {
        let fg = Color.conversationStatus(for: "waiting")
        let bg = Color.conversationStatusBackground(for: "waiting")
        XCTAssertEqual(fg, .statusWaiting)
        XCTAssertEqual(bg, .statusWaitingBackground)
    }

    func testCompletedStatusAndBackgroundAreConsistentPair() {
        let fg = Color.conversationStatus(for: "completed")
        let bg = Color.conversationStatusBackground(for: "completed")
        XCTAssertEqual(fg, .statusCompleted)
        XCTAssertEqual(bg, .statusCompletedBackground)
    }

    func testDefaultStatusAndBackgroundAreConsistentPair() {
        let fg = Color.conversationStatus(for: nil)
        let bg = Color.conversationStatusBackground(for: nil)
        XCTAssertEqual(fg, .statusDefault)
        XCTAssertEqual(bg, .statusDefaultBackground)
    }

    // MARK: - Both methods agree on isActive default

    func testIsActiveDefaultIsFalse() {
        // Calling without isActive should behave like isActive: false
        XCTAssertEqual(
            Color.conversationStatus(for: "waiting"),
            Color.conversationStatus(for: "waiting", isActive: false)
        )
        XCTAssertEqual(
            Color.conversationStatusBackground(for: "waiting"),
            Color.conversationStatusBackground(for: "waiting", isActive: false)
        )
    }

    // MARK: - Static color identity checks

    func testStatusActiveIsGreen() {
        XCTAssertEqual(Color.statusActive, .accentColor)
    }

    func testStatusWaitingIsOrange() {
        XCTAssertEqual(Color.statusWaiting, .accentColor)
    }

    func testStatusCompletedIsGray() {
        XCTAssertEqual(Color.statusCompleted, .gray)
    }

    func testStatusDefaultIsBlue() {
        XCTAssertEqual(Color.statusDefault, .blue)
    }

    func testStatusActiveBackgroundIsGreenWithOpacity() {
        XCTAssertEqual(Color.statusActiveBackground, Color.accentColor.opacity(0.15))
    }

    func testStatusWaitingBackgroundIsOrangeWithOpacity() {
        XCTAssertEqual(Color.statusWaitingBackground, Color.accentColor.opacity(0.15))
    }

    func testStatusCompletedBackgroundIsGrayWithOpacity() {
        XCTAssertEqual(Color.statusCompletedBackground, .gray.opacity(0.15))
    }

    func testStatusDefaultBackgroundIsBlueWithOpacity() {
        XCTAssertEqual(Color.statusDefaultBackground, .blue.opacity(0.15))
    }

    // MARK: - Feature brand colors

    func testSkillBrandIsOrange() {
        XCTAssertEqual(Color.skillBrand, .accentColor)
    }

    func testSkillBrandBackgroundIsOrangeWithOpacity() {
        XCTAssertEqual(Color.skillBrandBackground, Color.accentColor.opacity(0.15))
    }

    func testAgentBrandIsBlue() {
        XCTAssertEqual(Color.agentBrand, .blue)
    }

    func testProjectBrandIsPurple() {
        XCTAssertEqual(Color.projectBrand, .purple)
    }

    func testProjectBrandBackgroundIsPurpleWithOpacity() {
        XCTAssertEqual(Color.projectBrandBackground, .purple.opacity(0.15))
    }

    // MARK: - Presence colors

    func testPresenceOnlineIsGreen() {
        XCTAssertEqual(Color.presenceOnline, .accentColor)
    }

    func testPresenceOnlineBackgroundIsGreenWithOpacity() {
        XCTAssertEqual(Color.presenceOnlineBackground, Color.accentColor.opacity(0.15))
    }

    func testPresenceOfflineIsGray() {
        XCTAssertEqual(Color.presenceOffline, .gray)
    }

    func testPresenceOfflineBackgroundIsGrayWithOpacity() {
        XCTAssertEqual(Color.presenceOfflineBackground, .gray.opacity(0.15))
    }

    // MARK: - Ask / Question brand colors

    func testAskBrandIsOrange() {
        XCTAssertEqual(Color.askBrand, .accentColor)
    }

    func testAskBrandSubtleBackgroundOpacity() {
        XCTAssertEqual(Color.askBrandSubtleBackground, Color.accentColor.opacity(0.05))
    }

    func testAskBrandBackgroundOpacity() {
        XCTAssertEqual(Color.askBrandBackground, Color.accentColor.opacity(0.15))
    }

    func testAskBrandBorderOpacity() {
        XCTAssertEqual(Color.askBrandBorder, Color.accentColor.opacity(0.3))
    }

    // MARK: - Message bubble colors

    func testMessageBubbleUserBackground() {
        XCTAssertEqual(Color.messageBubbleUserBackground, .blue.opacity(0.15))
    }

    func testMessageUserAvatarColor() {
        XCTAssertEqual(Color.messageUserAvatarColor, .accentColor)
    }

    // MARK: - Todo colors

    func testTodoDoneIsGreen() {
        XCTAssertEqual(Color.todoDone, .accentColor)
    }

    func testTodoDoneBackgroundIsGreenWithOpacity() {
        XCTAssertEqual(Color.todoDoneBackground, Color.accentColor.opacity(0.15))
    }

    func testTodoInProgressIsBlue() {
        XCTAssertEqual(Color.todoInProgress, .blue)
    }

    func testTodoSkippedIsGray() {
        XCTAssertEqual(Color.todoSkipped, .gray)
    }

    // MARK: - Recording colors

    func testRecordingActiveIsRed() {
        XCTAssertEqual(Color.recordingActive, .red)
    }

    func testRecordingActiveBackgroundIsRedWithOpacity() {
        XCTAssertEqual(Color.recordingActiveBackground, .red.opacity(0.3))
    }

    // MARK: - Health / Diagnostics colors

    func testHealthGoodIsGreen() {
        XCTAssertEqual(Color.healthGood, .accentColor)
    }

    func testHealthWarningIsOrange() {
        XCTAssertEqual(Color.healthWarning, .accentColor)
    }

    func testHealthErrorIsRed() {
        XCTAssertEqual(Color.healthError, .red)
    }

    // MARK: - Inbox colors

    func testUnreadIndicatorIsBlue() {
        XCTAssertEqual(Color.unreadIndicator, .blue)
    }

    // MARK: - Composer colors

    func testComposerActionIsBlue() {
        XCTAssertEqual(Color.composerAction, .blue)
    }

    func testComposerDestructiveIsRed() {
        XCTAssertEqual(Color.composerDestructive, .red)
    }

    func testComposerWarningIsOrange() {
        XCTAssertEqual(Color.composerWarning, .accentColor)
    }

    // MARK: - Stats colors

    func testStatCostIsGreen() {
        XCTAssertEqual(Color.statCost, .accentColor)
    }

    func testStatRuntimeIsBlue() {
        XCTAssertEqual(Color.statRuntime, .blue)
    }

    func testStatAverageIsPurple() {
        XCTAssertEqual(Color.statAverage, .purple)
    }

    func testStatUserMessagesIsBlue() {
        XCTAssertEqual(Color.statUserMessages, .blue)
    }

    func testStatAllMessagesIsPurple() {
        XCTAssertEqual(Color.statAllMessages, .purple)
    }

    // MARK: - macOS-specific workspace surface colors

    #if os(macOS)
    func testConversationWorkspaceBackdropMacUsesWindowBackground() {
        let expected = Color(nsColor: .windowBackgroundColor)
        XCTAssertEqual(Color.conversationWorkspaceBackdropMac, expected)
    }

    func testConversationWorkspaceSurfaceMacUsesControlBackground() {
        let expected = Color(nsColor: .controlBackgroundColor)
        XCTAssertEqual(Color.conversationWorkspaceSurfaceMac, expected)
    }

    func testConversationWorkspaceBorderMacUsesSeparatorColor() {
        let expected = Color(nsColor: .separatorColor)
        XCTAssertEqual(Color.conversationWorkspaceBorderMac, expected)
    }

    func testConversationComposerShellMacUsesTextBackground() {
        let expected = Color(nsColor: .textBackgroundColor)
        XCTAssertEqual(Color.conversationComposerShellMac, expected)
    }

    func testConversationComposerFooterMacUsesControlBackground() {
        let expected = Color(nsColor: .controlBackgroundColor)
        XCTAssertEqual(Color.conversationComposerFooterMac, expected)
    }

    func testConversationComposerStrokeMacUsesSeparatorColor() {
        let expected = Color(nsColor: .separatorColor)
        XCTAssertEqual(Color.conversationComposerStrokeMac, expected)
    }
    #endif
}
