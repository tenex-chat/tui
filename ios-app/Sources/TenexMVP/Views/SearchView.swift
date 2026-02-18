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

enum SearchLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct SearchView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let layoutMode: SearchLayoutMode
    private let selectedConversationBindingOverride: Binding<ConversationFullInfo?>?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var searchText = ""
    @State private var groupedResults: [ConversationSearchGroup] = []
    @State private var isSearching = false
    @State private var searchTask: Task<Void, Never>?
    @State private var selectedConversationState: ConversationFullInfo?
    @State private var isLoadingConversation = false
    @State private var loadingConversationId: String?  // Track which conversation we're loading for "latest wins"
    @State private var loadErrorMessage: String?  // Error feedback for failed loads

    init(
        layoutMode: SearchLayoutMode = .adaptive,
        selectedConversation: Binding<ConversationFullInfo?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedConversationBindingOverride = selectedConversation
    }

    private var selectedConversationBinding: Binding<ConversationFullInfo?> {
        selectedConversationBindingOverride ?? $selectedConversationState
    }

    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail {
            return true
        }
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .adaptive:
                if useSplitView {
                    splitLayout
                } else {
                    stackLayout
                }
            }
        }
        .onChange(of: searchText) { _, newValue in
            performSearch(query: newValue)
        }
        .onChange(of: coreManager.appFilterProjectIds) { _, _ in
            performSearch(query: searchText)
        }
        .onChange(of: coreManager.appFilterTimeWindow) { _, _ in
            performSearch(query: searchText)
        }
        .onChange(of: groupedResults.map(\.id)) { _, visibleConversationIds in
            if let selectedId = selectedConversationBinding.wrappedValue?.id,
               !visibleConversationIds.contains(selectedId) {
                selectedConversationBinding.wrappedValue = nil
            }
        }
        .overlay {
            if isSearching || isLoadingConversation {
                ProgressView()
                    .scaleEffect(1.2)
            }
        }
        .alert("Unable to Load Conversation", isPresented: .init(
            get: { loadErrorMessage != nil },
            set: { if !$0 { loadErrorMessage = nil } }
        )) {
            Button("OK", role: .cancel) {
                loadErrorMessage = nil
            }
        } message: {
            if let message = loadErrorMessage {
                Text(message)
            }
        }
    }

    // MARK: - Layouts

    private var stackLayout: some View {
        NavigationStack {
            searchResultsList
                .navigationTitle("Search")
                .navigationBarTitleDisplayMode(.inline)
                .navigationDestination(item: selectedConversationBinding) { conversation in
                    ConversationAdaptiveDetailView(conversation: conversation)
                        .environmentObject(coreManager)
                }
        }
    }

    private var shellListLayout: some View {
        searchResultsList
            .navigationTitle("Search")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        NavigationStack {
            splitDetailContent
        }
        .accessibilityIdentifier("detail_column")
    }

    private var splitLayout: some View {
        #if os(macOS)
        return AnyView(
            HSplitView {
                searchResultsList
                    .frame(minWidth: 340, idealWidth: 420, maxWidth: 520, maxHeight: .infinity)

                NavigationStack {
                    splitDetailContent
                }
                .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        )
        #else
        return AnyView(
            NavigationSplitView {
                searchResultsList
                    .navigationTitle("Search")
            } detail: {
                NavigationStack {
                    splitDetailContent
                }
            }
        )
        #endif
    }

    @ViewBuilder
    private var splitDetailContent: some View {
        if let conversation = selectedConversationBinding.wrappedValue {
            ConversationAdaptiveDetailView(conversation: conversation)
                .environmentObject(coreManager)
        } else {
            ContentUnavailableView(
                "Select a Conversation",
                systemImage: "bubble.left.and.bubble.right",
                description: Text("Search results will open here")
            )
        }
    }

    private var searchResultsList: some View {
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
                        Button {
                            Task { await loadAndSelectConversation(id: group.id) }
                        } label: {
                            ConversationGroupHeader(group: group, showsChevron: !useSplitView)
                        }
                        .listRowInsets(EdgeInsets(top: 6, leading: 16, bottom: 2, trailing: 16))

                        ForEach(group.matches, id: \.eventId) { result in
                            Button {
                                if let threadId = result.threadId {
                                    Task { await loadAndSelectConversation(id: threadId) }
                                }
                            } label: {
                                MatchingMessageRow(result: result, searchTerm: searchText)
                            }
                            .listRowInsets(EdgeInsets(top: 0, leading: 28, bottom: 2, trailing: 16))
                        }
                    }
                }
                #if os(iOS)
                .listSectionSpacing(4)
                #endif
            }
        }
        #if os(iOS)
        .listStyle(.plain)
        #else
        .listStyle(.inset)
        #endif
        #if os(iOS)
        .searchable(text: $searchText, placement: .navigationBarDrawer(displayMode: .always))
        #else
        .searchable(text: $searchText)
        #endif
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                AppGlobalFilterToolbarButton()
            }
        }
    }

    private func performSearch(query: String) {
        // Cancel any pending search
        searchTask?.cancel()
        let filterSnapshot = coreManager.appFilterSnapshot

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

            let now = UInt64(Date().timeIntervalSince1970)
            let filteredResults = searchResults.filter { result in
                let projectId = result.projectATag.map(TenexCoreManager.projectId(fromATag:))
                return filterSnapshot.includes(projectId: projectId, timestamp: result.createdAt, now: now)
            }

            // Group results by threadId
            var grouped: [String: [SearchResult]] = [:]
            for result in filteredResults {
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
                let projectIdFromConversation: String? = {
                    guard let conv else { return nil }
                    let parsed = TenexCoreManager.projectId(fromATag: conv.projectATag)
                    return parsed.isEmpty ? nil : parsed
                }()
                let projectId = projectIdFromConversation ?? matches
                    .compactMap { $0.projectATag }
                    .map(TenexCoreManager.projectId(fromATag:))
                    .first(where: { !$0.isEmpty })
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

    /// Fetch conversation details and present the detail sheet
    /// Uses "latest request wins" pattern to prevent race conditions when user taps multiple results quickly
    private func loadAndSelectConversation(id: String) async {
        // Mark this as the current loading target - any previous in-flight request becomes stale
        await MainActor.run {
            loadingConversationId = id
            isLoadingConversation = true
        }

        let conversations = await coreManager.safeCore.getConversationsByIds(conversationIds: [id])

        await MainActor.run {
            // Only process if this is still the latest request (prevents race condition)
            guard loadingConversationId == id else { return }

            isLoadingConversation = false
            loadingConversationId = nil

            if let conversation = conversations.first {
                selectedConversationBinding.wrappedValue = conversation
            } else {
                // Show error feedback when conversation can't be loaded
                loadErrorMessage = "This conversation may have been deleted or is no longer available."
            }
        }
    }
}

// MARK: - Conversation Group Header

struct ConversationGroupHeader: View {
    let group: ConversationSearchGroup
    var showsChevron: Bool = true

    var body: some View {
        HStack(spacing: 12) {
            // Conversation icon
            Image(systemName: "bubble.left.and.bubble.right.fill")
                .font(.title2)
                .foregroundStyle(Color.agentBrand)

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
                .background(Color.askBrandBackground)
                .foregroundStyle(Color.askBrand)
                .clipShape(Capsule())

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
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
                .fill(isUser ? Color.messageUserAvatarColor.gradient : Color.agentBrand.gradient)
                .frame(width: 22, height: 22)
                .overlay {
                    Image(systemName: isUser ? "person.fill" : "sparkle")
                        .font(.caption2)
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
                    .foregroundStyle(Color.agentBrand.opacity(0.8))
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
