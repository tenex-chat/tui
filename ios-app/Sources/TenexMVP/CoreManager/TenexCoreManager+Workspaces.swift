import Foundation

extension TenexCoreManager {
    var sortedWorkspaces: [WorkspaceInfo] {
        workspaces.sorted { lhs, rhs in
            if lhs.pinned != rhs.pinned { return lhs.pinned }
            return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
        }
    }

    var activeWorkspace: WorkspaceInfo? {
        guard let activeWorkspaceId else { return nil }
        return workspaces.first(where: { $0.id == activeWorkspaceId })
    }

    var hasManualProjectScope: Bool {
        activeWorkspace == nil && !appFilterProjectIds.isEmpty
    }

    var currentProjectScopeIds: Set<String> {
        appFilterProjectIds
    }

    var currentProjectScopeName: String {
        if let activeWorkspace {
            return activeWorkspace.name
        }
        return appFilterProjectIds.isEmpty ? "All Projects" : "Unsaved Scope"
    }

    var projectsInCurrentScope: [Project] {
        projects.filter { includesProjectInCurrentScope($0.id) }
    }

    func includesProjectInCurrentScope(_ projectId: String) -> Bool {
        appFilterProjectIds.isEmpty || appFilterProjectIds.contains(projectId)
    }

    func workspaceProjectIds(for workspace: WorkspaceInfo) -> Set<String> {
        Set(workspace.projectATags.map(Self.projectId(fromATag:)).filter { !$0.isEmpty })
    }

    func projectATag(for project: Project) -> String {
        "31933:\(project.pubkey):\(project.id)"
    }

    func projectATags(for projectIds: Set<String>) -> [String] {
        projects
            .filter { projectIds.contains($0.id) }
            .map(projectATag)
            .sorted()
    }

    func workspaceProjectCount(_ workspace: WorkspaceInfo) -> Int {
        workspaceProjectIds(for: workspace).count
    }

    func workspaceOnlineProjectCount(_ workspace: WorkspaceInfo) -> Int {
        workspaceProjectIds(for: workspace).reduce(into: 0) { count, projectId in
            if projectOnlineStatus[projectId] ?? false {
                count += 1
            }
        }
    }

    func refreshWorkspacesFromCore() async {
        do {
            let fetchedWorkspaces = try await core.getWorkspaces()
            let fetchedActiveId = try await core.getActiveWorkspaceId()
            workspaces = fetchedWorkspaces
            if let fetchedActiveId,
               fetchedWorkspaces.contains(where: { $0.id == fetchedActiveId }) {
                activeWorkspaceId = fetchedActiveId
            } else {
                activeWorkspaceId = nil
            }
        } catch {
            profiler.logEvent(
                "refreshWorkspacesFromCore failed error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    @MainActor
    func syncActiveWorkspaceFilterFromState() {
        guard let activeWorkspace else { return }
        let projectIds = workspaceProjectIds(for: activeWorkspace)
        guard projectIds != appFilterProjectIds else { return }
        appFilterProjectIds = projectIds
        persistAppFilter()
    }

    @MainActor
    func applyWorkspace(_ workspace: WorkspaceInfo?) async {
        do {
            try await core.setActiveWorkspace(id: workspace?.id)
            activeWorkspaceId = workspace?.id
            let projectIds = workspace.map { workspaceProjectIds(for: $0) } ?? []
            updateAppFilter(
                projectIds: projectIds,
                timeWindow: appFilterTimeWindow,
                clearActiveWorkspaceOnProjectChange: false
            )
        } catch {
            profiler.logEvent(
                "applyWorkspace failed id=\(workspace?.id ?? "all") error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    @MainActor
    func clearActiveWorkspaceForManualProjectChange() {
        guard activeWorkspaceId != nil else { return }
        activeWorkspaceId = nil
        Task {
            do {
                try await core.setActiveWorkspace(id: nil)
            } catch {
                profiler.logEvent(
                    "clearActiveWorkspaceForManualProjectChange failed error=\(error.localizedDescription)",
                    category: .general,
                    level: .error
                )
            }
        }
    }

    @MainActor
    func createWorkspace(name: String, projectIds: Set<String>, activate: Bool) async throws -> WorkspaceInfo {
        let workspace = try await core.addWorkspace(
            name: name,
            projectATags: projectATags(for: projectIds)
        )
        await refreshWorkspacesFromCore()
        if activate {
            await applyWorkspace(workspace)
        }
        return workspace
    }

    @MainActor
    func updateWorkspace(_ workspace: WorkspaceInfo, name: String, projectIds: Set<String>) async throws {
        try await core.updateWorkspace(
            id: workspace.id,
            name: name,
            projectATags: projectATags(for: projectIds)
        )
        await refreshWorkspacesFromCore()
        if activeWorkspaceId == workspace.id,
           let updated = workspaces.first(where: { $0.id == workspace.id }) {
            await applyWorkspace(updated)
        }
    }

    @MainActor
    func deleteWorkspace(_ workspace: WorkspaceInfo) async throws {
        let wasActive = activeWorkspaceId == workspace.id
        try await core.deleteWorkspace(id: workspace.id)
        await refreshWorkspacesFromCore()
        if wasActive {
            await applyWorkspace(nil)
        }
    }

    @MainActor
    func toggleWorkspacePinned(_ workspace: WorkspaceInfo) async throws {
        _ = try await core.toggleWorkspacePinned(id: workspace.id)
        await refreshWorkspacesFromCore()
    }
}
