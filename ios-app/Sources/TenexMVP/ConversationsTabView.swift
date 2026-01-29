import SwiftUI
import CryptoKit

/// Main tab view for Conversations - shows aggregated conversation tree from all/filtered projects
/// with a project filter button in the toolbar
struct ConversationsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedProjectIds: Set<String> = []  // Empty means show all
    @State private var showFilterSheet = false
    @State private var selectedConversation: ConversationFullInfo?

    /// Filtered conversations based on selected projects
    private var filteredConversations: [ConversationFullInfo] {
        if selectedProjectIds.isEmpty {
            return coreManager.conversations
        }
        return coreManager.conversations.filter { selectedProjectIds.contains($0.projectATag) }
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
                if isLoading && allConversationsFull.isEmpty {
                    VStack(spacing: 16) {
                        ProgressView()
                            .scaleEffect(1.5)
                        Text("Loading conversations...")
                            .foregroundStyle(.secondary)
                    }
                } else if rootConversations.isEmpty {
                    ConversationsEmptyState(
                        hasFilter: !selectedProjectIds.isEmpty,
                        onRefresh: loadData,
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
                        await loadDataAsync()
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
                    Button(action: loadData) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(isLoading)
                }
            }
            .onAppear {
                if projects.isEmpty {
                    loadData()
                }
            }
            .sheet(isPresented: $showFilterSheet) {
                ProjectFilterSheet(
                    projects: projects,
                    selectedProjectIds: $selectedProjectIds
                )
            }
            .sheet(item: $selectedConversation) { conversation in
                ConversationDetailView(conversation: conversation)
                    .environmentObject(coreManager)
            }
        }
    }

    private func projectTitle(for conversation: ConversationFullInfo) -> String? {
        // Find project title from the conversation's projectATag
        return projects.first { $0.id == conversation.projectATag }?.title
    }

    private func loadData() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            _ = coreManager.core.refresh()
            let fetchedProjects = coreManager.core.getProjects()

            // Use getAllConversations with ConversationFullInfo for richer data
            let filter = ConversationFilter(
                projectIds: [],  // Empty = all projects
                showArchived: false,
                hideScheduled: true,
                timeFilter: .all
            )
            let conversations = (try? coreManager.core.getAllConversations(filter: filter)) ?? []

            DispatchQueue.main.async {
                self.projects = fetchedProjects
                self.allConversationsFull = conversations
                self.isLoading = false
            }
        }
    }

    private func loadDataAsync() async {
        await withCheckedContinuation { continuation in
            loadData()
            // Simple delay to allow UI to update
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                continuation.resume()
            }
        }
    }
}

// MARK: - Conversation Row for ConversationFullInfo

/// Conversation row that uses ConversationFullInfo's rich data
private struct ConversationRowFull: View {
    let conversation: ConversationFullInfo
    let projectTitle: String?
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

                // Row 3: Author avatar and project badge
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
    }
}

// MARK: - Legacy Optimized Hierarchy Conversation Row (Kept for backwards compatibility)

/// Conversation row that uses precomputed hierarchy data for O(1) access
/// instead of recomputing O(n²) BFS on every render
private struct HierarchyConversationRowOptimized: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let conversation: ConversationInfo
    let aggregatedData: AggregatedConversationData
    let projectTitle: String?
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

                // Row 3: Participating agent avatars with profile pictures from kind:0 events
                HStack(spacing: -8) {
                    ForEach(aggregatedData.participatingAgentInfos.prefix(maxVisibleAvatars)) { agentInfo in
                        AgentAvatarWithPicture(agentInfo: agentInfo)
                            .environmentObject(coreManager)
                    }

                    // Show overflow count if more than maxVisibleAvatars agents
                    if aggregatedData.participatingAgentInfos.count > maxVisibleAvatars {
                        Circle()
                            .fill(Color(.systemGray4))
                            .frame(width: 24, height: 24)
                            .overlay {
                                Text("+\(aggregatedData.participatingAgentInfos.count - maxVisibleAvatars)")
                                    .font(.caption2)
                                    .fontWeight(.medium)
                                    .foregroundStyle(.secondary)
                            }
                    }

                    Spacer()

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
    }
}

// MARK: - Legacy Hierarchy Conversation Row (Kept for ConversationsView.swift compatibility)
// TODO: Update ConversationsView.swift to use optimized version as well

private struct HierarchyConversationRowLegacy: View {
    let conversation: ConversationInfo
    let allConversations: [ConversationInfo]
    let projectTitle: String?
    let onSelect: (ConversationInfo) -> Void

