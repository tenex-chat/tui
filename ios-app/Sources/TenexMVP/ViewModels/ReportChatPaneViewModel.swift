import SwiftUI

/// View model for macOS report chat pane.
/// Loads root threads that a-tag a report and exposes loading/error state for UI.
@MainActor
final class ReportChatPaneViewModel: ObservableObject {
    typealias ThreadLoader = (String) async throws -> [Thread]

    @Published private(set) var threads: [Thread] = []
    @Published private(set) var isLoading = false
    @Published private(set) var errorMessage: String?

    private var refreshTask: Task<Void, Never>?
    private var loadGeneration: UInt64 = 0

    deinit {
        refreshTask?.cancel()
    }

    func load(reportATag: String, using loader: @escaping ThreadLoader) async {
        loadGeneration &+= 1
        let generation = loadGeneration
        isLoading = true
        errorMessage = nil

        do {
            let loadedThreads = try await loader(reportATag)
            guard generation == loadGeneration else { return }
            threads = loadedThreads.sorted { $0.lastActivity > $1.lastActivity }
            isLoading = false
        } catch {
            guard generation == loadGeneration else { return }
            threads = []
            errorMessage = error.localizedDescription
            isLoading = false
        }
    }

    func refreshDebounced(
        reportATag: String,
        delayNanoseconds: UInt64 = 300_000_000,
        using loader: @escaping ThreadLoader
    ) {
        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: delayNanoseconds)
            guard let self, !Task.isCancelled else { return }
            await self.load(reportATag: reportATag, using: loader)
        }
    }
}
