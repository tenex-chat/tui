import SwiftUI

// MARK: - InboxEventType Display Info

/// Single source of truth for event type display info
struct InboxEventTypeInfo {
    let icon: String
    let label: String       // Full label for detail view ("Mention")
    let shortLabel: String  // Short label for list view ("mentioned you")
    let color: Color
}

extension InboxEventType {
    var info: InboxEventTypeInfo {
        switch self {
        case .ask:
            return InboxEventTypeInfo(icon: "questionmark.circle", label: "Question", shortLabel: "asked you", color: .orange)
        case .mention:
            return InboxEventTypeInfo(icon: "at", label: "Mention", shortLabel: "mentioned you", color: .blue)
        }
    }
}

// MARK: - InboxItem Extensions (domain logic)

extension InboxItem {
    var isUnread: Bool {
        !isRead
    }

    var eventTypeInfo: InboxEventTypeInfo {
        eventType.info
    }

    func matches(filter: InboxFilter) -> Bool {
        filter.matches(eventType)
    }

    /// Extract project ID from the `projectATag` field.
    /// Returns nil if the aTag doesn't contain a valid project ID.
    var resolvedProjectId: String? {
        let parts = projectATag.split(separator: ":")
        guard parts.count >= 3 else { return nil }
        let id = parts.dropFirst(2).joined(separator: ":")
        return id.isEmpty ? nil : id
    }

    /// Truncated author pubkey for display (first 8 + last 4 chars)
    var authorDisplayName: String {
        let pk = authorPubkey
        if pk.count > 12 {
            return "\(pk.prefix(8))...\(pk.suffix(4))"
        }
        return pk
    }
}

// MARK: - Inbox Filter Tab

enum InboxFilter: String, CaseIterable, Identifiable {
    case all = "All"
    case questions = "Questions"
    case mentions = "Mentions"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .all: return "tray"
        case .questions: return "questionmark.circle"
        case .mentions: return "at"
        }
    }

    func matches(_ eventType: InboxEventType) -> Bool {
        switch self {
        case .all: return true
        case .questions: return eventType == .ask
        case .mentions: return eventType == .mention
        }
    }
}

// MARK: - Date Formatter (cached as static for performance)

private enum InboxDateFormatters {
    static let fullDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    static func fullDate(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return fullDateFormatter.string(from: date)
    }
}

// MARK: - Inbox View