    /// All descendants using safe BFS with cycle detection
    private var allDescendants: [ConversationInfo] {
        var descendants: [ConversationInfo] = []
        var visited = Set<String>()
        var queue = allConversations.filter { $0.parentId == conversation.id }
        var queueIndex = 0

        while queueIndex < queue.count {
            let current = queue[queueIndex]
            queueIndex += 1

            // Cycle detection
            if visited.contains(current.id) {
                continue
            }
            visited.insert(current.id)

            descendants.append(current)
            let children = allConversations.filter { $0.parentId == current.id }
            queue.append(contentsOf: children)
        }

        return descendants
    }

    private var effectiveLastActivity: UInt64 {
        let allActivities = [conversation.lastActivity] + allDescendants.map { $0.lastActivity }
        return allActivities.max() ?? conversation.lastActivity
    }

    private var activitySpan: TimeInterval {
        let allTimestamps = [conversation.lastActivity] + allDescendants.map { $0.lastActivity }
        guard let earliest = allTimestamps.min(),
              let latest = allTimestamps.max() else {
            return 0
        }
        return TimeInterval(latest - earliest)
    }

    private var participatingAgents: [String] {
        var agents = Set<String>()
        agents.insert(conversation.author)
        for descendant in allDescendants {
            agents.insert(descendant.author)
        }
        return agents.sorted()
    }

    var body: some View {
        HStack(spacing: 12) {
            Circle()
                .fill(conversationStatusColor(for: conversation.status))
                .frame(width: 10, height: 10)

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

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

                    if activitySpan > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.caption2)
                            Text(ConversationFormatters.formatDuration(activitySpan))
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                HStack(spacing: -8) {
                    ForEach(participatingAgents.prefix(maxVisibleAvatars), id: \.self) { agent in
                        SharedAgentAvatar(agentName: agent)
                    }

                    if participatingAgents.count > maxVisibleAvatars {
                        Circle()
                            .fill(Color(.systemGray4))
                            .frame(width: 24, height: 24)
                            .overlay {
                                Text("+\(participatingAgents.count - maxVisibleAvatars)")
                                    .font(.caption2)
                                    .fontWeight(.medium)
                                    .foregroundStyle(.secondary)
                            }
                    }

                    Spacer()

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
    }
}

// Note: AgentAvatar removed - use SharedAgentAvatar from ConversationHierarchy.swift

// MARK: - Legacy Conversation Tree Node (Recursive) - Kept for reference

private struct ConversationTreeNode: View {
    let conversation: ConversationInfo
    let allConversations: [ConversationInfo]
    let depth: Int
    let projectTitle: String?
    let onSelect: (ConversationInfo) -> Void

    @State private var isExpanded = true

    private var children: [ConversationInfo] {
        allConversations.filter { $0.parentId == conversation.id }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Conversation row
            HStack(spacing: 12) {
                // Expand/collapse button (only show for items with children or nested items)
                if !children.isEmpty {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 20, height: 44)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                isExpanded.toggle()
                            }
                        }
                } else if depth > 0 {
                    // Only add spacer for nested items without children
                    Spacer().frame(width: 20)
                }

                // Main conversation content
                HStack(spacing: 12) {
                    // Status indicator
                    Circle()
                        .fill(conversationStatusColor(for: conversation.status))
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
                            // Show project title for root conversations
                            if depth == 0, let projectTitle = projectTitle {
                                Text(projectTitle)
                                    .font(.caption)
                                    .foregroundStyle(.blue)
                                    .lineLimit(1)
                                Text("•")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }

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
                        projectTitle: nil,  // Don't show project for children
                        onSelect: onSelect
                    )
                }
            }
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
    let onRefresh: () -> Void
    let onClearFilter: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: hasFilter ? "line.3.horizontal.decrease.circle" : "bubble.left.and.bubble.right")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(hasFilter ? "No Matching Conversations" : "No Conversations")
                .font(.title2)
                .fontWeight(.semibold)

            Text(hasFilter ? "Try adjusting your project filter" : "Your conversations will appear here")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if hasFilter {
                Button(action: onClearFilter) {
                    Label("Clear Filter", systemImage: "xmark.circle")
                }
                .buttonStyle(.bordered)
                .padding(.top, 8)
            } else {
                Button(action: onRefresh) {
                    Label("Refresh", systemImage: "arrow.clockwise")
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
            .onAppear {
                loadMessages()
            }
        }
    }

    private func loadMessages() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            // Refresh ensures AppDataStore is synced with latest data from nostrdb
            _ = coreManager.core.refresh()
            let fetched = coreManager.core.getMessages(conversationId: conversation.id)
            DispatchQueue.main.async {
                self.messages = fetched
                self.isLoading = false
            }
        }
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
