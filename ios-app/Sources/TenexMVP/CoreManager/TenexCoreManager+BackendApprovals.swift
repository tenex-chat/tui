import Foundation

extension TenexCoreManager {
    @MainActor
    func reloadPendingBackendApprovalPrompts() async {
        do {
            let snapshot = try await safeCore.getBackendTrustSnapshot()
            applyPendingBackendApprovalSnapshot(snapshot.pending)
        } catch {
            profiler.logEvent(
                "reloadPendingBackendApprovalPrompts failed error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    @MainActor
    func applyPendingBackendApprovalSnapshot(_ pending: [PendingBackendInfo]) {
        let requests = Self.backendApprovalRequests(from: pending, projects: projects)
        pendingBackendApprovalRequests = requests.filter { request in
            snoozedBackendApprovalProjectTags[request.backendPubkey] != request.projectATags
        }
        signalDiagnosticsUpdate()
    }

    @MainActor
    func deferBackendApprovalPrompt(backendPubkey: String) {
        if let request = pendingBackendApprovalRequests.first(where: { $0.backendPubkey == backendPubkey }) {
            snoozedBackendApprovalProjectTags[backendPubkey] = request.projectATags
        }
        pendingBackendApprovalRequests.removeAll { $0.backendPubkey == backendPubkey }
    }

    @MainActor
    func approvePendingBackend(backendPubkey: String) async {
        do {
            try await safeCore.approveBackend(pubkey: backendPubkey)
            snoozedBackendApprovalProjectTags.removeValue(forKey: backendPubkey)
            pendingBackendApprovalRequests.removeAll { $0.backendPubkey == backendPubkey }
            await republishCachedApnsRegistrationNow()
            await fetchData()
        } catch {
            profiler.logEvent(
                "approvePendingBackend failed backend=\(backendPubkey) error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    @MainActor
    func blockPendingBackend(backendPubkey: String) async {
        do {
            try await safeCore.blockBackend(pubkey: backendPubkey)
            snoozedBackendApprovalProjectTags.removeValue(forKey: backendPubkey)
            pendingBackendApprovalRequests.removeAll { $0.backendPubkey == backendPubkey }
            await fetchData()
        } catch {
            profiler.logEvent(
                "blockPendingBackend failed backend=\(backendPubkey) error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }
}
