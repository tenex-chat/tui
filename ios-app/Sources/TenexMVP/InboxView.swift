import SwiftUI

// MARK: - Type-Safe Enums

/// Maps to FFI event_type field: "ask" or "mention"
enum InboxEventType: String {
    case ask
    case mention
    case unknown

    init(from string: String) {
        self = InboxEventType(rawValue: string) ?? .unknown
    }

    var info: InboxEventTypeInfo {
        switch self {
        case .ask:
            return InboxEventTypeInfo(icon: "questionmark.circle", label: "Question", shortLabel: "asked you", color: .orange)
        case .mention:
            return InboxEventTypeInfo(icon: "at", label: "Mention", shortLabel: "mentioned you", color: .blue)
        case .unknown:
            return InboxEventTypeInfo(icon: "bell", label: "Notification", shortLabel: "notification", color: .secondary)
        }
    }
}

/// Maps to FFI status field which encodes read state
enum InboxStatus: String {
    case waiting        // Unread in TUI
    case acknowledged   // Read in TUI
    case unknown        // Fallback

    init(from string: String) {
        self = InboxStatus(rawValue: string) ?? .unknown
    }

    var isUnread: Bool {
        self == .waiting
    }
}

/// Single source of truth for event type display info
struct InboxEventTypeInfo {
    let icon: String
    let label: String       // Full label for detail view ("Mention")
    let shortLabel: String  // Short label for list view ("mentioned you")
    let color: Color
}

// MARK: - InboxItem Extensions (domain logic)

extension InboxItem {
    var eventTypeEnum: InboxEventType {
        InboxEventType(from: eventType)
    }

    var status_enum: InboxStatus {
        InboxStatus(from: status)
    }

    var isUnread: Bool {
        status_enum.isUnread
    }

    var eventTypeInfo: InboxEventTypeInfo {
        eventTypeEnum.info
    }

    func matches(filter: InboxFilter) -> Bool {
        filter.matches(eventTypeEnum)
    }
}

// MARK: - Inbox Filter Tab

enum InboxFilter: String, CaseIterable {
    case all = "All"
    case questions = "Questions"
    case mentions = "Mentions"

    func matches(_ eventType: InboxEventType) -> Bool {
        switch self {
        case .all: return true
        case .questions: return eventType == .ask
        case .mentions: return eventType == .mention
        }
    }
}

// MARK: - Date Formatters (cached as static for performance)

private enum InboxDateFormatters {
    static let relativeDateFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    static let fullDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    static func relativeTime(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return relativeDateFormatter.localizedString(for: date, relativeTo: Date())
    }

    static func fullDate(from timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return fullDateFormatter.string(from: date)
    }
}

// MARK: - Inbox View

