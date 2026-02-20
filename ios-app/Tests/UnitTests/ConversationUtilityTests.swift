import XCTest
@testable import TenexMVP

// MARK: - TodoParser Tests

final class TodoParserTests: XCTestCase {
    func testParseEmptyMessages() {
        let result = TodoParser.parse(messages: [])
        XCTAssertEqual(result.items.count, 0)
        XCTAssertFalse(result.hasTodos)
    }

    func testParseMessagesWithNoTodoToolCalls() {
        let messages = [
            makeMessage(id: "1", content: "Hello world"),
            makeMessage(id: "2", content: "Some other content"),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 0)
        XCTAssertFalse(result.hasTodos)
    }

    func testParseSingleTodoWriteWithPendingItems() {
        let todosJson = """
        {"todos":[{"content":"Write tests","status":"pending"},{"content":"Fix bug","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 2)
        XCTAssertEqual(result.items[0].title, "Write tests")
        XCTAssertEqual(result.items[0].status, .pending)
        XCTAssertEqual(result.items[1].title, "Fix bug")
        XCTAssertEqual(result.items[1].status, .pending)
        XCTAssertEqual(result.completedCount, 0)
        XCTAssertFalse(result.isComplete)
    }

    func testParseTodoWriteWithMixedStatuses() {
        let todosJson = """
        {"todos":[{"content":"Task A","status":"done"},{"content":"Task B","status":"in_progress"},{"content":"Task C","status":"pending"},{"content":"Task D","status":"skipped"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 4)
        XCTAssertEqual(result.items[0].status, .done)
        XCTAssertEqual(result.items[1].status, .inProgress)
        XCTAssertEqual(result.items[2].status, .pending)
        XCTAssertEqual(result.items[3].status, .skipped)
        XCTAssertEqual(result.completedCount, 1)
        XCTAssertEqual(result.inProgressItem?.title, "Task B")
    }

    func testParseTodoWriteCompletedStatusTreatedAsDone() {
        let todosJson = """
        {"todos":[{"content":"Done task","status":"completed"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
        XCTAssertEqual(result.items[0].status, .completed)
        XCTAssertEqual(result.completedCount, 1)
        XCTAssertTrue(result.isComplete)
    }

    func testLaterTodoWriteReplacesEarlierOne() {
        let firstJson = """
        {"todos":[{"content":"Task 1","status":"pending"},{"content":"Task 2","status":"pending"}]}
        """
        let secondJson = """
        {"todos":[{"content":"Task 1","status":"done"},{"content":"Task 2","status":"in_progress"},{"content":"Task 3","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: firstJson),
            makeMessage(id: "2", content: "Intermediate message"),
            makeMessage(id: "3", content: "", toolName: "todo_write", toolArgs: secondJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 3)
        XCTAssertEqual(result.items[0].title, "Task 1")
        XCTAssertEqual(result.items[0].status, .done)
        XCTAssertEqual(result.items[1].title, "Task 2")
        XCTAssertEqual(result.items[1].status, .inProgress)
        XCTAssertEqual(result.items[2].title, "Task 3")
        XCTAssertEqual(result.items[2].status, .pending)
    }

    func testParseMcpWrappedToolName() {
        let todosJson = """
        {"todos":[{"content":"MCP task","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "mcp__tenex__todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
        XCTAssertEqual(result.items[0].title, "MCP task")
    }

    func testParseTodoWriteVariantName() {
        let todosJson = """
        {"todos":[{"content":"Variant task","status":"done"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todowrite", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
        XCTAssertEqual(result.items[0].title, "Variant task")
    }

    func testToolNameMatchingIsCaseInsensitive() {
        let todosJson = """
        {"todos":[{"content":"Upper case","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "TODO_WRITE", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
    }

    func testTodoWithTitleFieldInsteadOfContent() {
        let todosJson = """
        {"todos":[{"title":"Title-based task","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
        XCTAssertEqual(result.items[0].title, "Title-based task")
    }

    func testTodoContentFieldTakesPrecedenceOverTitle() {
        let todosJson = """
        {"todos":[{"content":"Content value","title":"Title value","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].title, "Content value")
    }

    func testTodoWithActiveFormDescription() {
        let todosJson = """
        {"todos":[{"content":"Task","status":"in_progress","activeForm":"Currently working on it"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].description, "Currently working on it")
    }

    func testTodoWithDescriptionFieldFallback() {
        let todosJson = """
        {"todos":[{"content":"Task","status":"pending","description":"Some description"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].description, "Some description")
    }

    func testTodoWithSkipReason() {
        let todosJson = """
        {"todos":[{"content":"Skipped task","status":"skipped","skip_reason":"Not applicable"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].status, .skipped)
        XCTAssertEqual(result.items[0].skipReason, "Not applicable")
    }

    func testEmptyTitleTodoIsSkipped() {
        let todosJson = """
        {"todos":[{"content":"","status":"pending"},{"content":"Valid","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 1)
        XCTAssertEqual(result.items[0].title, "Valid")
    }

    func testInvalidJsonToolArgsAreIgnored() {
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: "not valid json"),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 0)
    }

    func testMissingTodosArrayIsIgnored() {
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: "{\"something\":\"else\"}"),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 0)
    }

    func testToolCallWithNilToolArgsIsIgnored() {
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: nil),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 0)
    }

    func testUnrecognizedToolNameIsIgnored() {
        let todosJson = """
        {"todos":[{"content":"Should not appear","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "fs_read", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items.count, 0)
    }

    func testUnknownStatusDefaultsToPending() {
        let todosJson = """
        {"todos":[{"content":"Unknown status","status":"banana"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].status, .pending)
    }

    func testTodoItemIdsAreSequential() {
        let todosJson = """
        {"todos":[{"content":"A","status":"pending"},{"content":"B","status":"pending"},{"content":"C","status":"pending"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertEqual(result.items[0].id, "todo-0")
        XCTAssertEqual(result.items[1].id, "todo-1")
        XCTAssertEqual(result.items[2].id, "todo-2")
    }

    func testTodoStateIsCompleteWhenAllDone() {
        let todosJson = """
        {"todos":[{"content":"A","status":"done"},{"content":"B","status":"completed"}]}
        """
        let messages = [
            makeMessage(id: "1", content: "", toolName: "todo_write", toolArgs: todosJson),
        ]

        let result = TodoParser.parse(messages: messages)
        XCTAssertTrue(result.isComplete)
        XCTAssertEqual(result.completedCount, 2)
    }
}

// MARK: - LastAgentFinder Tests

final class LastAgentFinderTests: XCTestCase {
    func testEmptyMessagesReturnsNil() {
        let result = LastAgentFinder.findLastAgentPubkey(
            messages: [],
            availableAgents: [makeAgent(pubkey: "agent-1")]
        )
        XCTAssertNil(result)
    }

    func testEmptyAvailableAgentsReturnsNil() {
        let messages = [
            makeMessage(id: "1", content: "Hello", pubkey: "agent-1"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(
            messages: messages,
            availableAgents: []
        )
        XCTAssertNil(result)
    }

    func testFindsLastAgentByTimestamp() {
        let messages = [
            makeMessage(id: "1", content: "First", pubkey: "agent-1", createdAt: 100),
            makeMessage(id: "2", content: "Second", pubkey: "agent-2", createdAt: 200),
            makeMessage(id: "3", content: "Third", pubkey: "agent-1", createdAt: 300),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
            makeAgent(pubkey: "agent-2"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        XCTAssertEqual(result, "agent-1")
    }

    func testIgnoresMessagesFromUnknownPubkeys() {
        let messages = [
            makeMessage(id: "1", content: "Agent msg", pubkey: "agent-1", createdAt: 100),
            makeMessage(id: "2", content: "User msg", pubkey: "user-pubkey", createdAt: 200),
            makeMessage(id: "3", content: "Unknown msg", pubkey: "unknown-agent", createdAt: 300),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        XCTAssertEqual(result, "agent-1")
    }

    func testSingleAgentMessage() {
        let messages = [
            makeMessage(id: "1", content: "Hello", pubkey: "agent-1", createdAt: 100),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        XCTAssertEqual(result, "agent-1")
    }

    func testAllMessagesFromNonAgentsReturnsNil() {
        let messages = [
            makeMessage(id: "1", content: "User msg", pubkey: "user-1", createdAt: 100),
            makeMessage(id: "2", content: "User msg 2", pubkey: "user-2", createdAt: 200),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        XCTAssertNil(result)
    }

    func testEqualTimestampsTakesLastEncountered() {
        let messages = [
            makeMessage(id: "1", content: "First", pubkey: "agent-1", createdAt: 100),
            makeMessage(id: "2", content: "Second", pubkey: "agent-2", createdAt: 100),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
            makeAgent(pubkey: "agent-2"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        // With >= comparison and forward iteration, equal timestamps pick the last in array
        XCTAssertEqual(result, "agent-2")
    }

    func testToolCallMessagesStillCountForAgentLookup() {
        // LastAgentFinder does NOT filter out tool call messages - it checks all messages
        let messages = [
            makeMessage(id: "1", content: "Hello", pubkey: "agent-1", createdAt: 100),
            makeMessage(id: "2", content: "", pubkey: "agent-2", createdAt: 200, toolName: "fs_read"),
        ]
        let agents = [
            makeAgent(pubkey: "agent-1"),
            makeAgent(pubkey: "agent-2"),
        ]

        let result = LastAgentFinder.findLastAgentPubkey(messages: messages, availableAgents: agents)
        XCTAssertEqual(result, "agent-2")
    }
}

// MARK: - Test Helpers

private func makeMessage(
    id: String,
    content: String,
    pubkey: String = "default-pubkey",
    createdAt: UInt64 = 1000,
    toolName: String? = nil,
    toolArgs: String? = nil
) -> Message {
    Message(
        id: id,
        content: content,
        pubkey: pubkey,
        threadId: "thread-1",
        createdAt: createdAt,
        replyTo: nil,
        isReasoning: false,
        askEvent: nil,
        qTags: [],
        aTags: [],
        pTags: [],
        toolName: toolName,
        toolArgs: toolArgs,
        llmMetadata: [:],
        delegationTag: nil,
        branch: nil
    )
}

private func makeAgent(pubkey: String, name: String = "Test Agent") -> ProjectAgent {
    ProjectAgent(
        pubkey: pubkey,
        name: name,
        isPm: false,
        model: nil,
        tools: []
    )
}
