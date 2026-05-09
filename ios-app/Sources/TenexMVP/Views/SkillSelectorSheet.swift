import SwiftUI

/// Skill selector sheet for attaching skills to a message draft.
///
/// Skills are passed in pre-filtered by the caller (typically the union of skills
/// from the project's kind:24010 advertisement and the per-agent kind:0 metadata
/// of roster agents).
struct SkillSelectorSheet: View {
    let skills: [Skill]
    @Binding var selectedSkillIds: Set<String>
    var initialSearchQuery: String = ""
    var onDone: (() -> Void)?

    @Environment(\.dismiss) private var dismiss

    @State private var localSelectedSkillIds: Set<String> = []
    @State private var searchText = ""

    private var items: [Skill] {
        var base = skills

        if !searchText.isEmpty {
            let normalized = searchText.lowercased()
            base = base.filter { skill in
                let searchKey = (skill.title + "\n" + skill.description).lowercased()
                return searchKey.contains(normalized)
            }
        }

        return base.sorted { lhs, rhs in
            let lhsKey = lhs.title.lowercased()
            let rhsKey = rhs.title.lowercased()
            if lhsKey != rhsKey {
                return lhsKey < rhsKey
            }
            return lhs.id < rhs.id
        }
    }

    private var selectedSkills: [Skill] {
        skills
            .filter { localSelectedSkillIds.contains($0.id) }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if !selectedSkills.isEmpty {
                    selectedItemsBar
                }

                List {
                    if items.isEmpty {
                        emptyStateView
                    } else {
                        ForEach(items, id: \.id) { skill in
                            SkillSelectorRow(
                                skill: skill,
                                isSelected: localSelectedSkillIds.contains(skill.id),
                                onTap: { toggle(skill: skill) }
                            )
                        }
                    }
                }
                #if os(iOS)
                .listStyle(.insetGrouped)
                #else
                .listStyle(.inset)
                #endif
            }
            .searchable(text: $searchText, prompt: "Search skills...")
            .navigationTitle("Skills")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Done") {
                        selectedSkillIds = localSelectedSkillIds
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .keyboardShortcut(.defaultAction)
                }
            }
            .onAppear {
                localSelectedSkillIds = selectedSkillIds
                if !initialSearchQuery.isEmpty {
                    searchText = initialSearchQuery
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 520, idealWidth: 580, minHeight: 460, idealHeight: 560)
        #endif
    }

    private var selectedItemsBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedSkills, id: \.id) { skill in
                    selectedChip(label: "Skill: \(skill.title)", color: Color.skillBrand) {
                        localSelectedSkillIds.remove(skill.id)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    private func selectedChip(label: String, color: Color, onRemove: @escaping () -> Void) -> some View {
        HStack(spacing: 6) {
            Text(label)
                .font(.caption)
                .fontWeight(.medium)
                .foregroundStyle(color)

            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(
            Capsule()
                .fill(Color.systemBackground)
                .shadow(color: .black.opacity(0.1), radius: 1, x: 0, y: 1)
        )
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: searchText.isEmpty ? "bolt.slash" : "magnifyingglass")
                .font(.system(.largeTitle))
                .foregroundStyle(.secondary)

            if searchText.isEmpty {
                Text("No Skills")
                    .font(.headline)
                Text("No skills are available for this project.")
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

    private func toggle(skill: Skill) {
        if localSelectedSkillIds.contains(skill.id) {
            localSelectedSkillIds.remove(skill.id)
        } else {
            localSelectedSkillIds.insert(skill.id)
        }
    }
}

private struct SkillSelectorRow: View {
    let skill: Skill
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.skillBrandBackground)
                    .frame(width: 40, height: 40)
                    .overlay {
                        Image(systemName: "bolt.fill")
                            .font(.headline)
                            .foregroundStyle(Color.skillBrand)
                    }

                VStack(alignment: .leading, spacing: 4) {
                    Text(skill.title)
                        .font(.headline)
                        .foregroundStyle(.primary)

                    if !skill.description.isEmpty {
                        Text(skill.description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                }

                Spacer(minLength: 12)

                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(isSelected ? Color.skillBrand : .secondary)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
