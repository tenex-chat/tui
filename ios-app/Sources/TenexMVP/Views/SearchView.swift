import SwiftUI

// MARK: - Search View

struct SearchView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    @State private var searchText = ""
    @State private var results: [SearchResult] = []
    @State private var isSearching = false
    @State private var searchTask: Task<Void, Never>?
    @State private var navigateToConversation: SearchNavigationData?

    var body: some View {
        NavigationStack {
            List {
                if results.isEmpty && !searchText.isEmpty && !isSearching {
                    ContentUnavailableView(
                        "No Results",
                        systemImage: "magnifyingglass",
                        description: Text("No messages found matching \"\(searchText)\"")
                    )
                } else if results.isEmpty && searchText.isEmpty {
                    ContentUnavailableView(
                        "Search Messages",
                        systemImage: "magnifyingglass",
                        description: Text("Enter a search term to find messages across all conversations")
                    )
                } else {
                    ForEach(results, id: \.eventId) { result in
                        Button {
                            if let threadId = result.threadId {
                                navigateToConversation = SearchNavigationData(
                                    conversationId: threadId,
                                    projectId: result.projectATag?.components(separatedBy: ":").last
                                )
                            }
                        } label: {
                            SearchResultRow(result: result)
                        }
                        .buttonStyle(.plain)
                    }
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
                    conversationId: navData.conversationId,
                    projectId: navData.projectId
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
    }

    private func performSearch(query: String) {
        // Cancel any pending search
        searchTask?.cancel()

        guard query.count >= 2 else {
            results = []
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

            await MainActor.run {
                results = searchResults
            }
        }
    }
}

// MARK: - Search Result Row

struct SearchResultRow: View {
    let result: SearchResult

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Author and time
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

            // Content snippet
            Text(result.content)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(3)

            // Thread context indicator
            if result.threadId != nil {
                HStack(spacing: 4) {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .font(.caption2)
                    Text("In conversation")
                        .font(.caption2)
                }
                .foregroundStyle(.tertiary)
            }

            // Kind indicator (message vs report)
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
        .padding(.vertical, 6)
    }

    private func relativeTime(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}

// MARK: - Navigation Data

struct SearchNavigationData: Identifiable, Hashable {
    let id = UUID()
    let conversationId: String
    let projectId: String?
}

// MARK: - Search Conversation View

struct SearchConversationView: View {
    let conversationId: String
    let projectId: String?
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @State private var loadTask: Task<Void, Never>?

    var body: some View {
        Group {
            if isLoading {
                ProgressView("Loading conversation...")
            } else if messages.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Messages")
                        .font(.title2)
                        .fontWeight(.semibold)
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
        }
        .onDisappear {
            loadTask?.cancel()
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
