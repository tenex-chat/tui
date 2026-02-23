import XCTest
import SwiftUI
@testable import TenexMVP

final class ConversationFormattersTests: XCTestCase {

    // MARK: - formatDuration

    func testFormatDurationLessThanOneMinute() {
        XCTAssertEqual(ConversationFormatters.formatDuration(0), "<1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(1), "<1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(30), "<1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(59), "<1m")
    }

    func testFormatDurationExactMinutes() {
        XCTAssertEqual(ConversationFormatters.formatDuration(60), "1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(120), "2m")
        XCTAssertEqual(ConversationFormatters.formatDuration(300), "5m")
        XCTAssertEqual(ConversationFormatters.formatDuration(3540), "59m")
    }

    func testFormatDurationMinutesIgnoresRemainingSeconds() {
        // 90 seconds = 1m 30s, but formatDuration only shows minutes
        XCTAssertEqual(ConversationFormatters.formatDuration(90), "1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(150), "2m")
    }

    func testFormatDurationExactHours() {
        XCTAssertEqual(ConversationFormatters.formatDuration(3600), "1h 0m")
        XCTAssertEqual(ConversationFormatters.formatDuration(7200), "2h 0m")
    }

    func testFormatDurationHoursAndMinutes() {
        XCTAssertEqual(ConversationFormatters.formatDuration(3660), "1h 1m")
        XCTAssertEqual(ConversationFormatters.formatDuration(5400), "1h 30m")
        XCTAssertEqual(ConversationFormatters.formatDuration(9000), "2h 30m")
        XCTAssertEqual(ConversationFormatters.formatDuration(86400), "24h 0m")
    }

    func testFormatDurationHoursWithFractionalSeconds() {
        // 3601.5 seconds = 1h 0m (seconds are truncated)
        XCTAssertEqual(ConversationFormatters.formatDuration(3601.5), "1h 0m")
    }

    func testFormatDurationLargeValues() {
        // 48 hours
        XCTAssertEqual(ConversationFormatters.formatDuration(172800), "48h 0m")
    }

    // MARK: - shortId

    func testShortIdTruncatesTo12Characters() {
        let fullId = "abcdef1234567890abcdef1234567890"
        XCTAssertEqual(ConversationFormatters.shortId(fullId), "abcdef123456")
    }

    func testShortIdShorterThan12ReturnsWhole() {
        XCTAssertEqual(ConversationFormatters.shortId("abc"), "abc")
        XCTAssertEqual(ConversationFormatters.shortId("12chars12345"), "12chars12345")
    }

    func testShortIdExactly12Characters() {
        XCTAssertEqual(ConversationFormatters.shortId("123456789012"), "123456789012")
    }

    func testShortIdEmptyString() {
        XCTAssertEqual(ConversationFormatters.shortId(""), "")
    }

    // MARK: - deterministicColor

    func testDeterministicColorSameInputSameOutput() {
        let color1 = deterministicColor(for: "test-identifier")
        let color2 = deterministicColor(for: "test-identifier")
        XCTAssertEqual(color1, color2)
    }

    func testDeterministicColorDifferentInputsCanDiffer() {
        // With enough different inputs, we should see at least 2 distinct colors
        let inputs = ["alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi"]
        let colors = Set(inputs.map { deterministicColor(for: $0) })
        XCTAssertGreaterThan(colors.count, 1, "Different inputs should produce variety in colors")
    }

    func testDeterministicColorCustomPalette() {
        let customPalette: [Color] = [.red, .yellow]
        let color = deterministicColor(for: "anything", from: customPalette)
        // With only 2 colors, result must be one of them
        XCTAssertTrue(color == .red || color == .yellow)
    }

    func testDeterministicColorEmptyStringDoesNotCrash() {
        let color = deterministicColor(for: "")
        // Just verify it returns a valid color without crashing
        XCTAssertNotNil(color)
    }

    func testDeterministicColorStabilityAcrossCalls() {
        // Verify determinism holds for many iterations
        let identifier = "stable-test-pubkey-abc123"
        let reference = deterministicColor(for: identifier)
        for _ in 0..<100 {
            XCTAssertEqual(deterministicColor(for: identifier), reference)
        }
    }

    // MARK: - generateContextMessage (with messages array)

    func testGenerateContextMessageIncludesShortId() {
        let conversationId = "abcdef1234567890abcdef1234567890"
        let messages: [Message] = []
        let result = ConversationFormatters.generateContextMessage(
            conversationId: conversationId,
            messages: messages
        )
        XCTAssertTrue(result.contains("abcdef123456"))
    }

    func testGenerateContextMessageIncludesFullId() {
        let conversationId = "abcdef1234567890"
        let messages: [Message] = []
        let result = ConversationFormatters.generateContextMessage(
            conversationId: conversationId,
            messages: messages
        )
        XCTAssertTrue(result.contains("full: abcdef1234567890"))
    }

    func testGenerateContextMessageTokenCountFromMessages() {
        let messages = [
            makeMessage(content: String(repeating: "a", count: 400)),  // 400 chars / 4 = 100 tokens
            makeMessage(content: String(repeating: "b", count: 200)),  // 200 chars / 4 = 50 tokens
        ]
        let result = ConversationFormatters.generateContextMessage(
            conversationId: "test-id",
            messages: messages
        )
        // Total: 600 chars / 4 = 150 tokens
        XCTAssertTrue(result.contains("150 tokens"))
    }

    func testGenerateContextMessageEmptyMessagesZeroTokens() {
        let result = ConversationFormatters.generateContextMessage(
            conversationId: "test-id",
            messages: []
        )
        XCTAssertTrue(result.contains("0 tokens"))
    }

    func testGenerateContextMessageInstructsToInspect() {
        let result = ConversationFormatters.generateContextMessage(
            conversationId: "test-id",
            messages: []
        )
        XCTAssertTrue(result.contains("conversation_get"))
    }

    // MARK: - generateContextMessage (with ConversationFullInfo)

    func testGenerateContextMessageConversationInfoIncludesShortId() {
        let conversation = makeConversation(id: "abcdef1234567890abcdef1234567890", messageCount: 5)
        let result = ConversationFormatters.generateContextMessage(conversation: conversation)
        XCTAssertTrue(result.contains("abcdef123456"))
    }

    func testGenerateContextMessageConversationInfoIncludesFullId() {
        let conversation = makeConversation(id: "full-conversation-id-123", messageCount: 5)
        let result = ConversationFormatters.generateContextMessage(conversation: conversation)
        XCTAssertTrue(result.contains("full: full-conversation-id-123"))
    }

    func testGenerateContextMessageConversationInfoDefaultTokenEstimate() {
        // Default: 200 tokens per message, 10 messages = 2000 tokens
        let conversation = makeConversation(id: "test", messageCount: 10)
        let result = ConversationFormatters.generateContextMessage(conversation: conversation)
        XCTAssertTrue(result.contains("2000 tokens"))
    }

    func testGenerateContextMessageConversationInfoCustomTokenEstimate() {
        let conversation = makeConversation(id: "test", messageCount: 10)
        let result = ConversationFormatters.generateContextMessage(
            conversation: conversation,
            estimatedTokensPerMessage: 500
        )
        // 10 * 500 = 5000 tokens
        XCTAssertTrue(result.contains("5000 tokens"))
    }

    func testGenerateContextMessageConversationInfoZeroMessages() {
        let conversation = makeConversation(id: "test", messageCount: 0)
        let result = ConversationFormatters.generateContextMessage(conversation: conversation)
        XCTAssertTrue(result.contains("0 tokens"))
    }

    // MARK: - generateReportContextMessage

    func testGenerateReportContextMessageIncludesTitle() {
        let report = makeReport(title: "Weekly Summary", slug: "weekly-summary", content: "Report content here")
        let result = ConversationFormatters.generateReportContextMessage(report: report)
        XCTAssertTrue(result.contains("Weekly Summary"))
    }

    func testGenerateReportContextMessageIncludesSlug() {
        let report = makeReport(title: "Test", slug: "test-slug", content: "Content")
        let result = ConversationFormatters.generateReportContextMessage(report: report)
        XCTAssertTrue(result.contains("slug: test-slug"))
    }

    func testGenerateReportContextMessageTokenEstimate() {
        // 800 chars / 4 = 200 tokens
        let content = String(repeating: "x", count: 800)
        let report = makeReport(title: "Test", slug: "test", content: content)
        let result = ConversationFormatters.generateReportContextMessage(report: report)
        XCTAssertTrue(result.contains("200 tokens"))
    }

    func testGenerateReportContextMessageEmptyContent() {
        let report = makeReport(title: "Empty", slug: "empty", content: "")
        let result = ConversationFormatters.generateReportContextMessage(report: report)
        XCTAssertTrue(result.contains("0 tokens"))
    }

    func testGenerateReportContextMessageMentionsReportRead() {
        let report = makeReport(title: "T", slug: "s", content: "c")
        let result = ConversationFormatters.generateReportContextMessage(report: report)
        XCTAssertTrue(result.contains("report_read"))
    }

    // MARK: - formatRelativeTime

    func testFormatRelativeTimeRecentTimestamp() {
        // Use a timestamp that is 30 seconds ago
        let now = UInt64(Date().timeIntervalSince1970)
        let thirtySecondsAgo = now - 30
        let result = ConversationFormatters.formatRelativeTime(thirtySecondsAgo)
        // RelativeDateTimeFormatter with abbreviated style produces locale-dependent output,
        // but it should not be empty and should contain "ago" or a time indicator
        XCTAssertFalse(result.isEmpty)
    }

    func testFormatRelativeTimeFutureTimestamp() {
        // A timestamp in the future
        let now = UInt64(Date().timeIntervalSince1970)
        let future = now + 3600
        let result = ConversationFormatters.formatRelativeTime(future)
        // Should return a non-empty string (e.g., "in 1 hr.")
        XCTAssertFalse(result.isEmpty)
    }

    func testFormatRelativeTimeOldTimestamp() {
        // A timestamp from 2 days ago
        let now = UInt64(Date().timeIntervalSince1970)
        let twoDaysAgo = now - 172800
        let result = ConversationFormatters.formatRelativeTime(twoDaysAgo)
        XCTAssertFalse(result.isEmpty)
    }

    func testFormatRelativeTimeZeroTimestamp() {
        // Epoch timestamp (1970) - should return something like "54 yr. ago"
        let result = ConversationFormatters.formatRelativeTime(0)
        XCTAssertFalse(result.isEmpty)
    }

    func testFormatRelativeTimeVeryRecentReturnsShortString() {
        // 1 second ago - should be very short abbreviated string
        let now = UInt64(Date().timeIntervalSince1970)
        let result = ConversationFormatters.formatRelativeTime(now - 1)
        // Abbreviated style should produce something compact (e.g., "1 sec. ago")
        XCTAssertLessThan(result.count, 30, "Abbreviated relative time should be compact")
    }

    // MARK: - Helpers

    private func makeMessage(content: String) -> Message {
        Message(
            id: UUID().uuidString,
            content: content,
            pubkey: "test-pubkey",
            threadId: "test-thread",
            createdAt: UInt64(Date().timeIntervalSince1970),
            replyTo: nil,
            isReasoning: false,
            askEvent: nil,
            qTags: [],
            aTags: [],
            pTags: [],
            toolName: nil,
            toolArgs: nil,
            llmMetadata: [:],
            delegationTag: nil,
            branch: nil
        )
    }

    private func makeConversation(id: String, messageCount: UInt32) -> ConversationFullInfo {
        let thread = Thread(
            id: id,
            title: "Test Conversation",
            content: "",
            pubkey: "test-pubkey",
            lastActivity: UInt64(Date().timeIntervalSince1970),
            effectiveLastActivity: UInt64(Date().timeIntervalSince1970),
            statusLabel: nil,
            statusCurrentActivity: nil,
            summary: nil,
            hashtags: [],
            parentConversationId: nil,
            pTags: [],
            askEvent: nil,
            isScheduled: false
        )
        return ConversationFullInfo(
            thread: thread,
            author: "test-author",
            messageCount: messageCount,
            isActive: false,
            isArchived: false,
            hasChildren: false,
            projectATag: "31922:owner:project-1"
        )
    }

    private func makeReport(title: String, slug: String, content: String) -> Report {
        Report(
            id: "report-id",
            slug: slug,
            projectATag: "31922:owner:project-1",
            author: "author-pubkey",
            title: title,
            summary: "Summary",
            content: content,
            hashtags: [],
            createdAt: UInt64(Date().timeIntervalSince1970),
            readingTimeMins: 1
        )
    }
}
