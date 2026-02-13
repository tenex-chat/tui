import SwiftUI

/// A sheet for selecting nudges to include in a new conversation.
/// Uses NudgeInfo from FFI (kind:4201 events) with multi-select support.
struct NudgeSelectorSheet: View {
    // MARK: - Properties

    /// Available nudges to choose from
    let nudges: [NudgeInfo]

    /// Currently selected nudge IDs (binding for multi-select)
    @Binding var selectedNudgeIds: Set<String>

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    // MARK: - Environment

    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedIds: Set<String> = []
    @State private var searchText = ""

    // MARK: - Computed

    private var filteredNudges: [NudgeInfo] {
        if searchText.isEmpty {
            return nudges
        }

        let lowercasedSearch = searchText.lowercased()
        return nudges.filter { nudge in
            nudge.title.lowercased().contains(lowercasedSearch) ||
            nudge.description.lowercased().contains(lowercasedSearch)
        }
    }

    private var selectedNudges: [NudgeInfo] {
        nudges.filter { localSelectedIds.contains($0.id) }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Selected nudges bar (if any)
                if !selectedNudges.isEmpty {
                    selectedNudgesBar
                }

                // Search and nudge list
                List {
                    if filteredNudges.isEmpty {
                        emptyStateView
                    } else {
                        ForEach(filteredNudges, id: \.id) { nudge in
                            NudgeRowView(
                                nudge: nudge,
                                isSelected: localSelectedIds.contains(nudge.id)
                            ) {
                                toggleNudge(nudge)
                            }
                        }
                    }
                }
                .listStyle(.plain)
                .contentMargins(.top, 0, for: .scrollContent)
            }
            .searchable(text: $searchText, prompt: "Search nudges...")
            .navigationTitle("Select Nudges")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        // Discard local changes
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Done") {
                        // Commit local changes to parent binding
                        selectedNudgeIds = localSelectedIds
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
            .onAppear {
                // Initialize local state from parent binding
                localSelectedIds = selectedNudgeIds
            }
        }
    }

    // MARK: - Subviews

    private var selectedNudgesBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedNudges, id: \.id) { nudge in
                    HStack(spacing: 4) {
                        Text("/\(nudge.title)")
                            .font(.caption)
                            .fontWeight(.medium)

                        Button(action: { localSelectedIds.remove(nudge.id) }) {
                            Image(systemName: "xmark.circle.fill")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(
                        Capsule()
                            .fill(Color.systemBackground)
                            .shadow(color: .black.opacity(0.1), radius: 1, x: 0, y: 1)
                    )
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(Color.systemGray6)
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: searchText.isEmpty ? "list.bullet.clipboard" : "magnifyingglass")
                .font(.system(.largeTitle))
                .foregroundStyle(.secondary)

            if searchText.isEmpty {
                Text("No Nudges Available")
                    .font(.headline)
                Text("No nudges have been defined yet.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Text("No Results")
                    .font(.headline)
                Text("No nudges match \"\(searchText)\"")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Actions

    private func toggleNudge(_ nudge: NudgeInfo) {
        if localSelectedIds.contains(nudge.id) {
            localSelectedIds.remove(nudge.id)
        } else {
            localSelectedIds.insert(nudge.id)
        }
    }
}

// MARK: - NudgeInfo Identifiable

extension NudgeInfo: Identifiable {}

// MARK: - Nudge Row View

struct NudgeRowView: View {
    let nudge: NudgeInfo
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                // Nudge icon
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.purple.opacity(0.15))
                    .frame(width: 40, height: 40)
                    .overlay {
                        Text("/")
                            .font(.headline)
                            .fontWeight(.bold)
                            .foregroundStyle(.purple)
                    }

                // Nudge info
                VStack(alignment: .leading, spacing: 4) {
                    Text("/\(nudge.title)")
                        .font(.headline)
                        .foregroundStyle(.primary)

                    if !nudge.description.isEmpty {
                        Text(nudge.description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                }

                Spacer()

                // Selection indicator (checkbox for multi-select)
                Image(systemName: isSelected ? "checkmark.square.fill" : "square")
                    .font(.title2)
                    .foregroundStyle(isSelected ? .blue : .secondary)
            }
            .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Nudge Chip View (for display in composer)

struct NudgeChipView: View {
    let nudge: NudgeInfo
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Slash icon
            Text("/")
                .font(.subheadline)
                .fontWeight(.bold)
                .foregroundStyle(.purple)

            // Nudge title
            Text(nudge.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Remove button
            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.systemBackground)
                .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
        )
    }
}

// MARK: - Preview

#Preview {
    NudgeSelectorSheet(
        nudges: [
            NudgeInfo(
                id: "nudge1",
                pubkey: "abc123",
                title: "debugging",
                description: "Enable verbose debugging output"
            ),
            NudgeInfo(
                id: "nudge2",
                pubkey: "abc123",
                title: "code-review",
                description: "Focus on code quality and best practices"
            ),
            NudgeInfo(
                id: "nudge3",
                pubkey: "abc123",
                title: "testing",
                description: "Write comprehensive tests"
            )
        ],
        selectedNudgeIds: .constant(["nudge1"])
    )
}
