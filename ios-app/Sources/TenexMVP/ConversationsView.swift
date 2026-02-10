import SwiftUI

struct ConversationsView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedConversation: ConversationFullInfo?
    @State private var showReports = false
    @State private var showNewConversation = false
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    /// Conversations filtered by this project from the centralized store
    private var projectConversations: [ConversationFullInfo] {
        coreManager.conversations.filter { conv in
            // projectATag is in a-tag format "kind:pubkey:d-tag", extract d-tag to match project.id
            let projectId = conv.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
            return projectId == project.id
        }
    }

    /// Root conversations (no parent or orphaned) sorted by effective last activity
    private var rootConversations: [ConversationFullInfo] {
        let allIds = Set(projectConversations.map { $0.id })
        return projectConversations
            .filter { conv in
                if let parentId = conv.parentId {
                    return !allIds.contains(parentId)
                }
                return true
            }
            .sorted { $0.effectiveLastActivity > $1.effectiveLastActivity }
    }

    var body: some View {
        Group {
            if rootConversations.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Conversations")
                        .font(.title2)
                        .fontWeight(.semibold)
                    Text("Conversations will appear automatically")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            } else {
                List {
                    // Root conversations sorted by effective last activity
                    ForEach(rootConversations, id: \.id) { conversation in
                        ProjectConversationRow(
                            conversation: conversation,
                            onSelect: { selected in
                                #if os(macOS)
                                openWindow(id: "conversation-summary", value: selected.id)
                                #else
                                selectedConversation = selected
                                #endif
                            }
                        )
                    }
                }
                .listStyle(.plain)
                .refreshable {
                    await coreManager.manualRefresh()
                }
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
                Button(action: { showNewConversation = true }) {
                    Image(systemName: "plus")
                }
            }
        }
        #if os(iOS)
        .sheet(item: $selectedConversation) { conversation in
            NavigationStack {
                ConversationDetailView(conversation: conversation)
                    .environmentObject(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") {
                                selectedConversation = nil
                            }
                        }
                    }
            }
            .presentationDetents([.large])
            .presentationDragIndicator(.visible)
        }
        #endif
        .sheet(isPresented: $showNewConversation) {
            NavigationStack {
                MessageComposerView(
                    project: project,
                    onSend: { _ in
                        // Data will auto-refresh via push-based updates
                    }
                )
                .environmentObject(coreManager)
            }
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
}

// MARK: - Project Conversation Row (Uses ConversationFullInfo)

/// Conversation row for project-specific view using ConversationFullInfo
private struct ProjectConversationRow: View {
    let conversation: ConversationFullInfo
    let onSelect: (ConversationFullInfo) -> Void

