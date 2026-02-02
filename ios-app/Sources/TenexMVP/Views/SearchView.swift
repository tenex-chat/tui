import SwiftUI

// MARK: - Data Structures

/// A group of search results within the same conversation
struct ConversationSearchGroup: Identifiable {
    let id: String  // threadId
    let title: String
    let projectName: String?
    let projectId: String?
    let matches: [SearchResult]

    var matchCount: Int { matches.count }
}

// MARK: - Search View

struct SearchView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    @State private var searchText = ""
    @State private var groupedResults: [ConversationSearchGroup] = []
    @State private var isSearching = false
    @State private var searchTask: Task<Void, Never>?
    @State private var navigateToConversation: SearchNavigationData?

    var body: some View {
        List {
            if groupedResults.isEmpty && !searchText.isEmpty && !isSearching {
                ContentUnavailableView(
                    "No Results",
                    systemImage: "magnifyingglass",
                    description: Text("No messages found matching \"\(searchText)\"")
                )
            } else if groupedResults.isEmpty && searchText.isEmpty {
                ContentUnavailableView(
                    "Search Messages",
                    systemImage: "magnifyingglass",
                    description: Text("Enter a search term to find messages across all conversations")
                )
            } else {
                ForEach(groupedResults) { group in
                    Section {
                        // Conversation header - tappable to navigate
                        Button {
                            navigateToConversation = SearchNavigationData(
                                conversationId: group.id
                            )
                        } label: {
                            ConversationGroupHeader(group: group)
                        }
                        .buttonStyle(.plain)
                        .listRowInsets(EdgeInsets(top: 6, leading: 16, bottom: 2, trailing: 16))

                        // Matching messages (indented)
                        ForEach(group.matches, id: \.eventId) { result in
                            Button {
                                if let threadId = result.threadId {
                                    navigateToConversation = SearchNavigationData(
                                        conversationId: threadId
                                    )
                                }
                            } label: {
                                MatchingMessageRow(result: result, searchTerm: searchText)
                            }
                            .buttonStyle(.plain)
                            .listRowInsets(EdgeInsets(top: 0, leading: 28, bottom: 2, trailing: 16))
                        }
                    }
                }
                .listSectionSpacing(4)
            }
        }
        .listStyle(.plain)
        .searchable(text: $searchText, placement: .navigationBarDrawer(displayMode: .always))
        .onChange(of: searchText) { _, newValue in
            performSearch(query: newValue)
        }
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button("Cancel") {
                    dismiss()
                }
            }
        }
        .navigationDestination(item: $navigateToConversation) { navData in
            SearchConversationView(
                conversationId: navData.conversationId
            )
            .environmentObject(coreManager)
        }
        .overlay {
            if isSearching {
                ProgressView()
                    .scaleEffect(1.2)
            }
        }
    }

    private func performSearch(query: String) {
        // Cancel any pending search
        searchTask?.cancel()

        guard query.count >= 2 else {
            groupedResults = []
            return
        }

        searchTask = Task {
            isSearching = true
            defer { isSearching = false }

            // Small debounce to avoid searching on every keystroke
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }

            let searchResults = await coreManager.safeCore.search(query: query, limit: 50)

            guard !Task.isCancelled else { return }

            // Group results by threadId
            var grouped: [String: [SearchResult]] = [:]
            for result in searchResults {
                if let threadId = result.threadId {
                    grouped[threadId, default: []].append(result)
                }
            }

            // Fetch conversation info for titles
            let conversationIds = Array(grouped.keys)
            let conversations = await coreManager.safeCore.getConversationsByIds(conversationIds: conversationIds)
            let conversationMap = Dictionary(uniqueKeysWithValues: conversations.map { ($0.id, $0) })

            // Get projects for project name lookup
            let projects = await coreManager.safeCore.getProjects()
            let projectMap = Dictionary(uniqueKeysWithValues: projects.map { ($0.id, $0.title) })

            // Build grouped results
            let groups = grouped.compactMap { threadId, matches -> ConversationSearchGroup? in
                let conv = conversationMap[threadId]
                let projectId = matches.first?.projectATag?.components(separatedBy: ":").last
                let projectName = projectId.flatMap { projectMap[$0] }

                return ConversationSearchGroup(
                    id: threadId,
                    title: conv?.title ?? "Unknown Conversation",
                    projectName: projectName,
                    projectId: projectId,
                    matches: matches.sorted { $0.createdAt > $1.createdAt }
                )
            }.sorted { $0.matches.first?.createdAt ?? 0 > $1.matches.first?.createdAt ?? 0 }

            guard !Task.isCancelled else { return }

            await MainActor.run {
                groupedResults = groups
            }
        }
    }
}

// MARK: - Conversation Group Header

struct ConversationGroupHeader: View {
    let group: ConversationSearchGroup

