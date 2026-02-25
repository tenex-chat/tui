import SwiftUI

struct AppGlobalFilterToolbarButton: View {
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Menu {
            timeMenu
            projectsMenu
            scheduledEventsMenu
            hashtagsMenu
            statusMenu

            Divider()

            Button {
                applyFilter(showArchived: !coreManager.appFilterShowArchived)
            } label: {
                if coreManager.appFilterShowArchived {
                    Label("Show Archived", systemImage: "checkmark")
                } else {
                    Text("Show Archived")
                }
            }
            .accessibilityIdentifier("global_filter_show_archived")

            Divider()

            Button {
                coreManager.resetAppFilterToDefaults()
            } label: {
                Label("Reset to Defaults", systemImage: "arrow.counterclockwise")
            }
            .accessibilityIdentifier("global_filter_reset_defaults")
        } label: {
            filterButtonLabel
        }
        .accessibilityIdentifier("global_filter_button")
        .accessibilityLabel("Global Filter")
        .accessibilityValue(coreManager.appFilterSummaryLabel)
        .help("Filter by time, schedule, projects, status, and hashtags")
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

    private var timeMenu: some View {
        Menu("Time") {
            ForEach(AppTimeWindow.allCases, id: \.self) { window in
                Button {
                    applyFilter(timeWindow: window)
                } label: {
                    selectableLabel(window.label, isSelected: coreManager.appFilterTimeWindow == window)
                }
                .accessibilityIdentifier("global_filter_time_\(window.rawValue)")
            }
        }
        .accessibilityIdentifier("global_filter_menu_time")
    }

    private var scheduledEventsMenu: some View {
        Menu("Scheduled Events") {
            ForEach(ScheduledEventFilter.allCases, id: \.self) { filter in
                Button {
                    applyFilter(scheduledEvent: filter)
                } label: {
                    selectableLabel(filter.label, isSelected: coreManager.appFilterScheduledEvent == filter)
                }
                .accessibilityIdentifier("global_filter_scheduled_\(filter.rawValue)")
            }
        }
        .accessibilityIdentifier("global_filter_menu_scheduled_events")
    }

    private var statusMenu: some View {
        Menu("Status") {
            Button {
                applyFilter(status: .all)
            } label: {
                selectableLabel("All Statuses", isSelected: coreManager.appFilterStatus == .all)
            }
            .accessibilityIdentifier("global_filter_status_all")

            if coreManager.appFilterAvailableStatusLabels.isEmpty {
                Text("No statuses in current scope")
            } else {
                ForEach(coreManager.appFilterAvailableStatusLabels, id: \.self) { status in
                    Button {
                        applyFilter(status: .label(status))
                    } label: {
                        selectableLabel(
                            status,
                            isSelected: coreManager.appFilterStatus != .all
                                && coreManager.appFilterStatus.allows(statusLabel: status)
                        )
                    }
                    .accessibilityIdentifier("global_filter_status_\(status)")
                }
            }
        }
        .accessibilityIdentifier("global_filter_menu_status")
    }

    private var hashtagsMenu: some View {
        Menu("Hashtags") {
            Button {
                applyFilter(hashtags: [])
            } label: {
                selectableLabel("Any Hashtag", isSelected: coreManager.appFilterHashtags.isEmpty)
            }
            .accessibilityIdentifier("global_filter_any_hashtag")

            if coreManager.appFilterAvailableHashtags.isEmpty {
                Text("No hashtags in current scope")
            } else {
                ForEach(coreManager.appFilterAvailableHashtags, id: \.self) { hashtag in
                    Button {
                        toggleHashtag(hashtag)
                    } label: {
                        selectableLabel(
                            "#\(hashtag)",
                            isSelected: coreManager.appFilterHashtags.contains(hashtag)
                        )
                    }
                    .accessibilityIdentifier("global_filter_hashtag_\(hashtag)")
                }
            }
        }
        .accessibilityIdentifier("global_filter_menu_hashtags")
    }

    private var projectsMenu: some View {
        Menu("Projects") {
            Button {
                applyFilter(projectIds: [])
            } label: {
                selectableLabel("All Projects", isSelected: coreManager.appFilterProjectIds.isEmpty)
            }
            .accessibilityIdentifier("global_filter_all_projects")

            if !projectMenuState.booted.isEmpty {
                Section("Booted Projects") {
                    ForEach(projectMenuState.booted, id: \.id) { project in
                        projectButton(for: project)
                    }
                }
            }

            if !projectMenuState.unbooted.isEmpty {
                Menu("Unbooted Projects") {
                    ForEach(projectMenuState.unbooted, id: \.id) { project in
                        projectButton(for: project)
                    }
                }
            }

            if projectMenuState.booted.isEmpty && projectMenuState.unbooted.isEmpty {
                Text("No projects available")
            }
        }
        .accessibilityIdentifier("global_filter_menu_projects")
    }

    @ViewBuilder
    private func selectableLabel(_ title: String, isSelected: Bool) -> some View {
        if isSelected {
            Label(title, systemImage: "checkmark")
        } else {
            Text(title)
        }
    }

    private var projectMenuState: GlobalFilterProjectMenuState {
        let sorted = coreManager.projects.sorted { lhs, rhs in
            let lhsOnline = coreManager.projectOnlineStatus[lhs.id] ?? false
            let rhsOnline = coreManager.projectOnlineStatus[rhs.id] ?? false
            if lhsOnline != rhsOnline { return lhsOnline }
            return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
        }

        let booted = sorted.filter { coreManager.projectOnlineStatus[$0.id] ?? false }
        let unbooted = sorted.filter { !(coreManager.projectOnlineStatus[$0.id] ?? false) }
        return GlobalFilterProjectMenuState(booted: booted, unbooted: unbooted)
    }

    private func projectButton(for project: Project) -> some View {
        Button {
            toggleProject(project.id)
        } label: {
            selectableLabel(project.title, isSelected: coreManager.appFilterProjectIds.contains(project.id))
        }
        .accessibilityIdentifier("global_filter_project_\(project.id)")
    }

    private func toggleProject(_ projectId: String) {
        var updatedProjectIds = coreManager.appFilterProjectIds
        if updatedProjectIds.contains(projectId) {
            updatedProjectIds.remove(projectId)
        } else {
            updatedProjectIds.insert(projectId)
        }
        applyFilter(projectIds: updatedProjectIds)
    }

    private func toggleHashtag(_ hashtag: String) {
        var updatedHashtags = coreManager.appFilterHashtags
        if updatedHashtags.contains(hashtag) {
            updatedHashtags.remove(hashtag)
        } else {
            updatedHashtags.insert(hashtag)
        }
        applyFilter(hashtags: updatedHashtags)
    }

    private func applyFilter(
        projectIds: Set<String>? = nil,
        timeWindow: AppTimeWindow? = nil,
        scheduledEvent: ScheduledEventFilter? = nil,
        status: ConversationStatusFilter? = nil,
        hashtags: Set<String>? = nil,
        showArchived: Bool? = nil
    ) {
        coreManager.updateAppFilter(
            projectIds: projectIds ?? coreManager.appFilterProjectIds,
            timeWindow: timeWindow ?? coreManager.appFilterTimeWindow,
            scheduledEvent: scheduledEvent ?? coreManager.appFilterScheduledEvent,
            status: status ?? coreManager.appFilterStatus,
            hashtags: hashtags ?? coreManager.appFilterHashtags,
            showArchived: showArchived ?? coreManager.appFilterShowArchived
        )
    }
}

private struct GlobalFilterProjectMenuState {
    var booted: [Project] = []
    var unbooted: [Project] = []
}
