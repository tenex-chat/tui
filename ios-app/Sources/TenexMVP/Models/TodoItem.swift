import Foundation

// MARK: - Todo Models

/// Status of a todo item
enum TodoStatus: String, Codable {
    case pending
    case inProgress = "in_progress"
    case done
    case completed // Treat completed as done
    case skipped

    var displayName: String {
        switch self {
        case .pending: return "Pending"
        case .inProgress: return "In Progress"
        case .done, .completed: return "Done"
        case .skipped: return "Skipped"
        }
    }
}

/// A single todo item
struct TodoItem: Identifiable {
    let id: String
    let title: String
    let description: String?
    let status: TodoStatus
    let skipReason: String?
}

/// Todo list state
struct TodoState {
    let items: [TodoItem]

    var hasTodos: Bool {
        !items.isEmpty
    }

    var completedCount: Int {
        items.filter { $0.status == .done || $0.status == .completed }.count
    }

    var inProgressItem: TodoItem? {
        items.first { $0.status == .inProgress }
    }

    var isComplete: Bool {
        !items.isEmpty && completedCount == items.count
    }
}

/// Aggregate todo statistics across a conversation tree
struct AggregateTodoStats {
    var completedCount: Int
    var totalCount: Int

    var isComplete: Bool { totalCount > 0 && completedCount == totalCount }
    var hasTodos: Bool { totalCount > 0 }

    static let empty = AggregateTodoStats(completedCount: 0, totalCount: 0)

    mutating func add(_ other: AggregateTodoStats) {
        completedCount += other.completedCount
        totalCount += other.totalCount
    }

    mutating func add(_ state: TodoState) {
        completedCount += state.completedCount
        totalCount += state.items.count
    }
}

// MARK: - Todo Parsing

/// Parses todo_write tool calls from messages
enum TodoParser {
    /// Parse todos from a list of messages
    static func parse(messages: [MessageInfo]) -> TodoState {
        var items: [TodoItem] = []
        var idCounter = 0

        for message in messages {
            // Check if this is a todo_write tool call
            guard let toolName = message.toolName?.lowercased(),
                  toolName == "todo_write" || toolName == "todowrite",
                  let toolArgs = message.toolArgs else {
                continue
            }

            // Parse the tool args JSON
            guard let jsonData = toolArgs.data(using: .utf8),
                  let json = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
                  let todosArray = json["todos"] as? [[String: Any]] else {
                continue
            }

            // todo_write replaces the entire list
            items.removeAll()
            idCounter = 0

            for todoDict in todosArray {
                // Extract title (content or title field)
                let title = (todoDict["content"] as? String) ?? (todoDict["title"] as? String) ?? ""

                guard !title.isEmpty else { continue }

                // Extract status
                let statusStr = todoDict["status"] as? String ?? "pending"
                let status = TodoStatus(rawValue: statusStr.lowercased()) ?? .pending

                // Extract description (activeForm or description field)
                let description = (todoDict["activeForm"] as? String) ?? (todoDict["description"] as? String)

                // Extract skip reason
                let skipReason = todoDict["skip_reason"] as? String

                items.append(TodoItem(
                    id: "todo-\(idCounter)",
                    title: title,
                    description: description,
                    status: status,
                    skipReason: skipReason
                ))

                idCounter += 1
            }
        }

        return TodoState(items: items)
    }
}