    var body: some View {
        HStack(spacing: 12) {
            // Conversation icon
            Image(systemName: "bubble.left.and.bubble.right.fill")
                .font(.title2)
                .foregroundStyle(.blue)

            VStack(alignment: .leading, spacing: 2) {
                // Title
                Text(group.title)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(2)

                // Project name
                if let projectName = group.projectName {
                    Text(projectName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            // Match count badge
            Text("\(group.matchCount) \(group.matchCount == 1 ? "match" : "matches")")
                .font(.caption)
                .fontWeight(.medium)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.orange.opacity(0.2))
                .foregroundStyle(.orange)
                .clipShape(Capsule())

            // Navigation chevron
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
    }
}

// MARK: - Matching Message Row

struct MatchingMessageRow: View {
    let result: SearchResult
    let searchTerm: String

    private var isUser: Bool {
        // Heuristic: agent names typically include specific keywords
        let author = result.author.lowercased()
        return !author.contains("agent") && !author.contains("claude") && !author.contains("gpt")
    }

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            // Vertical indent line
            Rectangle()
                .fill(Color.secondary.opacity(0.3))
                .frame(width: 2)

            // Author avatar
            Circle()
                .fill(isUser ? Color.green.gradient : Color.blue.gradient)
                .frame(width: 22, height: 22)
                .overlay {
                    Image(systemName: isUser ? "person.fill" : "sparkle")
                        .font(.system(size: 10))
                        .foregroundStyle(.white)
                }

            VStack(alignment: .leading, spacing: 2) {
                // Author and timestamp
                HStack {
                    Text(result.author)
                        .font(.caption)
                        .fontWeight(.medium)
                        .foregroundStyle(.primary)

                    Spacer()

                    Text(relativeTime(from: result.createdAt))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }

                // Content with highlighted search term
                highlightedText(result.content, searchTerm: searchTerm)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)

                // Kind indicator (report)
                if result.kind == 30023 {
                    HStack(spacing: 4) {
                        Image(systemName: "doc.text")
                            .font(.caption2)
                        Text("Report")
                            .font(.caption2)
                    }
                    .foregroundStyle(.blue.opacity(0.8))
                }
            }
        }
    }

    private func relativeTime(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    /// Build highlighted text with search term emphasized using AttributedString
    private func highlightedText(_ text: String, searchTerm: String) -> Text {
        guard !searchTerm.isEmpty else {
            return Text(text)
        }

        var result = AttributedString(text)
        let lowercasedText = text.lowercased()
        let lowercasedTerm = searchTerm.lowercased()

        // Find all occurrences and apply highlighting
        var searchStart = lowercasedText.startIndex
        while let range = lowercasedText.range(of: lowercasedTerm, range: searchStart..<lowercasedText.endIndex) {
            // Map to AttributedString range
            let startOffset = lowercasedText.distance(from: lowercasedText.startIndex, to: range.lowerBound)
            let endOffset = lowercasedText.distance(from: lowercasedText.startIndex, to: range.upperBound)

            let attrStart = result.index(result.startIndex, offsetByCharacters: startOffset)
            let attrEnd = result.index(result.startIndex, offsetByCharacters: endOffset)

            result[attrStart..<attrEnd].font = .body.bold()
            result[attrStart..<attrEnd].foregroundColor = .orange

            searchStart = range.upperBound
        }

        return Text(result)
    }
}

// MARK: - Navigation Data

struct SearchNavigationData: Identifiable, Hashable {
    let id = UUID()
    let conversationId: String
    // Note: projectId removed - not needed for message fetching
}

// MARK: - Search Conversation View

struct SearchConversationView: View {
    let conversationId: String
    // Note: projectId removed - conversation IDs are globally unique Nostr event IDs
    // and getMessages(conversationId:) doesn't accept projectId parameter
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @State private var loadTask: Task<Void, Never>?
    @State private var messagesChangedObserver: NSObjectProtocol?

    var body: some View {
        Group {
            if messages.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Messages")
                        .font(.title2)
                        .fontWeight(.semibold)
                    if isLoading {
                        ProgressView()
                            .padding(.top, 8)
                    }
                }
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(messages, id: \.id) { message in
                            SearchMessageBubble(message: message)
                        }
                    }
                    .padding()
                }
            }
        }
        .navigationTitle("Conversation")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            loadMessages()
            subscribeToMessageChanges()
        }
        .onDisappear {
            loadTask?.cancel()
            if let observer = messagesChangedObserver {
                NotificationCenter.default.removeObserver(observer)
                messagesChangedObserver = nil
            }
        }
    }

    private func subscribeToMessageChanges() {
        // Subscribe to message change notifications for reactive updates
        messagesChangedObserver = NotificationCenter.default.addObserver(
            forName: .tenexMessagesChanged,
            object: nil,
            queue: .main
        ) { [conversationId] notification in
            // Only reload if this notification is for our conversation
            if let changedConversationId = notification.object as? String,
               changedConversationId == conversationId {
                loadMessages()
            }
        }
    }

    private func loadMessages() {
        loadTask?.cancel()

        loadTask = Task {
            isLoading = true
            defer { isLoading = false }

            guard !Task.isCancelled else { return }

            _ = await coreManager.safeCore.refresh()
            let fetched = await coreManager.safeCore.getMessages(conversationId: conversationId)

            guard !Task.isCancelled else { return }

            self.messages = fetched
        }
    }
}

// MARK: - Search Message Bubble

struct SearchMessageBubble: View {
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

                    Text(relativeTime(from: message.createdAt))
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

    private func relativeTime(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}