enum InboxLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct InboxView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let layoutMode: InboxLayoutMode
    private let selectedFilterBindingOverride: Binding<InboxFilter>?
    private let selectedItemIdBindingOverride: Binding<String?>?
    private let activeConversationIdBindingOverride: Binding<String?>?

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var selectedFilterState: InboxFilter = .all
    @State private var selectedItemIdState: String?
    @State private var presentedItem: InboxItem?
    @State private var pendingNavigation: ConversationNavigationData?
    @State private var navigateToConversation: ConversationNavigationData?
    @State private var activeConversationIdState: String?

    init(
        layoutMode: InboxLayoutMode = .adaptive,
        selectedFilter: Binding<InboxFilter>? = nil,
        selectedItemId: Binding<String?>? = nil,
        activeConversationId: Binding<String?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedFilterBindingOverride = selectedFilter
        self.selectedItemIdBindingOverride = selectedItemId
        self.activeConversationIdBindingOverride = activeConversationId
    }

    private var selectedFilterBinding: Binding<InboxFilter> {
        selectedFilterBindingOverride ?? $selectedFilterState
    }

    private var selectedItemIdBinding: Binding<String?> {
        selectedItemIdBindingOverride ?? $selectedItemIdState
    }

    private var activeConversationIdBinding: Binding<String?> {
        activeConversationIdBindingOverride ?? $activeConversationIdState
    }

    // MARK: - Time Invalidation

    /// Refresh interval for time-based invalidation (1 minute).
    /// Items crossing the selected time window boundary will disappear on next tick.
    private static let refreshIntervalSeconds: TimeInterval = 60

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

    // MARK: - Computed Properties (Time-Aware)

    /// Items within the global project/time filter scope.
    private func itemsWithinGlobalFilter(now: UInt64) -> [InboxItem] {
        coreManager.inboxItems.filter { item in
            coreManager.inboxItemMatchesAppFilter(item, now: now)
        }
    }

    /// Items filtered by current tab selection (after global project/time filter).
    private func filteredItems(now: UInt64) -> [InboxItem] {
        itemsWithinGlobalFilter(now: now).filter { $0.matches(filter: selectedFilterBinding.wrappedValue) }
    }

    /// Count of unread items for badge display (within global project/time filter).
    private func unreadCount(now: UInt64) -> Int {
        itemsWithinGlobalFilter(now: now).filter(\.isUnread).count
    }

    /// Unread count for a specific segment (within global project/time filter).
    private func unreadCount(for filter: InboxFilter, now: UInt64) -> Int {
        if filter == .all {
            return unreadCount(now: now)
        }
        return itemsWithinGlobalFilter(now: now).filter { $0.matches(filter: filter) && $0.isUnread }.count
    }

    var body: some View {
        // TimelineView triggers periodic re-render so time-window filtering stays current.
        TimelineView(.periodic(from: .now, by: Self.refreshIntervalSeconds)) { context in
            // Compute `now` once per render cycle for consistent filtering
            let now = UInt64(context.date.timeIntervalSince1970)
            let items = filteredItems(now: now)

            Group {
                switch layoutMode {
                case .shellList:
                    shellListLayout(items: items, now: now)
                case .shellDetail:
                    shellDetailLayout(now: now)
                case .adaptive:
                    if useSplitView {
                        splitLayout(items: items, now: now)
                    } else {
                        stackLayout(items: items, now: now)
                    }
                }
            }
            .onChange(of: selectedFilterBinding.wrappedValue) { _, _ in
                activeConversationIdBinding.wrappedValue = nil
            }
            .onChange(of: items) { _, _ in
                if let selectedItemId = selectedItemIdBinding.wrappedValue,
                   !items.contains(where: { $0.id == selectedItemId }) {
                    selectedItemIdBinding.wrappedValue = nil
                    activeConversationIdBinding.wrappedValue = nil
                }
            }
        }
    }

    // MARK: - Split Layout (macOS / iPad)

    private func splitLayout(items: [InboxItem], now: UInt64) -> some View {
        #if os(macOS)
        return AnyView(
            HSplitView {
                splitSidebar(items: items, now: now)
                    .frame(minWidth: 340, idealWidth: 420, maxWidth: 520, maxHeight: .infinity)

                NavigationStack {
                    splitDetailContent(now: now)
                }
                .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        )
        #else
        return AnyView(
            NavigationSplitView {
                splitSidebar(items: items, now: now)
                    .navigationTitle("Inbox")
            } detail: {
                NavigationStack {
                    splitDetailContent(now: now)
                }
            }
        )
        #endif
    }

    private func splitSidebar(items: [InboxItem], now: UInt64) -> some View {
        VStack(spacing: 0) {
            filterTabBar(now: now)

            Divider()

            if items.isEmpty {
                emptyStateView
            } else {
                List(selection: selectedItemIdBinding) {
                    ForEach(items, id: \.id) { item in
                        InboxItemRow(item: item, showsChevron: false)
                            .tag(item.id)
                    }
                }
                .modifier(ShellInboxListStyle(isShellColumn: layoutMode == .shellList))
                .onChange(of: selectedItemIdBinding.wrappedValue) { _, _ in
                    activeConversationIdBinding.wrappedValue = nil
                }
            }
        }
        .toolbar {
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
        }
    }

    @ViewBuilder
    private func splitDetailContent(now: UInt64) -> some View {
        if let conversationId = activeConversationIdBinding.wrappedValue {
            ConversationByIdAdaptiveDetailView(conversationId: conversationId)
                .environment(coreManager)
        } else if let item = selectedItem(now: now) {
            InboxDetailView(
                item: item,
                onNavigateToConversation: { conversationId in
                    activeConversationIdBinding.wrappedValue = conversationId
                },
                isEmbedded: true
            )
            .environment(coreManager)
        } else {
            ContentUnavailableView(
                "Select a Notification",
                systemImage: "tray",
                description: Text("Choose an inbox item from the list")
            )
        }
    }

    private func selectedItem(now: UInt64) -> InboxItem? {
        guard let selectedItemId = selectedItemIdBinding.wrappedValue else { return nil }
        return itemsWithinGlobalFilter(now: now).first(where: { $0.id == selectedItemId })
    }

    private func shellListLayout(items: [InboxItem], now: UInt64) -> some View {
        splitSidebar(items: items, now: now)
            .navigationTitle("Inbox")
            .accessibilityIdentifier("section_list_column")
    }

    private func shellDetailLayout(now: UInt64) -> some View {
        NavigationStack {
            splitDetailContent(now: now)
        }
        .accessibilityIdentifier("detail_column")
    }

    // MARK: - Stack Layout (iPhone)

    private func stackLayout(items: [InboxItem], now: UInt64) -> some View {
        NavigationStack {
            VStack(spacing: 0) {
                filterTabBar(now: now)

                Divider()

                if items.isEmpty {
                    emptyStateView
                } else {
                    inboxList(items: items) { item in
                        presentedItem = item
                    }
                }
            }
            .navigationTitle("Inbox")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.large)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .automatic) {
                    AppGlobalFilterToolbarButton()
                }
            }
            .sheet(item: $presentedItem, onDismiss: {
                if let pending = pendingNavigation {
                    navigateToConversation = pending
                    pendingNavigation = nil
                }
            }) { item in
                InboxDetailView(item: item, onNavigateToConversation: { convId in
                    pendingNavigation = ConversationNavigationData(conversationId: convId)
                    presentedItem = nil
                })
                .environment(coreManager)
            }
            .navigationDestination(item: $navigateToConversation) { navData in
                ConversationByIdAdaptiveDetailView(conversationId: navData.conversationId)
                    .environment(coreManager)
            }
        }
    }

    // MARK: - Filter Tab Bar

    private func filterTabBar(now: UInt64) -> some View {
        MailStyleCategoryPicker(
            cases: InboxFilter.allCases,
            selection: selectedFilterBinding,
            icon: \.icon,
            label: \.rawValue
        )
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
    }

    // MARK: - Empty State

    private var emptyStateView: some View {
        VStack {
            Spacer()
            VStack(spacing: 16) {
                Image(systemName: "tray")
                    .font(.system(size: 60))
                    .foregroundStyle(.secondary)
                Text("No Notifications")
                    .font(.title2)
                    .fontWeight(.semibold)
                Text(emptyStateMessage)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            Spacer()
        }
    }

    private var emptyStateMessage: String {
        switch selectedFilterBinding.wrappedValue {
        case .all:
            return "No items waiting for your attention"
        case .questions:
            return "No questions to answer"
        case .mentions:
            return "No mentions to review"
        }
    }

    // MARK: - Inbox List

    private func inboxList(items: [InboxItem], onSelect: @escaping (InboxItem) -> Void) -> some View {
        List {
            ForEach(items, id: \.id) { item in
                Button(action: { onSelect(item) }) {
                    InboxItemRow(item: item)
                }
            }
        }
        .listStyle(.plain)
        .tenexListSurfaceBackground()
    }

}

