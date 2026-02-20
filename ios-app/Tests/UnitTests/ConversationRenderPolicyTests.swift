import XCTest
@testable import TenexMVP

final class ConversationRenderPolicyTests: XCTestCase {

    // MARK: - shouldRenderQTags

    func testShouldRenderQTagsAllowsNilToolName() {
        XCTAssertTrue(ConversationRenderPolicy.shouldRenderQTags(toolName: nil))
    }

    func testShouldRenderQTagsAllowsUnknownTool() {
        XCTAssertTrue(ConversationRenderPolicy.shouldRenderQTags(toolName: "bash"))
    }

    func testShouldRenderQTagsDeniesListedTools() {
        for tool in ConversationRenderPolicy.qTagRenderDenylist {
            XCTAssertFalse(
                ConversationRenderPolicy.shouldRenderQTags(toolName: tool),
                "Expected q-tag rendering to be denied for \(tool)"
            )
        }
    }

    // MARK: - toolSummary: bash / shell

    func testBashWithCommand() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "bash",
            toolArgs: json(["command": "ls -la"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "terminal")
        XCTAssertEqual(summary.text, "$ ls -la")
    }

    func testBashWithDescription() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "execute_bash",
            toolArgs: json(["description": "list files", "command": "ls -la"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "terminal")
        // description takes priority over command for shell operations
        XCTAssertEqual(summary.text, "$ list files")
    }

    func testShellAlias() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "shell",
            toolArgs: json(["command": "echo hi"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "terminal")
        XCTAssertTrue(summary.text.hasPrefix("$ "))
    }

    // MARK: - toolSummary: file read

    func testFileReadWithPath() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "file_read",
            toolArgs: json(["file_path": "/src/main.rs"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "doc.text")
        XCTAssertTrue(summary.text.contains("main.rs"))
    }

    func testFsReadAlias() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "fs_read",
            toolArgs: json(["path": "README.md"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "doc.text")
        XCTAssertEqual(summary.text, "\u{1F4D6} README.md")
    }

    // MARK: - toolSummary: file write / edit

    func testFileWriteWithPath() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "write",
            toolArgs: json(["file_path": "/a/b/c/output.txt"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "square.and.pencil")
        // deep path should be truncated to last 2 segments
        XCTAssertEqual(summary.text, "\u{270F}\u{FE0F} .../c/output.txt")
    }

