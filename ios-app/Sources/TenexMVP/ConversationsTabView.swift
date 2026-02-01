import SwiftUI
import CryptoKit

/// Main tab view for Conversations - shows aggregated conversation tree from all/filtered projects
/// with a project filter button in the toolbar
struct ConversationsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedProjectIds: Set<String> = []  // Empty means show all
    @State private var showFilterSheet = false
    @State private var showDiagnostics = false
    @State private var showStats = false
    @State private var showArchived = false
    @State private var selectedConversation: ConversationFullInfo?
    @State private var runtimeText: String = "0m"
    @State private var showProjectPickerForNewConv = false
    @State private var projectForNewConversation: ProjectInfo?
    @State private var showNewConversation = false

    /// Updates the runtime text from SafeTenexCore
    private func updateRuntime() async {
        let totalMs = await coreManager.safeCore.getTodayRuntimeMs()
        let totalMinutes = Double(totalMs) / 60_000.0
        if totalMinutes >= 60.0 {
            // Show hours with 2 decimal places (e.g., "1.35h")
            let hours = totalMinutes / 60.0
            runtimeText = String(format: "%.2fh", hours)
        } else {
            // Show minutes as integer (e.g., "42m")
            runtimeText = "\(Int(totalMinutes))m"
        }
    }

    /// Filtered conversations based on selected projects and archived status
    private var filteredConversations: [ConversationFullInfo] {
        var conversations = coreManager.conversations

        // Filter by archived status
        if !showArchived {
            conversations = conversations.filter { !$0.isArchived }
        }

        // Filter by selected projects
        if !selectedProjectIds.isEmpty {
            conversations = conversations.filter { conv in
                // projectATag is in a-tag format "kind:pubkey:d-tag", extract d-tag to match project.id
                let projectId = conv.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
                return selectedProjectIds.contains(projectId)
            }
        }

        return conversations
    }

    /// Root conversations (no parent or orphaned) sorted by effective last activity
    private var rootConversations: [ConversationFullInfo] {
        let allIds = Set(filteredConversations.map { $0.id })
        return filteredConversations
            .filter { conv in
                // Root if no parent or parent doesn't exist in our set
                if let parentId = conv.parentId {
                    return !allIds.contains(parentId)
                }
                return true
            }
            .sorted { $0.effectiveLastActivity > $1.effectiveLastActivity }
    }

    /// Text for the filter button
    private var filterButtonLabel: String {
        if selectedProjectIds.isEmpty {
            return "All Projects"
        } else if selectedProjectIds.count == 1 {
            return coreManager.projects.first { $0.id == selectedProjectIds.first }?.title ?? "1 Project"
        } else {
            return "\(selectedProjectIds.count) Projects"
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if rootConversations.isEmpty {
                    ConversationsEmptyState(
                        hasFilter: !selectedProjectIds.isEmpty,
                        onClearFilter: { selectedProjectIds.removeAll() }
                    )
                } else {
                    List {
                        ForEach(rootConversations, id: \.id) { conversation in
                            ConversationRowFull(
                                conversation: conversation,
                                projectTitle: projectTitle(for: conversation),
                                onSelect: { selected in
                                    selectedConversation = selected
                                }
                            )
                            .environmentObject(coreManager)
                            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                Button(role: .destructive) {
                                    // Archive action placeholder
                                } label: {
                                    Label("Archive", systemImage: "archivebox")
                                }
                            }
                        }
                    }
                    .listStyle(.plain)
                    .refreshable {
                        await coreManager.manualRefresh()
                    }
                }
            }
            .navigationTitle("Conversations")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(action: { showFilterSheet = true }) {
                        Label(filterButtonLabel, systemImage: selectedProjectIds.isEmpty ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    HStack(spacing: 12) {
                        Button(action: { showStats = true }) {
                            Text(runtimeText)
                                .font(.subheadline)
                                .fontWeight(.medium)
                                .foregroundStyle(.secondary)
                        }
                        Menu {
                            Toggle(isOn: $showArchived) {
                                Label("Show Archived", systemImage: "archivebox")
                            }

                            Divider()

                            Button(action: { showDiagnostics = true }) {
                                Label("Diagnostics", systemImage: "gauge.with.needle")
                            }
                        } label: {
                            Image(systemName: "person.circle")
                                .font(.title3)
                        }
                    }
                }
            }
            .sheet(isPresented: $showFilterSheet) {
                ProjectFilterSheet(
                    projects: coreManager.projects,
                    selectedProjectIds: $selectedProjectIds
                )
            }
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
            .sheet(isPresented: $showDiagnostics) {
                DiagnosticsView(coreManager: coreManager)
            }
            .sheet(isPresented: $showStats) {
                StatsView(coreManager: coreManager)
            }
            .sheet(isPresented: $showProjectPickerForNewConv) {
                ProjectSelectorSheet(
                    projects: coreManager.projects,
                    selectedProject: $projectForNewConversation,
                    onDone: {
                        if projectForNewConversation != nil {
                            showProjectPickerForNewConv = false
                            showNewConversation = true
                        }
                    }
                )
            }
            .sheet(isPresented: $showNewConversation) {
                if let project = projectForNewConversation {
                    MessageComposerView(project: project)
                        .environmentObject(coreManager)
                }
            }
            .task {
                await updateRuntime()
            }
            .onChange(of: coreManager.conversations) { _, _ in
                Task { await updateRuntime() }
            }
        }
    }

    private func projectTitle(for conversation: ConversationFullInfo) -> String? {
        // projectATag is in a-tag format "kind:pubkey:d-tag", extract d-tag to match project.id
        let projectId = conversation.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
        return coreManager.projects.first { $0.id == projectId }?.title
    }
}