private struct ShellInboxListStyle: ViewModifier {
    let isShellColumn: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if isShellColumn {
            #if os(macOS)
            content.listStyle(.inset)
            #else
            content.listStyle(.plain)
            #endif
        } else {
            content.listStyle(.sidebar)
        }
    }
}

// MARK: - Inbox Item Row

struct InboxItemRow: View {
    let item: InboxItem
    var showsChevron: Bool = true

    var body: some View {
        HStack(spacing: 12) {
            // Unread indicator dot
            Circle()
                .fill(item.isUnread ? Color.unreadIndicator : Color.clear)
                .frame(width: 10, height: 10)

            // Event type icon (using domain extension)
            Image(systemName: item.eventTypeInfo.icon)
                .foregroundStyle(item.eventTypeInfo.color)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 4) {
                // Title row with time
                HStack {
                    Text(item.title)
                        .font(.headline)
                        .fontWeight(item.isUnread ? .bold : .regular)
                        .foregroundStyle(item.isUnread ? .primary : .secondary)
                        .lineLimit(1)

                    Spacer()

                    RelativeTimeText(timestamp: item.createdAt, style: .localizedAbbreviated)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }

                // Event type label (using short label from domain extension)
                HStack(spacing: 4) {
                    Text(item.authorDisplayName)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Text("•")
                        .foregroundStyle(.tertiary)

                    Text(item.eventTypeInfo.shortLabel)
                        .font(.caption)
                        .foregroundStyle(item.eventTypeInfo.color)
                }

                // Project info if available
                if let projectId = item.resolvedProjectId {
                    HStack(spacing: 4) {
                        Image(systemName: "folder")
                            .font(.caption2)
                        Text(projectId)
                            .font(.caption)
                    }
                    .foregroundStyle(.tertiary)
                }
            }

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 8)
        .opacity(item.isUnread ? 1.0 : 0.7)
    }
}

