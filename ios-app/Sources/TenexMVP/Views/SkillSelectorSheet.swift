import SwiftUI

enum NudgeSkillSelectorMode: String, CaseIterable, Identifiable {
    case skills

    var id: String { rawValue }

    var title: String {
        "Skills"
    }
}

/// Skill-only selector sheet for attaching skills to a message draft.
struct NudgeSkillSelectorSheet: View {
    let skills: [Skill]
    @Binding var selectedSkillIds: Set<String>
    /// Current bookmark set from TenexCoreManager.bookmarkedIds — used to seed the local copy.
    var bookmarkedIds: Set<String> = []
    var initialMode: NudgeSkillSelectorMode = .skills
    var initialSearchQuery: String = ""
    var onDone: (() -> Void)?
    /// Called with the base item ID when the user toggles a bookmark. Caller is responsible for
    /// persisting the change via FFI; this sheet applies the toggle optimistically.
    var onToggleBookmark: ((String) -> Void)?

    @Environment(\.dismiss) private var dismiss

    @State private var localSelectedSkillIds: Set<String> = []
    /// Optimistic local bookmark state — seeded from `bookmarkedIds` on appear.
    @State private var localBookmarkedIds: Set<String> = []
    @State private var searchText = ""
    /// When true, only bookmarked items are shown. Defaults to true.
    @State private var showBookmarkedOnly: Bool = true

    private var items: [Skill] {
        var base = skills

        if showBookmarkedOnly {
            base = base.filter { localBookmarkedIds.contains($0.id) }
        }

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
                                isBookmarked: localBookmarkedIds.contains(skill.id),
                                onTap: { toggle(skill: skill) },
                                onToggleBookmark: { toggleBookmark(skill: skill) }
                            )
                            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                                let isSkillBookmarked = localBookmarkedIds.contains(skill.id)
                                Button {
                                    toggleBookmark(skill: skill)
                                } label: {
                                    Label(
                                        isSkillBookmarked ? "Unbookmark" : "Bookmark",
                                        systemImage: isSkillBookmarked ? "star.slash" : "star"
                                    )
                                }
                                .tint(isSkillBookmarked ? .orange : .yellow)
                            }
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

                #if os(iOS)
                ToolbarItem(placement: .topBarLeading) {
                    bookmarkFilterButton
                }
                #else
                ToolbarItem(placement: .automatic) {
                    bookmarkFilterButton
                }
                #endif
            }
            .onAppear {
                localSelectedSkillIds = selectedSkillIds
                localBookmarkedIds = bookmarkedIds
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

    private var bookmarkFilterButton: some View {
        Button {
            showBookmarkedOnly.toggle()
        } label: {
            Image(systemName: showBookmarkedOnly ? "bookmark.fill" : "bookmark")
                .symbolEffect(.bounce, value: showBookmarkedOnly)
        }
        .help(showBookmarkedOnly ? "Showing bookmarked only — tap to show all" : "Tap to show bookmarked only")
        .accessibilityLabel(showBookmarkedOnly ? "Show all skills" : "Show bookmarked skills only")
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
            if showBookmarkedOnly && localBookmarkedIds.isEmpty && searchText.isEmpty {
                Image(systemName: "star")
                    .font(.system(.largeTitle))
                    .foregroundStyle(.secondary)
                Text("No Bookmarks Yet")
                    .font(.headline)
                Text("Tap ★ next to any skill to bookmark it.\nBookmarked skills appear here for quick access.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button {
                    showBookmarkedOnly = false
                } label: {
                    Label("Show All Skills", systemImage: "list.bullet")
                }
                .adaptiveGlassButtonStyle()
                .padding(.top, 4)
            } else {
                Image(systemName: searchText.isEmpty ? "bolt.slash" : "magnifyingglass")
                    .font(.system(.largeTitle))
                    .foregroundStyle(.secondary)

                if searchText.isEmpty {
                    Text("No Skills")
                        .font(.headline)
                    Text("No skills are available yet.")
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

    private func toggleBookmark(skill: Skill) {
        if localBookmarkedIds.contains(skill.id) {
            localBookmarkedIds.remove(skill.id)
        } else {
            localBookmarkedIds.insert(skill.id)
        }
        onToggleBookmark?(skill.id)
    }
}

private struct SkillSelectorRow: View {
    let skill: Skill
    let isSelected: Bool
    let isBookmarked: Bool
    let onTap: () -> Void
    let onToggleBookmark: () -> Void

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

                Button(action: onToggleBookmark) {
                    Image(systemName: isBookmarked ? "star.fill" : "star")
                        .foregroundStyle(isBookmarked ? .yellow : .secondary)
                }
                .buttonStyle(.borderless)

                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(isSelected ? Color.skillBrand : .secondary)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