// MARK: - Conversation Row for ConversationFullInfo

/// Conversation row that uses ConversationFullInfo's rich data
private struct ConversationRowFull: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let conversation: ConversationFullInfo
    let projectTitle: String?
    let onSelect: (ConversationFullInfo) -> Void

    /// Delegation agent infos loaded from descendants
    @State private var delegationAgentInfos: [AgentAvatarInfo] = []

    /// P-tagged recipient info (first p-tag from conversation root event)
    @State private var pTaggedRecipientInfo: AgentAvatarInfo?

    /// Load delegation agent infos by finding all descendants
    private func loadDelegationAgentInfos() async {
        // Load p-tagged recipient (first p-tag from conversation root event)
        if let pTaggedPubkey = conversation.pTags.first {
            let name = await coreManager.safeCore.getProfileName(pubkey: pTaggedPubkey)
            pTaggedRecipientInfo = AgentAvatarInfo(name: name, pubkey: pTaggedPubkey)
        }

        // Get all descendants of this conversation
        let descendantIds = await coreManager.safeCore.getDescendantConversationIds(conversationId: conversation.id)
        let descendants = await coreManager.safeCore.getConversationsByIds(conversationIds: descendantIds)

        // Collect unique agents from descendants (excluding the conversation author and p-tagged recipient)
        let pTaggedPubkey = conversation.pTags.first
        var agentsByPubkey: [String: AgentAvatarInfo] = [:]
        for descendant in descendants {
            if descendant.authorPubkey != conversation.authorPubkey && descendant.authorPubkey != pTaggedPubkey {
                agentsByPubkey[descendant.authorPubkey] = AgentAvatarInfo(
                    name: descendant.author,
                    pubkey: descendant.authorPubkey
                )
            }
        }

        delegationAgentInfos = agentsByPubkey.values.sorted { $0.name < $1.name }
    }

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

                // Row 3: Avatars (author + p-tagged overlapping, then delegation agents) + badges
                HStack(spacing: 0) {
                    ConversationAvatarGroup(
                        authorInfo: AgentAvatarInfo(name: conversation.author, pubkey: conversation.authorPubkey),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        otherParticipants: delegationAgentInfos,
                        maxVisibleAvatars: maxVisibleAvatars
                    )
                    .environmentObject(coreManager)

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

                    // Show project title badge if available
                    if let projectTitle = projectTitle {
                        Text(projectTitle)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.blue.opacity(0.15))
                            .foregroundStyle(.blue)
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
        .task {
            await loadDelegationAgentInfos()
        }
    }
}

