import SwiftUI

/// Display mode for the unified nudge/skill selector.
enum NudgeSkillSelectorMode: String, CaseIterable, Identifiable {
    case all
    case nudges
    case skills

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: return "All"
        case .nudges: return "Nudges"
        case .skills: return "Skills"
        }
    }
}

/// Unified sheet for selecting both nudges and skills.
/// Supports bookmarking items and filtering to show only bookmarked items.
struct NudgeSkillSelectorSheet: View {
    let nudges: [Nudge]
    let skills: [Skill]
    @Binding var selectedNudgeIds: Set<String>
    @Binding var selectedSkillIds: Set<String>
    /// Current bookmark set from TenexCoreManager.bookmarkedIds — used to seed the local copy.
    var bookmarkedIds: Set<String> = []
    var initialMode: NudgeSkillSelectorMode = .all
    var initialSearchQuery: String = ""
    var onDone: (() -> Void)?
    /// Called with the base item ID when the user toggles a bookmark. Caller is responsible for
    /// persisting the change via FFI; this sheet applies the toggle optimistically.
    var onToggleBookmark: ((String) -> Void)?

    @Environment(\.dismiss) private var dismiss

    @State private var localSelectedNudgeIds: Set<String> = []
    @State private var localSelectedSkillIds: Set<String> = []
    /// Optimistic local bookmark state — seeded from `bookmarkedIds` on appear.
    @State private var localBookmarkedIds: Set<String> = []
    @State private var searchText = ""
    @State private var mode: NudgeSkillSelectorMode = .all
    /// When true, only bookmarked items are shown. Defaults to true.
    @State private var showBookmarkedOnly: Bool = true

    fileprivate enum SelectorItem: Identifiable {
        case nudge(Nudge)
        case skill(Skill)

        var id: String {
            switch self {
            case .nudge(let nudge): return "nudge:\(nudge.id)"
            case .skill(let skill): return "skill:\(skill.id)"
            }
        }

        /// The raw Nostr event ID used for bookmark lookups.
        var baseId: String {
            switch self {
            case .nudge(let nudge): return nudge.id
            case .skill(let skill): return skill.id
            }
        }

        var title: String {
            switch self {
            case .nudge(let nudge): return nudge.title
            case .skill(let skill): return skill.title
            }
        }

        var description: String {
            switch self {
            case .nudge(let nudge): return nudge.description
            case .skill(let skill): return skill.description
            }
        }

        var searchKey: String {
            (title + "\n" + description).lowercased()
        }

        var sortKey: String {
            title.lowercased()
        }

        var isSkill: Bool {
            if case .skill = self { return true }
            return false
        }
    }

    private var allItems: [SelectorItem] {
        switch mode {
        case .all:
            return nudges.map { .nudge($0) } + skills.map { .skill($0) }
        case .nudges:
            return nudges.map { .nudge($0) }
        case .skills:
            return skills.map { .skill($0) }
        }
    }

    private var items: [SelectorItem] {
        var base = allItems

        if showBookmarkedOnly {
            base = base.filter { localBookmarkedIds.contains($0.baseId) }
        }

        if !searchText.isEmpty {
            let normalized = searchText.lowercased()
            base = base.filter { $0.searchKey.contains(normalized) }
        }

        return base.sorted { lhs, rhs in
            if lhs.sortKey != rhs.sortKey {
                return lhs.sortKey < rhs.sortKey
            }
            // Stable ordering: nudges before skills for identical titles.
            if lhs.isSkill != rhs.isSkill {
                return !lhs.isSkill
            }
            return lhs.id < rhs.id
        }
    }