    func testFileReadWithDescription() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "read",
            toolArgs: json(["description": "reading config", "file_path": "/a/b/c.txt"]),
            contentFallback: nil
        )
        // file operations prefer description over path
        XCTAssertEqual(summary.text, "\u{1F4D6} reading config")
    }

    func testEditAlias() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "str_replace_editor",
            toolArgs: json(["file_path": "lib.rs"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "square.and.pencil")
    }

    // MARK: - toolSummary: search / grep / glob

    func testGrepWithPattern() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "grep",
            toolArgs: json(["pattern": "TODO"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "magnifyingglass")
        XCTAssertEqual(summary.text, "\u{1F50D} \"TODO\"")
    }

    func testWebSearchWithQuery() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "web_search",
            toolArgs: json(["query": "swift concurrency"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "magnifyingglass")
        XCTAssertEqual(summary.text, "\u{1F50D} \"swift concurrency\"")
    }

    func testFsGlobAlias() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "fs_glob",
            toolArgs: json(["pattern": "*.swift"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "magnifyingglass")
        XCTAssertTrue(summary.text.contains("*.swift"))
    }

    // MARK: - toolSummary: ask

    func testAskWithTitleOnly() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "ask",
            toolArgs: json(["title": "Confirm deletion"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "questionmark.circle")
        XCTAssertEqual(summary.text, "Asking: \"Confirm deletion\"")
    }

    func testAskWithTitleAndQuestions() {
        let args: [String: Any] = [
            "title": "Setup",
            "questions": [
                ["header": "Name"],
                ["header": "Email"]
            ]
        ]
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "askuserquestion",
            toolArgs: jsonAny(args),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "questionmark.circle")
        XCTAssertEqual(summary.text, "Asking: \"Setup\" [Name, Email]")
    }

    func testAskWithNoTitleFallsBackToQuestion() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "ask",
            toolArgs: json([:]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.text, "Asking: \"Question\"")
    }

    // MARK: - toolSummary: todo_write

    func testTodoWriteCountsTodos() {
        let args: [String: Any] = [
            "todos": [
                ["text": "fix bug"],
                ["text": "write tests"],
                ["text": "deploy"]
            ]
        ]
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "todo_write",
            toolArgs: jsonAny(args),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "checklist")
        XCTAssertEqual(summary.text, "\u{25B8} 3 tasks")
    }

    func testTodoWriteWithItemsKey() {
        let args: [String: Any] = [
            "items": [["text": "one"], ["text": "two"]]
        ]
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "todowrite",
            toolArgs: jsonAny(args),
            contentFallback: nil
        )
        XCTAssertEqual(summary.text, "\u{25B8} 2 tasks")
    }

    func testTodoWriteWithNoArrayShowsZero() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "mcp__tenex__todo_write",
            toolArgs: json([:]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "checklist")
        XCTAssertEqual(summary.text, "\u{25B8} 0 tasks")
    }

    // MARK: - toolSummary: task / agent

    func testTaskWithDescription() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "task",
            toolArgs: json(["description": "Run integration tests"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "play.fill")
        XCTAssertEqual(summary.text, "\u{25B6} Run integration tests")
    }

    // MARK: - toolSummary: change_model

    func testChangeModel() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "change_model",
            toolArgs: json(["variant": "opus"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "brain")
        XCTAssertEqual(summary.text, "\u{1F9E0} \u{2192} opus")
    }

    // MARK: - toolSummary: conversation_get

    func testConversationGetWithPrompt() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "conversation_get",
            toolArgs: json(["conversationId": "abc123def456", "prompt": "summarize this"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "doc.text.magnifyingglass")
        XCTAssertEqual(summary.text, "\u{1F4DC} abc123def456 \u{2192} \"summarize this\"")
    }

    func testConversationGetTruncatesLongId() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "mcp__tenex__conversation_get",
            toolArgs: json(["conversation_id": "abcdef1234567890"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "doc.text.magnifyingglass")
        // id truncated to 12 chars: "abcdef123..."
        XCTAssertTrue(summary.text.contains("abcdef123..."))
    }

    // MARK: - toolSummary: default / fallback

    func testUnknownToolWithContentFallback() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "custom_tool",
            toolArgs: nil,
            contentFallback: "Some result output"
        )
        XCTAssertEqual(summary.text, "Some result output")
    }

    func testUnknownToolWithNoFallbackShowsExecuting() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "custom_tool",
            toolArgs: json(["file_path": "data.json"]),
            contentFallback: nil
        )
        // default verb is "Executing", target is "data.json"
        XCTAssertEqual(summary.text, "Executing data.json")
    }

    func testMcpToolGetsIcon() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "mcp__github__create_pr",
            toolArgs: nil,
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "puzzlepiece")
    }

    func testNonMcpUnknownToolGetsWrenchIcon() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "custom_tool",
            toolArgs: nil,
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "wrench")
    }

    func testNilToolNameNilArgsFallsBackToToolLabel() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: nil,
            toolArgs: nil,
            contentFallback: nil
        )
        // normalizedName is "", verb is "Executing", target is ""
        // verb non-empty, target empty -> just verb
        XCTAssertEqual(summary.text, "Executing")
    }

    // MARK: - toolSummary: truncation via extractTarget

    func testLongCommandGetsTruncated() {
        let longCommand = String(repeating: "a", count: 100)
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "bash",
            toolArgs: json(["command": longCommand]),
            contentFallback: nil
        )
        // command truncated to 50 chars: 47 chars + "..."
        XCTAssertTrue(summary.text.hasSuffix("..."))
        XCTAssertLessThanOrEqual(summary.text.count, 2 + 50) // "$ " + 50
    }

    func testLongContentFallbackGetsTruncated() {
        let longFallback = String(repeating: "z", count: 200)
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "unknown_tool",
            toolArgs: nil,
            contentFallback: longFallback
        )
        XCTAssertTrue(summary.text.hasSuffix("..."))
        XCTAssertLessThanOrEqual(summary.text.count, 80)
    }

    // MARK: - toolSummary: path shortening

    func testDeepPathShortenedToLastTwoSegments() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "read",
            toolArgs: json(["file_path": "/Users/dev/projects/myapp/src/lib.rs"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.text, "\u{1F4D6} .../src/lib.rs")
    }

    func testShallowPathKeptIntact() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "read",
            toolArgs: json(["path": "lib.rs"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.text, "\u{1F4D6} lib.rs")
    }

    // MARK: - toolSummary: parseArgs edge cases

    func testNilToolArgsProducesEmptyArgs() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "bash",
            toolArgs: nil,
            contentFallback: nil
        )
        // no command -> empty target
        XCTAssertEqual(summary.text, "$ ")
    }

    func testInvalidJsonToolArgsProducesEmptyArgs() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "bash",
            toolArgs: "not json at all",
            contentFallback: nil
        )
        XCTAssertEqual(summary.text, "$ ")
    }

    // MARK: - toolSummary: case insensitivity

    func testToolNameIsCaseInsensitive() {
        let summary = ConversationRenderPolicy.toolSummary(
            toolName: "BASH",
            toolArgs: json(["command": "pwd"]),
            contentFallback: nil
        )
        XCTAssertEqual(summary.icon, "terminal")
        XCTAssertEqual(summary.text, "$ pwd")
    }

    // MARK: - Helpers

    private func json(_ dict: [String: String]) -> String {
        let data = try! JSONSerialization.data(withJSONObject: dict)
        return String(data: data, encoding: .utf8)!
    }

    private func jsonAny(_ dict: [String: Any]) -> String {
        let data = try! JSONSerialization.data(withJSONObject: dict)
        return String(data: data, encoding: .utf8)!
    }
}