// MARK: - Project Filter Sheet

private struct ProjectFilterSheet: View {
    let projects: [ProjectInfo]
    @Binding var selectedProjectIds: Set<String>
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                // "All Projects" option
                Button(action: {
                    selectedProjectIds.removeAll()
                }) {
                    HStack {
                        Image(systemName: "square.grid.2x2")
                            .foregroundStyle(.blue)
                            .frame(width: 24)
                        Text("All Projects")
                            .foregroundStyle(.primary)
                        Spacer()
                        if selectedProjectIds.isEmpty {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.blue)
                        }
                    }
                }

                Divider()

                // Individual projects
                ForEach(projects, id: \.id) { project in
                    Button(action: {
                        toggleProject(project.id)
                    }) {
                        HStack {
                            RoundedRectangle(cornerRadius: 6)
                                .fill(projectColor(for: project).gradient)
                                .frame(width: 24, height: 24)
                                .overlay {
                                    Image(systemName: "folder.fill")
                                        .foregroundStyle(.white)
                                        .font(.caption)
                                }

                            Text(project.title)
                                .foregroundStyle(.primary)
                                .lineLimit(1)

                            Spacer()

                            if selectedProjectIds.contains(project.id) {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.blue)
                            }
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Filter Projects")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func toggleProject(_ id: String) {
        if selectedProjectIds.contains(id) {
            selectedProjectIds.remove(id)
        } else {
            selectedProjectIds.insert(id)
        }
    }

    /// Deterministic color using shared utility (stable across app launches)
    private func projectColor(for project: ProjectInfo) -> Color {
        deterministicColor(for: project.id)
    }
}

// MARK: - Empty State

private struct ConversationsEmptyState: View {
    let hasFilter: Bool
    let onClearFilter: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: hasFilter ? "line.3.horizontal.decrease.circle" : "bubble.left.and.bubble.right")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(hasFilter ? "No Matching Conversations" : "No Conversations")
                .font(.title2)
                .fontWeight(.semibold)

            Text(hasFilter ? "Try adjusting your project filter" : "Conversations will appear automatically")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if hasFilter {
                Button(action: onClearFilter) {
                    Label("Clear Filter", systemImage: "xmark.circle")
                }
                .buttonStyle(.bordered)
                .padding(.top, 8)
            }
        }
        .padding()
    }
}

// MARK: - Messages Sheet View (for viewing conversation details)

private struct MessagesSheetView: View {
    let conversation: ConversationInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if isLoading {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                            .padding()
                    } else if messages.isEmpty {
                        VStack(spacing: 12) {
                            Image(systemName: "bubble.left")
                                .font(.system(size: 40))
                                .foregroundStyle(.secondary)
                            Text("No messages yet")
                                .foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.top, 60)
                    } else {
                        ForEach(messages, id: \.id) { message in
                            MessageBubbleView(message: message)
                        }
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
            .task {
                await loadMessages()
            }
        }
    }

    private func loadMessages() async {
        isLoading = true
        // Refresh ensures AppDataStore is synced with latest data from nostrdb
        _ = await coreManager.safeCore.refresh()
        messages = await coreManager.safeCore.getMessages(conversationId: conversation.id)
        isLoading = false
    }
}

// MARK: - Message Bubble View

private struct MessageBubbleView: View {
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
                    .background(isUser ? Color.blue.opacity(0.15) : Color(.systemGray6))
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }

            if !isUser { Spacer(minLength: 50) }
        }
    }
}

// Note: ConversationInfo Identifiable conformance is in ConversationsView.swift

#Preview {
    ConversationsTabView()
        .environmentObject(TenexCoreManager())
}