struct InboxView: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var selectedFilter: InboxFilter = .all
    @State private var selectedItem: InboxItem?
    @State private var pendingNavigation: ConversationNavigationData?
    @State private var navigateToConversation: ConversationNavigationData?

    // MARK: - 48-Hour Cap Constants & Time Invalidation

    /// Hard cap: 48 hours in seconds (48 * 60 * 60 = 172,800).
    /// NOTE: This value is synchronized with Rust constant `tenex_core::constants::INBOX_48H_CAP_SECONDS`.
    /// If changed, update both locations for cross-platform consistency.
    private static let inbox48HourCapSeconds: UInt64 = 48 * 60 * 60

    /// Refresh interval for time-based invalidation (1 minute).
    /// Items crossing the 48h threshold will disappear on next tick.
    private static let refreshIntervalSeconds: TimeInterval = 60

    // MARK: - Computed Properties (Time-Aware)

    /// Items within the 48-hour window from centralized store.
    /// Takes `now` as parameter to ensure consistent filtering across all derived computations.
    private func itemsWithin48Hours(now: UInt64) -> [InboxItem] {
        let cutoff = now > Self.inbox48HourCapSeconds ? now - Self.inbox48HourCapSeconds : 0
        return coreManager.inboxItems.filter { $0.createdAt >= cutoff }
    }

    /// Items filtered by current tab selection (after 48h cap).
    private func filteredItems(now: UInt64) -> [InboxItem] {
        itemsWithin48Hours(now: now).filter { $0.matches(filter: selectedFilter) }
    }

    /// Count of unread items for badge display (within 48h cap).
    private func unreadCount(now: UInt64) -> Int {
        itemsWithin48Hours(now: now).filter(\.isUnread).count
    }

    /// Unread count for a specific filter (within 48h cap).
    private func unreadCount(for filter: InboxFilter, now: UInt64) -> Int {
        if filter == .all {
            return unreadCount(now: now)
        }
        return itemsWithin48Hours(now: now).filter { $0.matches(filter: filter) && $0.isUnread }.count
    }

    var body: some View {
        // TimelineView triggers re-render periodically to invalidate stale items
        // that have crossed the 48h threshold since last render.
        TimelineView(.periodic(from: .now, by: Self.refreshIntervalSeconds)) { context in
            // Compute `now` once per render cycle for consistent filtering
            let now = UInt64(context.date.timeIntervalSince1970)
            let items = filteredItems(now: now)

            NavigationStack {
                VStack(spacing: 0) {
                    // Tab filter bar: All / Questions / Mentions
                    filterTabBar(now: now)

                    Divider()

                    // Inbox list - uses centralized coreManager.inboxItems
                    if items.isEmpty {
                        emptyStateView
                    } else {
                        inboxList(items: items)
                    }
                }
                .navigationTitle("Inbox")
                .navigationBarTitleDisplayMode(.large)
                .sheet(item: $selectedItem, onDismiss: {
                    // Handle navigation after sheet dismisses deterministically
                    if let pending = pendingNavigation {
                        navigateToConversation = pending
                        pendingNavigation = nil
                    }
                }) { item in
                    InboxDetailView(item: item, onNavigateToConversation: { convId in
                        // Store pending navigation, then dismiss sheet
                        pendingNavigation = ConversationNavigationData(
                            conversationId: convId
                        )
                        selectedItem = nil
                    })
                    .environmentObject(coreManager)
                }
                .navigationDestination(item: $navigateToConversation) { navData in
                    InboxConversationView(
                        conversationId: navData.conversationId
                    )
                    .environmentObject(coreManager)
                }
            }
        }
    }

    // MARK: - Filter Tab Bar

    private func filterTabBar(now: UInt64) -> some View {
        HStack(spacing: 0) {
            ForEach(InboxFilter.allCases, id: \.self) { filter in
                filterTab(for: filter, now: now)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color.systemBackground)
    }

    private func filterTab(for filter: InboxFilter, now: UInt64) -> some View {
        let count = unreadCount(for: filter, now: now)

        return Button(action: { selectedFilter = filter }) {
            HStack(spacing: 4) {
                Text(filter.rawValue)
                    .font(.subheadline)
                    .fontWeight(selectedFilter == filter ? .semibold : .regular)

                if count > 0 {
                    Text("(\(count))")
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(selectedFilter == filter ? Color.blue.opacity(0.15) : Color.clear)
            .foregroundStyle(selectedFilter == filter ? .blue : .primary)
            .clipShape(Capsule())
        }
        .buttonStyle(.plain)
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
        switch selectedFilter {
        case .all:
            return "No items waiting for your attention"
        case .questions:
            return "No questions to answer"
        case .mentions:
            return "No mentions to review"
        }
    }

    // MARK: - Inbox List

    private func inboxList(items: [InboxItem]) -> some View {
        List {
            ForEach(items, id: \.id) { item in
                Button(action: { selectedItem = item }) {
                    InboxItemRow(item: item)
                }
                .buttonStyle(.plain)
            }
        }
        .listStyle(.plain)
    }

}

// MARK: - Inbox Item Row

struct InboxItemRow: View {
    let item: InboxItem

    var body: some View {
        HStack(spacing: 12) {
            // Unread indicator dot
            Circle()
                .fill(item.isUnread ? Color.blue : Color.clear)
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

                    Text(InboxDateFormatters.relativeTime(from: item.createdAt))
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }

                // Event type label (using short label from domain extension)
                HStack(spacing: 4) {
                    Text(item.fromAgent)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Text("•")
                        .foregroundStyle(.tertiary)

                    Text(item.eventTypeInfo.shortLabel)
                        .font(.caption)
                        .foregroundStyle(item.eventTypeInfo.color)
                }

                // Project info if available
                if let projectId = item.projectId {
                    HStack(spacing: 4) {
                        Image(systemName: "folder")
                            .font(.caption2)
                        Text(projectId)
                            .font(.caption)
                    }
                    .foregroundStyle(.tertiary)
                }
            }

            // Chevron for navigation
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 8)
        .opacity(item.isUnread ? 1.0 : 0.7)
    }
}

// MARK: - Inbox Detail View

struct InboxDetailView: View {
    let item: InboxItem
    let onNavigateToConversation: (String) -> Void
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Header with badges
                    headerSection

                    Divider()

                    // Content
                    contentSection

                    // Related info with navigation
                    if item.projectId != nil || item.conversationId != nil {
                        Divider()
                        relatedSection
                    }

                    Spacer()
                }
                .padding()
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
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
                .background(item.isUnread ? Color.orange.opacity(0.15) : Color.green.opacity(0.15))
                .foregroundStyle(item.isUnread ? .orange : .green)
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
                    Text(item.fromAgent)
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
           let conversationId = item.conversationId,
           let projectId = item.projectId {
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
            .environmentObject(coreManager)
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

            if let projectId = item.projectId {
                HStack {
                    Image(systemName: "folder.fill")
                        .foregroundStyle(.blue)
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
            if let convId = item.conversationId {
                Button(action: {
                    onNavigateToConversation(convId)
                }) {
                    HStack {
                        Image(systemName: "bubble.left.and.bubble.right.fill")
                            .foregroundStyle(.green)
                        Text("View Conversation")
                        Spacer()
                        Image(systemName: "chevron.right")
                            .foregroundStyle(.tertiary)
                    }
                    .padding()
                    .background(Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                }
                .buttonStyle(.plain)
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
    // Note: projectId removed - conversation IDs are globally unique Nostr event IDs
    // and getMessages(conversationId:) doesn't accept projectId parameter
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @State private var loadTask: Task<Void, Never>?

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
                            InboxMessageBubble(message: message)
                        }
                    }
                    .padding()
                }
            }
        }
        .navigationTitle("Conversation")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await loadMessages()
        }
        .onReceive(coreManager.$messagesByConversation) { cache in
            if let updated = cache[conversationId] {
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

                    Text(InboxDateFormatters.relativeTime(from: message.createdAt))
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
            }

            if !isUser { Spacer(minLength: 50) }
        }
    }
}

// MARK: - InboxItem Identifiable

extension InboxItem: Identifiable {}