// MARK: - Inbox Detail View

struct InboxDetailView: View {
    let item: InboxItem
    let onNavigateToConversation: (String) -> Void
    var isEmbedded: Bool = false
    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

    @ViewBuilder
    var body: some View {
        if isEmbedded {
            detailContent
        } else {
            modalDetailContent
        }
    }

    private var modalDetailContent: some View {
        NavigationStack {
            detailContent
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .tenexModalPresentation(detents: [.large])
    }

    private var detailContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // Header with badges
                headerSection

                Divider()

                // Content
                contentSection

                // Related info with navigation
                if item.resolvedProjectId != nil || item.threadId != nil {
                    Divider()
                    relatedSection
                }

                Spacer()
            }
            .padding()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - Header Section

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Type and status badges (using domain extensions)
            HStack {
                // Event type badge
                HStack(spacing: 4) {
                    Image(systemName: item.eventTypeInfo.icon)
                        .font(.caption2)
                    Text(item.eventTypeInfo.label)
                        .font(.caption)
                        .fontWeight(.medium)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(item.eventTypeInfo.color.opacity(0.15))
                .foregroundStyle(item.eventTypeInfo.color)
                .clipShape(Capsule())

                // Read status badge
                HStack(spacing: 4) {
                    Image(systemName: item.isUnread ? "circle.fill" : "checkmark.circle.fill")
                        .font(.caption2)
                    Text(item.isUnread ? "Unread" : "Read")
                        .font(.caption)
                        .fontWeight(.medium)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(item.isUnread ? Color.askBrandBackground : Color.presenceOnlineBackground)
                .foregroundStyle(item.isUnread ? Color.askBrand : Color.presenceOnline)
                .clipShape(Capsule())

                Spacer()
            }

            // Title
            Text(item.title)
                .font(.title)
                .fontWeight(.bold)

            // Metadata (using cached formatters)
            HStack(spacing: 16) {
                HStack(spacing: 6) {
                    Image(systemName: "person.circle.fill")
                    Text(item.authorDisplayName)
                }
                .foregroundStyle(.secondary)

                HStack(spacing: 6) {
                    Image(systemName: "clock")
                    Text(InboxDateFormatters.fullDate(from: item.createdAt))
                }
                .foregroundStyle(.secondary)
            }
            .font(.subheadline)
        }
    }

    // MARK: - Content Section

    @ViewBuilder
    private var contentSection: some View {
        if let askEvent = item.askEvent,
           let conversationId = item.threadId,
           let projectId = item.resolvedProjectId {
            // Interactive ask answering view
            AskAnswerView(
                askEvent: askEvent,
                askEventId: item.id,
                askAuthorPubkey: item.authorPubkey,
                conversationId: conversationId,
                projectId: projectId
            ) {
                // Dismiss after successful submit
                dismiss()
            }
            .environment(coreManager)
        } else {
            // Regular text content
            Text(item.content)
                .font(.body)
        }
    }

