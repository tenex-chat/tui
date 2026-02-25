import SwiftUI

struct AppGlobalFilterToolbarButton: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var isPresented = false

    var body: some View {
        Button {
            isPresented = true
        } label: {
            filterButtonLabel
        }
        .accessibilityIdentifier("global_filter_button")
        .accessibilityLabel("Global Filter")
        .accessibilityValue(coreManager.appFilterSummaryLabel)
        .help("Filter by time, schedule, projects, status, and hashtags")
        .sheet(isPresented: $isPresented) {
            AppGlobalFilterSheet(
                selectedProjectIds: coreManager.appFilterProjectIds,
                selectedTimeWindow: coreManager.appFilterTimeWindow,
                selectedScheduledEvent: coreManager.appFilterScheduledEvent,
                selectedStatus: coreManager.appFilterStatus,
                selectedHashtags: coreManager.appFilterHashtags
            )
            .environment(coreManager)
        }
    }

    @ViewBuilder
    private var filterButtonLabel: some View {
        #if os(macOS)
        Image(systemName: filterIcon)
        #else
        Label(coreManager.appFilterSummaryLabel, systemImage: filterIcon)
            .lineLimit(1)
        #endif
    }

    private var filterIcon: String {
        coreManager.isAppFilterDefault
            ? "line.3.horizontal.decrease.circle"
            : "line.3.horizontal.decrease.circle.fill"
    }
}

struct AppGlobalFilterSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    @State private var draftProjectIds: Set<String>
    @State private var draftTimeWindow: AppTimeWindow
    @State private var draftScheduledEvent: ScheduledEventFilter
    @State private var draftStatus: ConversationStatusFilter
    @State private var draftHashtags: Set<String>

    init(
        selectedProjectIds: Set<String>,
        selectedTimeWindow: AppTimeWindow,
        selectedScheduledEvent: ScheduledEventFilter,
        selectedStatus: ConversationStatusFilter,
        selectedHashtags: Set<String>
    ) {
        _draftProjectIds = State(initialValue: selectedProjectIds)
        _draftTimeWindow = State(initialValue: selectedTimeWindow)
        _draftScheduledEvent = State(initialValue: selectedScheduledEvent)
        _draftStatus = State(initialValue: selectedStatus)
        _draftHashtags = State(initialValue: selectedHashtags)
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

                Section("Scheduled Events") {
                    ForEach(ScheduledEventFilter.allCases, id: \.self) { filter in
                        Button {
                            draftScheduledEvent = filter
                        } label: {
                            HStack {
                                Text(filter.label)
                                Spacer()
                                if draftScheduledEvent == filter {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(Color.agentBrand)
                                }
                            }
                        }
                        .accessibilityIdentifier("global_filter_scheduled_\(filter.rawValue)")
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

                Section("Status") {
                    Button {
                        draftStatus = .all
                    } label: {
                        HStack {
                            Text("All Statuses")
                            Spacer()
                            if draftStatus == .all {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.agentBrand)
                            }
                        }
                    }
                    .buttonStyle(.borderless)

                    if coreManager.appFilterAvailableStatusLabels.isEmpty {
                        Text("No statuses in current scope")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(coreManager.appFilterAvailableStatusLabels, id: \.self) { status in
                            Button {
                                draftStatus = .label(status)
                            } label: {
                                HStack {
                                    Text(status)
                                    Spacer()
                                    if draftStatus != .all,
                                       draftStatus.allows(statusLabel: status) {
                                        Image(systemName: "checkmark")
                                            .foregroundStyle(Color.agentBrand)
                                    }
                                }
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }

                Section("Hashtags") {
                    Button {
                        draftHashtags.removeAll()
                    } label: {
                        HStack {
                            Text("Any Hashtag")
                            Spacer()
                            if draftHashtags.isEmpty {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.agentBrand)
                            }
                        }
                    }
                    .buttonStyle(.borderless)

                    if coreManager.appFilterAvailableHashtags.isEmpty {
                        Text("No hashtags in current scope")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(coreManager.appFilterAvailableHashtags, id: \.self) { hashtag in
                            Button {
                                toggleHashtag(hashtag)
                            } label: {
                                HStack {
                                    Text("#\(hashtag)")
                                    Spacer()
                                    if draftHashtags.contains(hashtag) {
                                        Image(systemName: "checkmark")
                                            .foregroundStyle(Color.agentBrand)
                                    }
                                }
                            }
                            .buttonStyle(.borderless)
                        }
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
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        applyAndDismiss()
                    }
                    .fontWeight(.semibold)
                    .accessibilityIdentifier("global_filter_done")
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 360, idealWidth: 420, maxWidth: 480, minHeight: 500, idealHeight: 620)
        #endif
    }

    private var sortedProjects: [Project] {
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
        draftScheduledEvent = .defaultValue
        draftStatus = .defaultValue
        draftHashtags.removeAll()
    }

    private func toggleHashtag(_ hashtag: String) {
        if draftHashtags.contains(hashtag) {
            draftHashtags.remove(hashtag)
        } else {
            draftHashtags.insert(hashtag)
        }
    }

    private func applyAndDismiss() {
        coreManager.updateAppFilter(
            projectIds: draftProjectIds,
            timeWindow: draftTimeWindow,
            scheduledEvent: draftScheduledEvent,
            status: draftStatus,
            hashtags: draftHashtags
        )
        dismiss()
    }
}
