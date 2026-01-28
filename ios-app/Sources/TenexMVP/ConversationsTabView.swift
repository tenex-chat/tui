import SwiftUI

/// Main tab view for Conversations - shows aggregated conversation tree from all/filtered projects
/// with a project filter button in the toolbar
struct ConversationsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var projects: [ProjectInfo] = []
    @State private var conversationsByProject: [String: [ConversationInfo]] = [:]
    @State private var isLoading = false
    @State private var selectedProjectIds: Set<String> = []  // Empty means show all
    @State private var showFilterSheet = false
    @State private var selectedConversation: ConversationInfo?

    /// All conversations from filtered projects, flattened
    private var allConversations: [ConversationInfo] {
        let projectIds = selectedProjectIds.isEmpty
            ? Set(projects.map { $0.id })
            : selectedProjectIds

        return projectIds.flatMap { conversationsByProject[$0] ?? [] }
    }

    /// Root conversations (no parent) from filtered projects
    private var rootConversations: [ConversationInfo] {
        allConversations.filter { $0.parentId == nil }
    }

    /// Text for the filter button
    private var filterButtonLabel: String {
        if selectedProjectIds.isEmpty {
            return "All Projects"
        } else if selectedProjectIds.count == 1 {
            return projects.first { $0.id == selectedProjectIds.first }?.title ?? "1 Project"
        } else {
            return "\(selectedProjectIds.count) Projects"
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if isLoading && conversationsByProject.isEmpty {
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
                            ConversationTreeNode(
                                conversation: conversation,
                                allConversations: allConversations,
                                depth: 0,
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
                MessagesSheetView(conversation: conversation)
                    .environmentObject(coreManager)
            }
        }
    }

    private func projectTitle(for conversation: ConversationInfo) -> String? {
        // Find which project this conversation belongs to
        for (projectId, conversations) in conversationsByProject {
            if conversations.contains(where: { $0.id == conversation.id }) {
                return projects.first { $0.id == projectId }?.title
            }
        }
        return nil
    }

    private func loadData() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            _ = coreManager.core.refresh()
            let fetchedProjects = coreManager.core.getProjects()

            var conversationsMap: [String: [ConversationInfo]] = [:]
            for project in fetchedProjects {
                let conversations = coreManager.core.getConversations(projectId: project.id)
                conversationsMap[project.id] = conversations
            }

            DispatchQueue.main.async {
                self.projects = fetchedProjects
                self.conversationsByProject = conversationsMap
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

// MARK: - Conversation Tree Node (Recursive)

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

    private var statusColor: Color {
        switch conversation.status {
        case "active": return .green
        case "waiting": return .orange
        case "completed": return .gray
        default: return .blue
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

    private func projectColor(for project: ProjectInfo) -> Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal]
        let hash = project.id.hashValue
        return colors[abs(hash) % colors.count]
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

// Note: ConversationInfo Identifiable conformance is in ConversationsView.swift

#Preview {
    ConversationsTabView()
        .environmentObject(TenexCoreManager())
}