    private var statusColor: Color {
        if conversation.isActive { return .green }
        switch conversation.status?.lowercased() ?? "" {
        case "active", "in progress": return .green
        case "waiting", "blocked": return .orange
        case "completed", "done": return .gray
        default: return .blue
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            // Status indicator with activity pulse
            ZStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)

                if conversation.isActive {
                    Circle()
                        .stroke(statusColor.opacity(0.5), lineWidth: 2)
                        .frame(width: 16, height: 16)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(conversation.effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary or current activity
                HStack(alignment: .top) {
                    if let activity = conversation.currentActivity, conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "bolt.fill")
                                .font(.caption2)
                                .foregroundStyle(.orange)
                            Text(activity)
                                .font(.subheadline)
                                .foregroundStyle(.orange)
                                .lineLimit(1)
                        }
                    } else if let summary = conversation.summary {
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

                    // Show message count
                    if conversation.messageCount > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "bubble.left")
                                .font(.caption2)
                            Text("\(conversation.messageCount)")
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                // Row 3: Author avatar and status badge
                HStack(spacing: -8) {
                    SharedAgentAvatar(agentName: conversation.author)

                    Spacer()

                    // Status badge
                    if let status = conversation.status {
                        Text(status)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(statusColor.opacity(0.15))
                            .foregroundStyle(statusColor)
                            .clipShape(Capsule())
                    }
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
}

// MARK: - Optimized Conversation Row (Uses precomputed hierarchy data)

/// Conversation row that uses precomputed hierarchy data for O(1) access
/// instead of recomputing O(n²) BFS on every render
private struct OptimizedConversationRow: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let conversation: ConversationInfo
    let aggregatedData: AggregatedConversationData
    let onSelect: (ConversationInfo) -> Void

    var body: some View {
        HStack(spacing: 12) {
            // Status indicator
            Circle()
                .fill(conversationStatusColor(for: conversation.status))
                .frame(width: 10, height: 10)

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(aggregatedData.effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary and activity span (renamed from "total running time")
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

                    // Show activity span (time from earliest to latest activity)
                    if aggregatedData.activitySpan > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.caption2)
                            Text(ConversationFormatters.formatDuration(aggregatedData.activitySpan))
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                // Row 3: Author avatar (standalone) + delegation agents (overlapping)
                HStack(spacing: 0) {
                    // Author who started the conversation (standalone)
                    if let authorInfo = aggregatedData.authorInfo {
                        AgentAvatarView(agentName: authorInfo.name, pubkey: authorInfo.pubkey)
                            .environmentObject(coreManager)
                    }

                    // Gap between author and delegation agents
                    if !aggregatedData.delegationAgentInfos.isEmpty {
                        Spacer()
                            .frame(width: 12)

                        // Delegation agents (overlapping)
                        HStack(spacing: -8) {
                            ForEach(aggregatedData.delegationAgentInfos.prefix(maxVisibleAvatars - 1)) { agentInfo in
                                AgentAvatarView(agentName: agentInfo.name, pubkey: agentInfo.pubkey)
                                    .environmentObject(coreManager)
                            }

                            if aggregatedData.delegationAgentInfos.count > maxVisibleAvatars - 1 {
                                Circle()
                                    .fill(Color.systemGray4)
                                    .frame(width: 24, height: 24)
                                    .overlay {
                                        Text("+\(aggregatedData.delegationAgentInfos.count - (maxVisibleAvatars - 1))")
                                            .font(.caption2)
                                            .fontWeight(.medium)
                                            .foregroundStyle(.secondary)
                                    }
                            }
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
}

// Note: AgentAvatarView removed - use SharedAgentAvatar from ConversationHierarchy.swift

// MARK: - Messages View

struct MessagesView: View {
    let conversation: ConversationInfo
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @State private var showReplyComposer = false
    @State private var availableAgents: [OnlineAgentInfo] = []
    @Environment(\.dismiss) private var dismiss

    /// Find the last agent that spoke in the conversation
    private var lastAgentPubkey: String? {
        let agentPubkeys = Set(availableAgents.map { $0.pubkey })
        var latestAgentPubkey: String?
        var latestTimestamp: UInt64 = 0

        for msg in messages {
            if msg.role == "user" { continue }
            if agentPubkeys.contains(msg.authorNpub) && msg.createdAt >= latestTimestamp {
                latestTimestamp = msg.createdAt
                latestAgentPubkey = msg.authorNpub
            }
        }
        return latestAgentPubkey
    }

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
                    Button(action: { Task { await coreManager.syncNow(); await loadMessages() } }) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(isLoading)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .task {
                await loadMessages()
                await loadAgents()
            }
            .onReceive(coreManager.$messagesByConversation) { cache in
                if let updated = cache[conversation.id] {
                    messages = updated
                }
            }
            .onReceive(coreManager.$onlineAgents) { cache in
                availableAgents = cache[project.id] ?? []
            }
            .sheet(isPresented: $showReplyComposer) {
                NavigationStack {
                    MessageComposerView(
                        project: project,
                        conversationId: conversation.id,
                        conversationTitle: conversation.title,
                        initialAgentPubkey: lastAgentPubkey,
                        onSend: { _ in }
                    )
                    .environmentObject(coreManager)
                }
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
            .background(Color.systemGray6)
        }
        .buttonStyle(.plain)
    }

    private func loadMessages() async {
        isLoading = true
        await coreManager.ensureMessagesLoaded(conversationId: conversation.id)
        messages = coreManager.messagesByConversation[conversation.id] ?? []
        isLoading = false
    }

    private func loadAgents() async {
        availableAgents = coreManager.onlineAgents[project.id] ?? []
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

                    Text(ConversationFormatters.formatRelativeTime(message.createdAt))
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
                    .background(isUser ? Color.blue.opacity(0.15) : Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 16))

                // Ask event component (inline ask)
                if let askEvent = message.askEvent {
                    AskEventView(askEvent: askEvent)
                }
            }

            if !isUser { Spacer(minLength: 50) }
        }
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
