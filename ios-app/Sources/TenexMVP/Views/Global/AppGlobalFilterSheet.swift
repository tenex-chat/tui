import SwiftUI

struct AppGlobalFilterToolbarButton: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(coreManager.appFilterSummaryLabel, systemImage: filterIcon)
                .lineLimit(1)
        }
        .accessibilityIdentifier("global_filter_button")
        .accessibilityValue(coreManager.appFilterSummaryLabel)
        .help("Filter by time and project")
    }

    private var filterIcon: String {
        coreManager.isAppFilterDefault
            ? "line.3.horizontal.decrease.circle"
            : "line.3.horizontal.decrease.circle.fill"
    }
}

struct AppGlobalFilterSheet: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    @State private var draftProjectIds: Set<String>
    @State private var draftTimeWindow: AppTimeWindow

    init(selectedProjectIds: Set<String>, selectedTimeWindow: AppTimeWindow) {
        _draftProjectIds = State(initialValue: selectedProjectIds)
        _draftTimeWindow = State(initialValue: selectedTimeWindow)
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Time Window") {
                    ForEach(AppTimeWindow.allCases, id: \.self) { window in
                        Button {
                            draftTimeWindow = window
                        } label: {
                            HStack {
                                Text(window.label)
                                Spacer()
                                if draftTimeWindow == window {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.agentBrand)
                                }
                            }
                        }
                        .accessibilityIdentifier("global_filter_time_\(window.rawValue)")
                        .buttonStyle(.borderless)
                    }
                }

                Section("Projects") {
                    Button {
                        draftProjectIds.removeAll()
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: "square.grid.2x2")
                                .foregroundStyle(Color.agentBrand)
                                .frame(width: 24)
                            Text("All Projects")
                            Spacer()
                            if draftProjectIds.isEmpty {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.agentBrand)
                            }
                        }
                    }
                    .accessibilityIdentifier("global_filter_all_projects")
                    .buttonStyle(.borderless)

                    ForEach(sortedProjects, id: \.id) { project in
                        Button {
                            toggleProject(project.id)
                        } label: {
                            HStack(spacing: 10) {
                                RoundedRectangle(cornerRadius: 6)
                                    .fill(deterministicColor(for: project.id).gradient)
                                    .frame(width: 24, height: 24)
                                    .overlay {
                                        Image(systemName: "folder.fill")
                                            .foregroundStyle(.white)
                                            .font(.caption)
                                    }

                                Text(project.title)
                                    .lineLimit(1)

                                if coreManager.projectOnlineStatus[project.id] == true {
                                    Circle()
                                        .fill(Color.presenceOnline)
                                        .frame(width: 8, height: 8)
                                }

                                Spacer()

                                if draftProjectIds.contains(project.id) {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.agentBrand)
                                }
                            }
                        }
                        .buttonStyle(.borderless)
                    }
                }

                Section {
                    Button(role: .none) {
                        resetToDefaults()
                    } label: {
                        Label("Reset to Defaults", systemImage: "arrow.counterclockwise")
                            .fontWeight(.semibold)
                    }
                    .accessibilityIdentifier("global_filter_reset_defaults")
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .navigationTitle("Global Filter")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        applyAndDismiss()
                    }
                    .fontWeight(.semibold)
                    .accessibilityIdentifier("global_filter_done")
                }
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    private var sortedProjects: [ProjectInfo] {
        coreManager.projects.sorted { lhs, rhs in
            let lhsOnline = coreManager.projectOnlineStatus[lhs.id] ?? false
            let rhsOnline = coreManager.projectOnlineStatus[rhs.id] ?? false
            if lhsOnline != rhsOnline { return lhsOnline }
            return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
        }
    }

    private func toggleProject(_ projectId: String) {
        if draftProjectIds.contains(projectId) {
            draftProjectIds.remove(projectId)
        } else {
            draftProjectIds.insert(projectId)
        }
    }

    private func resetToDefaults() {
        draftProjectIds = []
        draftTimeWindow = .defaultValue
    }

    private func applyAndDismiss() {
        coreManager.updateAppFilter(projectIds: draftProjectIds, timeWindow: draftTimeWindow)
        dismiss()
    }
}
