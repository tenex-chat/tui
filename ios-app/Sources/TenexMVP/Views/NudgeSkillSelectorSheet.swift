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
struct NudgeSkillSelectorSheet: View {
    let nudges: [NudgeInfo]
    let skills: [SkillInfo]
    @Binding var selectedNudgeIds: Set<String>
    @Binding var selectedSkillIds: Set<String>
    var initialMode: NudgeSkillSelectorMode = .all
    var initialSearchQuery: String = ""
    var onDone: (() -> Void)?

    @Environment(\.dismiss) private var dismiss

    @State private var localSelectedNudgeIds: Set<String> = []
    @State private var localSelectedSkillIds: Set<String> = []
    @State private var searchText = ""
    @State private var mode: NudgeSkillSelectorMode = .all

    fileprivate enum SelectorItem: Identifiable {
        case nudge(NudgeInfo)
        case skill(SkillInfo)

        var id: String {
            switch self {
            case .nudge(let nudge): return "nudge:\(nudge.id)"
            case .skill(let skill): return "skill:\(skill.id)"
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

    private var items: [SelectorItem] {
        var base: [SelectorItem]
        switch mode {
        case .all:
            base = nudges.map { .nudge($0) } + skills.map { .skill($0) }
        case .nudges:
            base = nudges.map { .nudge($0) }
        case .skills:
            base = skills.map { .skill($0) }
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

    private var selectedNudges: [NudgeInfo] {
        nudges
            .filter { localSelectedNudgeIds.contains($0.id) }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    private var selectedSkills: [SkillInfo] {
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
                            NudgeSkillSelectorRow(item: item, isSelected: isSelected(item: item)) {
                                toggle(item: item)
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
            }
            .onAppear {
                localSelectedNudgeIds = selectedNudgeIds
                localSelectedSkillIds = selectedSkillIds
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

    private var emptyStateView: some View {
        VStack(spacing: 16) {
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
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

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
}

private struct NudgeSkillSelectorRow: View {
    let item: NudgeSkillSelectorSheet.SelectorItem
    let isSelected: Bool
    let onTap: () -> Void

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

            Image(systemName: isSelected ? "checkmark.square.fill" : "square")
                .font(.title2)
                .foregroundStyle(isSelected ? Color.agentBrand : .secondary)
        }
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
    }
}
