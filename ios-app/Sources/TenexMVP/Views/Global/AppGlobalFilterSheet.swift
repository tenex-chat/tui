import SwiftUI

struct AppGlobalFilterToolbarButton: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var isPresented = false

    var body: some View {
        #if os(macOS)
        Menu {
            timeSubmenu
            scheduledEventsSubmenu
            projectsSubmenu
            statusSubmenu
            hashtagsSubmenu
            if !coreManager.isAppFilterDefault {
                Divider()
                Button {
                    coreManager.resetAppFilterToDefaults()
                } label: {
                    Label("Reset to Defaults", systemImage: "arrow.counterclockwise")
                }
            }
        } label: {
            Image(systemName: filterIcon)
        }
        .accessibilityIdentifier("global_filter_button")
        .accessibilityLabel("Global Filter")
        .accessibilityValue(coreManager.appFilterSummaryLabel)
        .help("Filter by time, schedule, projects, status, and hashtags")
        #else
        Button {
            isPresented = true
        } label: {
            Label(coreManager.appFilterSummaryLabel, systemImage: filterIcon)
                .lineLimit(1)
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
        #endif
    }

    private var filterIcon: String {
        coreManager.isAppFilterDefault
            ? "line.3.horizontal.decrease.circle"
            : "line.3.horizontal.decrease.circle.fill"
    }

    // MARK: - macOS Menu Submenus

    #if os(macOS)
    @ViewBuilder
    private var timeSubmenu: some View {
        Picker(selection: Binding(
            get: { coreManager.appFilterTimeWindow },
            set: { newValue in
                coreManager.updateAppFilter(
                    projectIds: coreManager.appFilterProjectIds,
                    timeWindow: newValue
                )
            }
        )) {
            ForEach(AppTimeWindow.allCases, id: \.self) { window in
                Text(window.label).tag(window)
            }
        } label: {
            Label("Time", systemImage: "clock")
        }
    }

    @ViewBuilder
    private var scheduledEventsSubmenu: some View {
        Picker(selection: Binding(
            get: { coreManager.appFilterScheduledEvent },
            set: { newValue in
                coreManager.updateAppFilter(
                    projectIds: coreManager.appFilterProjectIds,
                    timeWindow: coreManager.appFilterTimeWindow,
                    scheduledEvent: newValue
                )
            }
        )) {
            ForEach(ScheduledEventFilter.allCases, id: \.self) { filter in
                Text(filter.label).tag(filter)
            }
        } label: {
            Label("Scheduled Events", systemImage: "calendar.badge.clock")
        }
    }

    @ViewBuilder
    private var projectsSubmenu: some View {
        Menu {
            Button {
                coreManager.updateAppFilter(projectIds: [], timeWindow: coreManager.appFilterTimeWindow)
            } label: {
                if coreManager.appFilterProjectIds.isEmpty {
                    Label("All Projects", systemImage: "checkmark")
                } else {
                    Text("All Projects")
                }
            }

            Divider()

            ForEach(bootedProjects, id: \.id) { project in
                Toggle(isOn: Binding(
                    get: { coreManager.appFilterProjectIds.contains(project.id) },
                    set: { _ in toggleProject(project.id) }
                )) {
                    Label(project.title, systemImage: "bolt.fill")
                }
            }

            if !unbootedProjects.isEmpty {
                Divider()

                Menu("Unbooted Projects") {
                    ForEach(unbootedProjects, id: \.id) { project in
                        Toggle(isOn: Binding(
                            get: { coreManager.appFilterProjectIds.contains(project.id) },
                            set: { _ in toggleProject(project.id) }
                        )) {
                            Text(project.title)
                        }
                    }
                }
            }
        } label: {
            Label("Projects", systemImage: "folder")
        }
    }

    @ViewBuilder
    private var statusSubmenu: some View {
        Menu {
            Button {
                updateStatus(.all)
            } label: {
                if coreManager.appFilterStatus == .all {
                    Label("All Statuses", systemImage: "checkmark")
                } else {
                    Text("All Statuses")
                }
            }

            if coreManager.appFilterAvailableStatusLabels.isEmpty {
                Divider()
                Text("No statuses in current scope")
                    .foregroundStyle(.secondary)
            } else {
                Divider()
                ForEach(coreManager.appFilterAvailableStatusLabels, id: \.self) { status in
                    Button {
                        updateStatus(.label(status))
                    } label: {
                        if coreManager.appFilterStatus != .all,
                           coreManager.appFilterStatus.allows(statusLabel: status) {
                            Label(status, systemImage: "checkmark")
                        } else {
                            Text(status)
                        }
                    }
                }
            }
        } label: {
            Label("Status", systemImage: "flag")
        }
    }

    @ViewBuilder
    private var hashtagsSubmenu: some View {
        Menu {
            Button {
                updateHashtags(Set<String>())
            } label: {
                if coreManager.appFilterHashtags.isEmpty {
                    Label("Any Hashtag", systemImage: "checkmark")
                } else {
                    Text("Any Hashtag")
                }
            }

            if coreManager.appFilterAvailableHashtags.isEmpty {
                Divider()
                Text("No hashtags in current scope")
                    .foregroundStyle(.secondary)
            } else {
                Divider()
                ForEach(coreManager.appFilterAvailableHashtags, id: \.self) { hashtag in
                    Toggle(isOn: Binding(
                        get: { coreManager.appFilterHashtags.contains(hashtag) },
                        set: { _ in toggleHashtag(hashtag) }
                    )) {
                        Text("#\(hashtag)")
                    }
                }
            }
        } label: {
            Label("Hashtag", systemImage: "number")
        }
    }

    private var bootedProjects: [Project] {
        coreManager.projects
            .filter { coreManager.projectOnlineStatus[$0.id] == true }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    private var unbootedProjects: [Project] {
        coreManager.projects
            .filter { coreManager.projectOnlineStatus[$0.id] != true }
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    private func toggleProject(_ projectId: String) {
        var ids = coreManager.appFilterProjectIds
        if ids.contains(projectId) {
            ids.remove(projectId)
        } else {
            ids.insert(projectId)
        }
        coreManager.updateAppFilter(projectIds: ids, timeWindow: coreManager.appFilterTimeWindow)
    }

    private func updateStatus(_ status: ConversationStatusFilter) {
        coreManager.updateAppFilter(
            projectIds: coreManager.appFilterProjectIds,
            timeWindow: coreManager.appFilterTimeWindow,
            status: status
        )
    }

    private func updateHashtags(_ hashtags: Set<String>) {
        coreManager.updateAppFilter(
            projectIds: coreManager.appFilterProjectIds,
            timeWindow: coreManager.appFilterTimeWindow,
            hashtags: hashtags
        )
    }

    private func toggleHashtag(_ hashtag: String) {
        var hashtags = coreManager.appFilterHashtags
        if hashtags.contains(hashtag) {
            hashtags.remove(hashtag)
        } else {
            hashtags.insert(hashtag)
        }
        updateHashtags(hashtags)
    }
    #endif
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