    private var selectedNudges: [Nudge] {
        nudges
            .filter { localSelectedNudgeIds.contains($0.id) }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    private var selectedSkills: [Skill] {
        skills
            .filter { localSelectedSkillIds.contains($0.id) }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if !selectedNudges.isEmpty || !selectedSkills.isEmpty {
                    selectedItemsBar
                }

                Picker("Filter", selection: $mode) {
                    ForEach(NudgeSkillSelectorMode.allCases) { mode in
                        Text(mode.title).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 16)
                .padding(.top, 12)
                .padding(.bottom, 8)

                List {
                    if items.isEmpty {
                        emptyStateView
                    } else {
                        ForEach(items) { item in
                            NudgeSkillSelectorRow(
                                item: item,
                                isSelected: isSelected(item: item),
                                isBookmarked: localBookmarkedIds.contains(item.baseId),
                                onTap: { toggle(item: item) },
                                onToggleBookmark: { toggleBookmark(item: item) }
                            )
                            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                                let isItemBookmarked = localBookmarkedIds.contains(item.baseId)
                                Button {
                                    toggleBookmark(item: item)
                                } label: {
                                    Label(
                                        isItemBookmarked ? "Unbookmark" : "Bookmark",
                                        systemImage: isItemBookmarked ? "star.slash" : "star"
                                    )
                                }
                                .tint(isItemBookmarked ? .orange : .yellow)
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
            .searchable(text: $searchText, prompt: "Search nudges and skills...")
            .navigationTitle("Nudges & Skills")
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
                        selectedNudgeIds = localSelectedNudgeIds
                        selectedSkillIds = localSelectedSkillIds
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .keyboardShortcut(.defaultAction)
                }

                ToolbarItem(placement: .topBarLeading) {
                    bookmarkFilterButton
                }
            }
            .onAppear {
                localSelectedNudgeIds = selectedNudgeIds
                localSelectedSkillIds = selectedSkillIds
                localBookmarkedIds = bookmarkedIds
                mode = initialMode
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

    // MARK: - Bookmark Filter Button

    private var bookmarkFilterButton: some View {
        Button {
            showBookmarkedOnly.toggle()
        } label: {
            Image(systemName: showBookmarkedOnly ? "bookmark.fill" : "bookmark")
                .symbolEffect(.bounce, value: showBookmarkedOnly)
        }
        .help(showBookmarkedOnly ? "Showing bookmarked only — tap to show all" : "Tap to show bookmarked only")
        .accessibilityLabel(showBookmarkedOnly ? "Show all items" : "Show bookmarked only")
    }

    // MARK: - Selected Items Bar

    private var selectedItemsBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedNudges, id: \.id) { nudge in
                    selectedChip(label: "/\(nudge.title)", color: Color.projectBrand) {
                        localSelectedNudgeIds.remove(nudge.id)
                    }
                }

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

    // MARK: - Empty State

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            if showBookmarkedOnly && localBookmarkedIds.isEmpty && searchText.isEmpty {
                Image(systemName: "star")
                    .font(.system(.largeTitle))
                    .foregroundStyle(.secondary)
                Text("No Bookmarks Yet")
                    .font(.headline)
                Text("Tap ★ next to any item to bookmark it.\nBookmarked items appear here for quick access.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button {
                    showBookmarkedOnly = false
                } label: {
                    Label("Show All Items", systemImage: "list.bullet")
                }
                .buttonStyle(.bordered)
                .padding(.top, 4)
            } else {
                Image(systemName: searchText.isEmpty ? "bolt.slash" : "magnifyingglass")
                    .font(.system(.largeTitle))
                    .foregroundStyle(.secondary)

                if searchText.isEmpty {
                    Text("No Nudges or Skills")
                        .font(.headline)
                    Text("No nudges or skills are available yet.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                } else {
                    Text("No Results")
                        .font(.headline)
                    Text("No items match \"\(searchText)\"")
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

    // MARK: - Helpers

    private func isSelected(item: SelectorItem) -> Bool {
        switch item {
        case .nudge(let nudge):
            return localSelectedNudgeIds.contains(nudge.id)
        case .skill(let skill):
            return localSelectedSkillIds.contains(skill.id)
        }
    }

    private func toggle(item: SelectorItem) {
        switch item {
        case .nudge(let nudge):
            if localSelectedNudgeIds.contains(nudge.id) {
                localSelectedNudgeIds.remove(nudge.id)
            } else {
                localSelectedNudgeIds.insert(nudge.id)
            }
        case .skill(let skill):
            if localSelectedSkillIds.contains(skill.id) {
                localSelectedSkillIds.remove(skill.id)
            } else {
                localSelectedSkillIds.insert(skill.id)
            }
        }
    }

    private func toggleBookmark(item: SelectorItem) {
        let itemId = item.baseId
        // Optimistic local update so the UI responds immediately.
        if localBookmarkedIds.contains(itemId) {
            localBookmarkedIds.remove(itemId)
        } else {
            localBookmarkedIds.insert(itemId)
        }
        // Persist via FFI (fire-and-forget; Rust will emit BookmarkListChanged callback).
        onToggleBookmark?(itemId)
    }
}

// MARK: - Row View

private struct NudgeSkillSelectorRow: View {
    let item: NudgeSkillSelectorSheet.SelectorItem
    let isSelected: Bool
    let isBookmarked: Bool
    let onTap: () -> Void
    let onToggleBookmark: () -> Void

    private var iconName: String {
        switch item {
        case .nudge:
            return "slash"
        case .skill:
            return "bolt.fill"
        }
    }

    private var iconBackground: Color {
        switch item {
        case .nudge:
            return Color.projectBrandBackground
        case .skill:
            return Color.skillBrandBackground
        }
    }

    private var iconColor: Color {
        switch item {
        case .nudge:
            return Color.projectBrand
        case .skill:
            return Color.skillBrand
        }
    }

    private var title: String {
        switch item {
        case .nudge(let nudge):
            return "/\(nudge.title)"
        case .skill(let skill):
            return skill.title
        }
    }

    private var subtitle: String {
        item.description
    }

    var body: some View {
        HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 8)
                .fill(iconBackground)
                .frame(width: 40, height: 40)
                .overlay {
                    Image(systemName: iconName)
                        .font(.headline)
                        .foregroundStyle(iconColor)
                }

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)
                    .foregroundStyle(.primary)

                if !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }

            Spacer()

            // Bookmark star button — tappable, does not trigger row selection.
            Button(action: onToggleBookmark) {
                Image(systemName: isBookmarked ? "star.fill" : "star")
                    .font(.body)
                    .foregroundStyle(isBookmarked ? Color.yellow : Color.secondary.opacity(0.5))
                    .frame(width: 28, height: 28)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(isBookmarked ? "Remove bookmark" : "Add bookmark")

            // Selection checkbox
            Image(systemName: isSelected ? "checkmark.square.fill" : "square")
                .font(.title2)
                .foregroundStyle(isSelected ? Color.accentColor : .secondary)
        }
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}
