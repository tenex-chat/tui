import SwiftUI

struct ConversationsView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var conversations: [ConversationInfo] = []
    @State private var isLoading = false
    @State private var selectedConversation: ConversationInfo?
    @State private var showReports = false

    var body: some View {
        Group {
            if isLoading {
                ProgressView("Loading conversations...")
            } else if conversations.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Conversations")
                        .font(.title2)
                        .fontWeight(.semibold)
                }
            } else {
                List {
                    // Root conversations (no parent)
                    let rootConversations = conversations.filter { $0.parentId == nil }
                    ForEach(rootConversations, id: \.id) { conversation in
                        ConversationTreeNode(
                            conversation: conversation,
                            allConversations: conversations,
                            depth: 0,
                            onSelect: { selected in
                                selectedConversation = selected
                            }
                        )
                    }
                }
                .listStyle(.plain)
            }
        }
        .navigationTitle(project.title)
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button(action: { showReports = true }) {
                    Label("Reports", systemImage: "doc.richtext")
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button(action: loadConversations) {
                    Image(systemName: "arrow.clockwise")
                }
                .disabled(isLoading)
            }
        }
        .onAppear {
            loadConversations()
        }
        .sheet(item: $selectedConversation) { conversation in
            MessagesView(conversation: conversation)
                .environmentObject(coreManager)
        }
        .sheet(isPresented: $showReports) {
            NavigationStack {
                ReportsView(project: project)
                    .environmentObject(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            Button("Done") { showReports = false }
                        }
                    }
            }
        }
    }

    private func loadConversations() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            let fetched = coreManager.core.getConversations(projectId: project.id)
            DispatchQueue.main.async {
                self.conversations = fetched
                self.isLoading = false
            }
        }
    }
}

// MARK: - Conversation Tree Node (Recursive)
// Uses separate tap areas to avoid nested Button issues (gestures conflict)

struct ConversationTreeNode: View {
    let conversation: ConversationInfo
    let allConversations: [ConversationInfo]
    let depth: Int
    let onSelect: (ConversationInfo) -> Void

    @State private var isExpanded = true

    private var children: [ConversationInfo] {
        allConversations.filter { $0.parentId == conversation.id }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Conversation row - use HStack with separate tap targets
            HStack(spacing: 12) {
                // Expand/collapse button (separate from main content)
                if !children.isEmpty {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 20, height: 44) // Make tap target larger
                        .contentShape(Rectangle())
                        .onTapGesture {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                isExpanded.toggle()
                            }
                        }
                } else {
                    Spacer().frame(width: 20)
                }

                // Main conversation content - tappable to view details
                HStack(spacing: 12) {
                    // Status indicator
                    Circle()
                        .fill(statusColor)
                        .frame(width: 10, height: 10)

                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(conversation.title)
                                .font(.headline)
                                .lineLimit(1)

                            Spacer()

                            Text("\(conversation.messageCount)")
                                .font(.caption)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 2)
                                .background(Color(.systemGray5))
                                .clipShape(Capsule())
                        }

                        HStack {
                            Text(conversation.author)
                                .font(.caption)
                                .foregroundStyle(.secondary)

                            if let summary = conversation.summary {
                                Text("• \(summary)")
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                                    .lineLimit(1)
                            }
                        }
                    }

                    Image(systemName: "chevron.right")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
                .contentShape(Rectangle())
                .onTapGesture {
                    onSelect(conversation)
                }
            }
            .padding(.vertical, 10)
            .padding(.leading, CGFloat(depth * 16))

            // Children (nested conversations)
            if isExpanded {
                ForEach(children, id: \.id) { child in
                    ConversationTreeNode(
                        conversation: child,
                        allConversations: allConversations,
                        depth: depth + 1,
                        onSelect: onSelect
                    )
                }
            }
        }
    }

    private var statusColor: Color {
        switch conversation.status {
        case "active": return .green
        case "waiting": return .orange
        case "completed": return .gray
        default: return .blue
        }
    }
}

// MARK: - Messages View

struct MessagesView: View {
    let conversation: ConversationInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(messages, id: \.id) { message in
                        MessageBubble(message: message)
                    }
                }
                .padding()
            }
            .navigationTitle(conversation.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                loadMessages()
            }
        }
    }

    private func loadMessages() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            let fetched = coreManager.core.getMessages(conversationId: conversation.id)
            DispatchQueue.main.async {
                self.messages = fetched
                self.isLoading = false
            }
        }
    }
}

