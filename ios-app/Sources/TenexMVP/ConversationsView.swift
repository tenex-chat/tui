import SwiftUI

struct ConversationsView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var conversations: [ConversationInfo] = []
    @State private var isLoading = false
    @State private var selectedConversation: ConversationInfo?
    @State private var showReports = false
    @State private var showNewConversation = false

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
                    // Root conversations (no parent) - only show top-level with aggregated nested data
                    let rootConversations = conversations.filter { $0.parentId == nil }
                    ForEach(rootConversations, id: \.id) { conversation in
                        HierarchyConversationRow(
                            conversation: conversation,
                            allConversations: conversations,
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
                HStack(spacing: 16) {
                    Button(action: loadConversations) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(isLoading)

                    Button(action: { showNewConversation = true }) {
                        Image(systemName: "plus")
                    }
                }
            }
        }
        .onAppear {
            loadConversations()
        }
        .sheet(item: $selectedConversation) { conversation in
            MessagesView(conversation: conversation, project: project)
                .environmentObject(coreManager)
        }
        .sheet(isPresented: $showNewConversation) {
            MessageComposerView(
                project: project,
                onSend: { _ in
                    // Refresh conversations after sending
                    loadConversations()
                }
            )
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

// MARK: - Hierarchy Conversation Row (Shows aggregated info from nested conversations)

private struct HierarchyConversationRow: View {
    let conversation: ConversationInfo
    let allConversations: [ConversationInfo]
    let onSelect: (ConversationInfo) -> Void

    /// All descendants of this conversation (children, grandchildren, etc.)
    private var allDescendants: [ConversationInfo] {
        var descendants: [ConversationInfo] = []
        var queue = allConversations.filter { $0.parentId == conversation.id }

        while !queue.isEmpty {
            let current = queue.removeFirst()
            descendants.append(current)
            let children = allConversations.filter { $0.parentId == current.id }
            queue.append(contentsOf: children)
        }

        return descendants
    }

    /// Effective last activity (max across all nested conversations)
    private var effectiveLastActivity: UInt64 {
        let allActivities = [conversation.lastActivity] + allDescendants.map { $0.lastActivity }
        return allActivities.max() ?? conversation.lastActivity
    }

    /// Total running time across all nested conversations
    private var totalRunningTime: TimeInterval {
        let allTimestamps = [conversation.lastActivity] + allDescendants.map { $0.lastActivity }
        guard let earliest = allTimestamps.min(),
              let latest = allTimestamps.max() else {
            return 0
        }
        return TimeInterval(latest - earliest)
    }

    /// All unique participating agents (including from nested conversations)
    private var participatingAgents: [String] {
        var agents = Set<String>()
        agents.insert(conversation.author)
        for descendant in allDescendants {
            agents.insert(descendant.author)
        }
        return agents.sorted()
    }

    /// Status color based on conversation status
    private var statusColor: Color {
        switch conversation.status {
        case "active": return .green
        case "waiting": return .orange
        case "completed": return .gray
        default: return .blue
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            // Status indicator
            Circle()
                .fill(statusColor)
                .frame(width: 10, height: 10)

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    Spacer()

                    Text(formatRelativeTime(effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary and total running time
                HStack(alignment: .top) {
                    if let summary = conversation.summary {
                        Text(summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    } else {
                        Text("No summary")
                            .font(.subheadline)
                            .foregroundStyle(.tertiary)
                            .italic()
                    }

                    Spacer()

                    if totalRunningTime > 0 {
                        Text(formatDuration(totalRunningTime))
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                }

                // Row 3: Participating agent avatars
                HStack(spacing: -8) {
                    ForEach(participatingAgents.prefix(6), id: \.self) { agent in
                        AgentAvatarView(agentName: agent)
                    }

                    if participatingAgents.count > 6 {
                        Circle()
                            .fill(Color(.systemGray4))
                            .frame(width: 24, height: 24)
                            .overlay {
                                Text("+\(participatingAgents.count - 6)")
                                    .font(.caption2)
                                    .fontWeight(.medium)
                                    .foregroundStyle(.secondary)
                            }
                    }

                    Spacer()
                }
            }

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
        .onTapGesture {
            onSelect(conversation)
        }
    }

    private func formatRelativeTime(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let hours = Int(seconds) / 3600
        let minutes = (Int(seconds) % 3600) / 60

        if hours > 0 {
            return "\(hours)h \(minutes)m"
        } else if minutes > 0 {
            return "\(minutes)m"
        } else {
            return "<1m"
        }
    }
}

// MARK: - Agent Avatar View

private struct AgentAvatarView: View {
    let agentName: String

    private var avatarColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan, .mint]
        let hash = agentName.hashValue
        return colors[abs(hash) % colors.count]
    }

    private var initials: String {
        let parts = agentName.split(separator: "-")
        if parts.count >= 2 {
            return String(parts.prefix(2).compactMap { $0.first }.map { String($0).uppercased() }.joined())
        } else if let first = agentName.first {
            let chars = agentName.prefix(2)
            return String(chars).uppercased()
        }
        return "?"
    }

    var body: some View {
        Circle()
            .fill(avatarColor.gradient)
            .frame(width: 24, height: 24)
            .overlay {
                Text(initials)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .overlay {
                Circle()
                    .stroke(Color(.systemBackground), lineWidth: 2)
            }
    }
}

// MARK: - Messages View

struct MessagesView: View {
    let conversation: ConversationInfo
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @State private var showReplyComposer = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Messages list
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(messages, id: \.id) { message in
                            MessageBubble(message: message)
                        }
                    }
                    .padding()
                }

                Divider()

                // Reply button
                replyButton
            }
            .navigationTitle(conversation.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(action: loadMessages) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(isLoading)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                loadMessages()
            }
            .sheet(isPresented: $showReplyComposer) {
                MessageComposerView(
                    project: project,
                    conversationId: conversation.id,
                    conversationTitle: conversation.title,
                    onSend: { _ in
                        // Refresh messages after sending
                        loadMessages()
                    }
                )
                .environmentObject(coreManager)
            }
        }
    }

    private var replyButton: some View {
        Button(action: { showReplyComposer = true }) {
            HStack {
                Image(systemName: "text.bubble")
                Text("Reply to this conversation")
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .background(Color(.systemGray6))
        }
        .buttonStyle(.plain)
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