    // MARK: - Related Section (Navigation to Conversation)

    private var relatedSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Related")
                .font(.headline)

            if let projectId = item.resolvedProjectId {
                HStack {
                    Image(systemName: "folder.fill")
                        .foregroundStyle(Color.agentBrand)
                    Text("Project: \(projectId)")
                    Spacer()
                    Image(systemName: "chevron.right")
                        .foregroundStyle(.tertiary)
                }
                .padding()
                .background(Color.systemGray6)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }

            // Conversation navigation button
            if let convId = item.threadId {
                Button(action: {
                    onNavigateToConversation(convId)
                }) {
                    HStack {
                        Image(systemName: "bubble.left.and.bubble.right.fill")
                            .foregroundStyle(Color.presenceOnline)
                        Text("View Conversation")
                        Spacer()
                        Image(systemName: "chevron.right")
                            .foregroundStyle(.tertiary)
                    }
                    .padding()
                    .background(Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                }
                .buttonStyle(.borderless)
            }
        }
    }
}

// MARK: - Navigation Data

struct ConversationNavigationData: Identifiable, Hashable {
    let id = UUID()
    let conversationId: String
    // Note: projectId removed - not needed for message fetching
}

// MARK: - Inbox Conversation View (Navigate from inbox item)

struct InboxConversationView: View {
    let conversationId: String
    @Environment(TenexCoreManager.self) var coreManager
    @State private var messages: [Message] = []
    @State private var isLoading = false
    @State private var loadTask: Task<Void, Never>?
    @State private var userPubkey: String = ""

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
                            InboxMessageBubble(message: message, userPubkey: userPubkey)
                        }
                    }
                    .padding()
                }
            }
        }
        .navigationTitle("Conversation")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .task {
            userPubkey = (await coreManager.safeCore.getCurrentUser())?.pubkey ?? ""
            await loadMessages()
        }
        .onChange(of: coreManager.messagesByConversation) { _, _ in
            if let updated = coreManager.messagesByConversation[conversationId] {
                messages = updated
            }
        }
        .onDisappear {
            loadTask?.cancel()
        }
    }

    private func loadMessages() async {
        loadTask?.cancel()

        let task = Task { @MainActor in
            isLoading = true
            defer { isLoading = false }

            await coreManager.ensureMessagesLoaded(conversationId: conversationId)
            let fetched = coreManager.messagesByConversation[conversationId] ?? []
            self.messages = fetched
        }

        loadTask = task
        await task.value
    }
}

// MARK: - Inbox Message Bubble (Simplified from ConversationsView)

struct InboxMessageBubble: View {
    let message: Message
    let userPubkey: String

    private var isUser: Bool {
        !userPubkey.isEmpty && message.pubkey == userPubkey
    }

    private var displayName: String {
        let pk = message.pubkey
        if pk.count > 12 {
            return "\(pk.prefix(8))...\(pk.suffix(4))"
        }
        return pk
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if isUser { Spacer(minLength: 50) }

            VStack(alignment: isUser ? .trailing : .leading, spacing: 4) {
                // Author header
                HStack(spacing: 6) {
                    if !isUser {
                        Circle()
                            .fill(Color.agentBrand.gradient)
                            .frame(width: 24, height: 24)
                            .overlay {
                                Image(systemName: "sparkle")
                                    .font(.caption2)
                                    .foregroundStyle(.white)
                            }
                    }

                    Text(isUser ? "You" : displayName)
                        .font(.caption)
                        .fontWeight(.medium)
                        .foregroundStyle(.secondary)

                    RelativeTimeText(timestamp: message.createdAt, style: .localizedAbbreviated)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)

                    if isUser {
                        Circle()
                            .fill(Color.messageUserAvatarColor.gradient)
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
                    .background(isUser ? Color.messageBubbleUserBackground : Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }

            if !isUser { Spacer(minLength: 50) }
        }
    }
}

// MARK: - InboxItem Identifiable

extension InboxItem: Identifiable {}