// MARK: - Message Bubble

struct MessageBubble: View {
    let message: MessageInfo

    private var isUser: Bool {
        message.role == "user"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if isUser { Spacer(minLength: 50) }

            VStack(alignment: isUser ? .trailing : .leading, spacing: 4) {
                // Author header
                HStack(spacing: 6) {
                    if !isUser {
                        Circle()
                            .fill(Color.blue.gradient)
                            .frame(width: 24, height: 24)
                            .overlay {
                                Image(systemName: "sparkle")
                                    .font(.caption2)
                                    .foregroundStyle(.white)
                            }
                    }

                    Text(message.author)
                        .font(.caption)
                        .fontWeight(.medium)
                        .foregroundStyle(.secondary)

                    Text(formatTimestamp(message.createdAt))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)

                    if isUser {
                        Circle()
                            .fill(Color.green.gradient)
                            .frame(width: 24, height: 24)
                            .overlay {
                                Image(systemName: "person.fill")
                                    .font(.caption2)
                                    .foregroundStyle(.white)
                            }
                    }
                }

                // Message content
                Text(message.content)
                    .font(.body)
                    .padding(12)
                    .background(isUser ? Color.blue.opacity(0.15) : Color(.systemGray6))
                    .clipShape(RoundedRectangle(cornerRadius: 16))

                // Ask event component (inline ask)
                if let askEvent = message.askEvent {
                    AskEventView(askEvent: askEvent)
                }
            }

            if !isUser { Spacer(minLength: 50) }
        }
    }

    private func formatTimestamp(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}

// MARK: - Ask Event View

struct AskEventView: View {
    let askEvent: AskEventInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Title and context
            if let title = askEvent.title {
                HStack(spacing: 8) {
                    Image(systemName: "questionmark.circle.fill")
                        .foregroundStyle(.orange)
                    Text(title)
                        .font(.headline)
                }
            }

            if !askEvent.context.isEmpty {
                Text(askEvent.context)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            // Questions
            ForEach(Array(askEvent.questions.enumerated()), id: \.offset) { _, question in
                AskQuestionView(question: question)
            }
        }
        .padding(16)
        .background(Color.orange.opacity(0.1))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.orange.opacity(0.3), lineWidth: 1)
        )
    }
}

// MARK: - Ask Question View

struct AskQuestionView: View {
    let question: AskQuestionInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Title
            Text(questionTitle)
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.orange)

            // Question text
            Text(questionText)
                .font(.body)

            // Choices (unified term for suggestions/options)
            if !choices.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(choices.enumerated()), id: \.offset) { _, choice in
                        ChoiceRow(choice: choice, isMultiSelect: isMultiSelect)
                    }
                }
            }
        }
    }

    // MARK: - Computed Properties

    private var questionTitle: String {
        switch question {
        case .singleSelect(let title, _, _): return title
        case .multiSelect(let title, _, _): return title
        }
    }

    private var questionText: String {
        switch question {
        case .singleSelect(_, let text, _): return text
        case .multiSelect(_, let text, _): return text
        }
    }

    private var choices: [String] {
        switch question {
        case .singleSelect(_, _, let suggestions): return suggestions
        case .multiSelect(_, _, let options): return options
        }
    }

    private var isMultiSelect: Bool {
        switch question {
        case .singleSelect: return false
        case .multiSelect: return true
        }
    }
}

// MARK: - Choice Row View

struct ChoiceRow: View {
    let choice: String
    let isMultiSelect: Bool

    var body: some View {
        HStack(spacing: 8) {
            indicator
            Text(choice)
                .font(.subheadline)
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private var indicator: some View {
        if isMultiSelect {
            RoundedRectangle(cornerRadius: 3)
                .stroke(Color.orange, lineWidth: 2)
                .frame(width: 16, height: 16)
        } else {
            Circle()
                .stroke(Color.orange, lineWidth: 2)
                .frame(width: 16, height: 16)
        }
    }
}

// MARK: - ConversationInfo Identifiable

extension ConversationInfo: Identifiable {}
