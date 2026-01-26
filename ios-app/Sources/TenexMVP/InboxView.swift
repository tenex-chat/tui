import SwiftUI

struct InboxView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var inboxItems: [InboxItem] = []
    @State private var isLoading = false
    @State private var selectedItem: InboxItem?
    @State private var filterPriority: String? = nil

    var filteredItems: [InboxItem] {
        guard let priority = filterPriority else { return inboxItems }
        return inboxItems.filter { $0.priority == priority }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Filter bar
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        FilterChip(title: "All", isSelected: filterPriority == nil) {
                            filterPriority = nil
                        }
                        FilterChip(title: "High", isSelected: filterPriority == "high", color: .red) {
                            filterPriority = filterPriority == "high" ? nil : "high"
                        }
                        FilterChip(title: "Medium", isSelected: filterPriority == "medium", color: .orange) {
                            filterPriority = filterPriority == "medium" ? nil : "medium"
                        }
                        FilterChip(title: "Low", isSelected: filterPriority == "low", color: .gray) {
                            filterPriority = filterPriority == "low" ? nil : "low"
                        }
                    }
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                }
                .background(Color(.systemBackground))

                Divider()

                // Inbox list
                if isLoading {
                    Spacer()
                    ProgressView("Loading inbox...")
                    Spacer()
                } else if filteredItems.isEmpty {
                    Spacer()
                    VStack(spacing: 16) {
                        Image(systemName: "tray")
                            .font(.system(size: 60))
                            .foregroundStyle(.secondary)
                        Text("Inbox Empty")
                            .font(.title2)
                            .fontWeight(.semibold)
                        Text("No items waiting for your attention")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                } else {
                    List {
                        ForEach(filteredItems, id: \.id) { item in
                            InboxItemRow(item: item)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    selectedItem = item
                                }
                        }
                    }
                    .listStyle(.plain)
                }
            }
            .navigationTitle("Inbox")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: loadInbox) {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(isLoading)
                }
            }
            .onAppear {
                loadInbox()
            }
            .sheet(item: $selectedItem) { item in
                InboxDetailView(item: item)
            }
        }
    }

    private func loadInbox() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            let fetched = coreManager.core.getInbox()
            DispatchQueue.main.async {
                self.inboxItems = fetched
                self.isLoading = false
            }
        }
    }
}

// MARK: - Filter Chip

struct FilterChip: View {
    let title: String
    let isSelected: Bool
    var color: Color = .blue
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline)
                .fontWeight(isSelected ? .semibold : .regular)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(isSelected ? color.opacity(0.15) : Color(.systemGray6))
                .foregroundStyle(isSelected ? color : .primary)
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Inbox Item Row

struct InboxItemRow: View {
    let item: InboxItem

    var body: some View {
        HStack(spacing: 12) {
            // Priority indicator
            Circle()
                .fill(priorityColor)
                .frame(width: 12, height: 12)

            // Status icon
            Image(systemName: statusIcon)
                .foregroundStyle(statusColor)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.title)
                        .font(.headline)
                        .lineLimit(1)

                    Spacer()

                    Text(formatTime(item.createdAt))
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }

                Text(item.content)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 8) {
                    HStack(spacing: 4) {
                        Image(systemName: "person.circle")
                            .font(.caption2)
                        Text(item.fromAgent)
                            .font(.caption)
                    }
                    .foregroundStyle(.tertiary)

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
            }
        }
        .padding(.vertical, 8)
    }

    private var priorityColor: Color {
        switch item.priority {
        case "high": return .red
        case "medium": return .orange
        case "low": return .gray
        default: return .blue
        }
    }

    private var statusIcon: String {
        switch item.status {
        case "waiting": return "clock.fill"
        case "acknowledged": return "eye.fill"
        case "resolved": return "checkmark.circle.fill"
        default: return "questionmark.circle"
        }
    }

    private var statusColor: Color {
        switch item.status {
        case "waiting": return .orange
        case "acknowledged": return .blue
        case "resolved": return .green
        default: return .gray
        }
    }

    private func formatTime(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}

// MARK: - Inbox Detail View

struct InboxDetailView: View {
    let item: InboxItem
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Header
                    VStack(alignment: .leading, spacing: 12) {
                        // Priority badge
                        HStack {
                            HStack(spacing: 4) {
                                Circle()
                                    .fill(priorityColor)
                                    .frame(width: 8, height: 8)
                                Text(item.priority.capitalized)
                                    .font(.caption)
                                    .fontWeight(.medium)
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(priorityColor.opacity(0.15))
                            .foregroundStyle(priorityColor)
                            .clipShape(Capsule())

                            HStack(spacing: 4) {
                                Image(systemName: statusIcon)
                                Text(item.status.capitalized)
                                    .font(.caption)
                                    .fontWeight(.medium)
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(statusColor.opacity(0.15))
                            .foregroundStyle(statusColor)
                            .clipShape(Capsule())

                            Spacer()
                        }

                        Text(item.title)
                            .font(.title)
                            .fontWeight(.bold)

                        // Metadata
                        HStack(spacing: 16) {
                            HStack(spacing: 6) {
                                Image(systemName: "person.circle.fill")
                                Text(item.fromAgent)
                            }
                            .foregroundStyle(.secondary)

                            HStack(spacing: 6) {
                                Image(systemName: "clock")
                                Text(formatDate(item.createdAt))
                            }
                            .foregroundStyle(.secondary)
                        }
                        .font(.subheadline)
                    }

                    Divider()

                    // Content
                    Text(item.content)
                        .font(.body)

                    // Related info
                    if item.projectId != nil || item.conversationId != nil {
                        Divider()

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
                                .background(Color(.systemGray6))
                                .clipShape(RoundedRectangle(cornerRadius: 10))
                            }

                            if let convId = item.conversationId {
                                HStack {
                                    Image(systemName: "bubble.left.and.bubble.right.fill")
                                        .foregroundStyle(.green)
                                    Text("Conversation: \(convId)")
                                    Spacer()
                                    Image(systemName: "chevron.right")
                                        .foregroundStyle(.tertiary)
                                }
                                .padding()
                                .background(Color(.systemGray6))
                                .clipShape(RoundedRectangle(cornerRadius: 10))
                            }
                        }
                    }

                    // Actions
                    Divider()

                    VStack(spacing: 12) {
                        Button(action: {}) {
                            Label("Mark as Resolved", systemImage: "checkmark.circle")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.borderedProminent)

                        Button(action: {}) {
                            Label("Acknowledge", systemImage: "eye")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.bordered)
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

    private var priorityColor: Color {
        switch item.priority {
        case "high": return .red
        case "medium": return .orange
        case "low": return .gray
        default: return .blue
        }
    }

    private var statusIcon: String {
        switch item.status {
        case "waiting": return "clock.fill"
        case "acknowledged": return "eye.fill"
        case "resolved": return "checkmark.circle.fill"
        default: return "questionmark.circle"
        }
    }

    private var statusColor: Color {
        switch item.status {
        case "waiting": return .orange
        case "acknowledged": return .blue
        case "resolved": return .green
        default: return .gray
        }
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
}

// MARK: - InboxItem Identifiable

extension InboxItem: Identifiable {}
