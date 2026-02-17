import SwiftUI

/// A sheet for selecting skills to include in a new conversation.
/// Uses SkillInfo from FFI (kind:4202 events) with multi-select support.
/// Skills differ from nudges by potentially having file attachments.
struct SkillSelectorSheet: View {
    // MARK: - Properties

    /// Available skills to choose from
    let skills: [SkillInfo]

    /// Currently selected skill IDs (binding for multi-select)
    @Binding var selectedSkillIds: Set<String>

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    // MARK: - Environment

    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedIds: Set<String> = []
    @State private var searchText = ""

    // MARK: - Computed

    private var filteredSkills: [SkillInfo] {
        if searchText.isEmpty {
            return skills
        }

        let lowercasedSearch = searchText.lowercased()
        return skills.filter { skill in
            skill.title.lowercased().contains(lowercasedSearch) ||
            skill.description.lowercased().contains(lowercasedSearch)
        }
    }

    private var selectedSkills: [SkillInfo] {
        skills.filter { localSelectedIds.contains($0.id) }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Selected skills bar (if any)
                if !selectedSkills.isEmpty {
                    selectedSkillsBar
                }

                // Search and skill list
                List {
                    if filteredSkills.isEmpty {
                        emptyStateView
                    } else {
                        ForEach(filteredSkills, id: \.id) { skill in
                            SkillRowView(
                                skill: skill,
                                isSelected: localSelectedIds.contains(skill.id)
                            ) {
                                toggleSkill(skill)
                            }
                        }
                    }
                }
                .listStyle(.plain)
                .contentMargins(.top, 0, for: .scrollContent)
            }
            .searchable(text: $searchText, prompt: "Search skills...")
            #if os(iOS)
            .navigationTitle("Select Skills")
            .navigationBarTitleDisplayMode(.inline)
            #else
            .navigationTitle("")
            #endif
            .toolbar {
                #if os(macOS)
                ToolbarItem(placement: .principal) {
                    Text("Select Skills").fontWeight(.semibold)
                }
                #endif
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        // Discard local changes
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Done") {
                        // Commit local changes to parent binding
                        selectedSkillIds = localSelectedIds
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
            .onAppear {
                // Initialize local state from parent binding
                localSelectedIds = selectedSkillIds
            }
        }
        #if os(iOS)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        #else
        .frame(minWidth: 480, idealWidth: 540, minHeight: 420, idealHeight: 520)
        #endif
    }

    // MARK: - Subviews

    private var selectedSkillsBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedSkills, id: \.id) { skill in
                    HStack(spacing: 4) {
                        Image(systemName: "bolt.fill")
                            .font(.caption2)
                            .foregroundStyle(Color.skillBrand)

                        Text(skill.title)
                            .font(.caption)
                            .fontWeight(.medium)

                        Button(action: { localSelectedIds.remove(skill.id) }) {
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
            Image(systemName: searchText.isEmpty ? "bolt.slash" : "magnifyingglass")
                .font(.system(.largeTitle))
                .foregroundStyle(.secondary)

            if searchText.isEmpty {
                Text("No Skills Available")
                    .font(.headline)
                Text("No skills have been defined yet.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Text("No Results")
                    .font(.headline)
                Text("No skills match \"\(searchText)\"")
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

    private func toggleSkill(_ skill: SkillInfo) {
        if localSelectedIds.contains(skill.id) {
            localSelectedIds.remove(skill.id)
        } else {
            localSelectedIds.insert(skill.id)
        }
    }
}

// MARK: - SkillInfo Identifiable

extension SkillInfo: Identifiable {}

// MARK: - Skill Row View

struct SkillRowView: View {
    let skill: SkillInfo
    let isSelected: Bool
    let onTap: () -> Void

    /// Check if skill has file attachments (content is non-empty)
    private var hasFiles: Bool {
        !skill.content.isEmpty
    }

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                // Skill icon (bolt for skills, distinguishes from nudges which use "/")
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.skillBrandBackground)
                    .frame(width: 40, height: 40)
                    .overlay {
                        Image(systemName: "bolt.fill")
                            .font(.headline)
                            .foregroundStyle(Color.skillBrand)
                    }

                // Skill info
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(skill.title)
                            .font(.headline)
                            .foregroundStyle(.primary)

                        // File attachment indicator
                        if hasFiles {
                            Image(systemName: "doc.fill")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if !skill.description.isEmpty {
                        Text(skill.description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                }

                Spacer()

                // Selection indicator (checkbox for multi-select)
                Image(systemName: isSelected ? "checkmark.square.fill" : "square")
                    .font(.title2)
                    .foregroundStyle(isSelected ? Color.skillBrand : .secondary)
            }
            .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Skill Chip View (for display in composer)

struct SkillChipView: View {
    let skill: SkillInfo
    let onRemove: () -> Void

    /// Check if skill has file attachments
    private var hasFiles: Bool {
        !skill.content.isEmpty
    }

    var body: some View {
        HStack(spacing: 6) {
            // Bolt icon
            Image(systemName: "bolt.fill")
                .font(.subheadline)
                .foregroundStyle(Color.skillBrand)

            // Skill title
            Text(skill.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // File indicator
            if hasFiles {
                Image(systemName: "doc.fill")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

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
    SkillSelectorSheet(
        skills: [
            SkillInfo(
                id: "skill1",
                pubkey: "abc123",
                title: "code-review",
                description: "Perform thorough code reviews",
                content: "Review code for best practices, security issues, and performance..."
            ),
            SkillInfo(
                id: "skill2",
                pubkey: "abc123",
                title: "testing",
                description: "Write comprehensive tests",
                content: ""
            ),
            SkillInfo(
                id: "skill3",
                pubkey: "abc123",
                title: "documentation",
                description: "Generate documentation from code",
                content: "Parse code and generate markdown documentation..."
            )
        ],
        selectedSkillIds: .constant(["skill1"])
    )
}
