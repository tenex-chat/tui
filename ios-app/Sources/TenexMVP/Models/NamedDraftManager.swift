import Foundation

/// Actor responsible for thread-safe named draft file persistence.
/// All I/O operations happen off the main thread.
actor NamedDraftStore {
    private let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        return encoder
    }()

    private let decoder = JSONDecoder()

    private var fileURL: URL {
        let documentsDirectory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return documentsDirectory.appendingPathComponent("named_drafts.json")
    }

    func load() -> (drafts: [NamedDraft], loadFailed: Bool) {
        let url = fileURL

        guard FileManager.default.fileExists(atPath: url.path) else {
            return ([], false)
        }

        do {
            let data = try Data(contentsOf: url)
            let loaded = try decoder.decode([NamedDraft].self, from: data)
            return (loaded, false)
        } catch {
            let backupURL = url.deletingPathExtension().appendingPathExtension("corrupted-\(Date().timeIntervalSince1970).json")
            try? FileManager.default.moveItem(at: url, to: backupURL)
            return ([], true)
        }
    }

    func save(_ drafts: [NamedDraft], allowSave: Bool) throws {
        guard allowSave else {
            throw NamedDraftStoreError.saveForbidden(reason: "Cannot save - previous load failed and file was quarantined.")
        }

        let data = try encoder.encode(drafts)
        try data.write(to: fileURL, options: .atomic)
    }

    enum NamedDraftStoreError: Error, LocalizedError {
        case saveForbidden(reason: String)

        var errorDescription: String? {
            switch self {
            case .saveForbidden(let reason):
                return "Save forbidden: \(reason)"
            }
        }
    }
}

/// Manager for named drafts — explicit user-initiated save/browse/delete.
/// Singleton following the same pattern as DraftManager.
@Observable
@MainActor
final class NamedDraftManager {
    static let shared = NamedDraftManager()

    /// All named drafts, sorted by lastModified descending
    private(set) var drafts: [NamedDraft] = []

    private(set) var loadFailed = false
    private(set) var loadCompleted = false
    private(set) var lastSaveError: Error?

    private let store: NamedDraftStore
    private var loadTask: Task<Void, Never>?

    private init() {
        self.store = NamedDraftStore()
        loadTask = Task { @MainActor in
            await loadDrafts()
            loadCompleted = true
        }
    }

    // MARK: - Public API

    @discardableResult
    func save(_ text: String, projectId: String) async -> NamedDraft {
        await ensureLoaded()

        let draft = NamedDraft(text: text, projectId: projectId)
        drafts.insert(draft, at: 0)
        persistNow()
        return draft
    }

    func delete(_ id: String) async {
        await ensureLoaded()
        drafts.removeAll { $0.id == id }
        persistNow()
    }

    func draftsForProject(_ projectId: String) -> [NamedDraft] {
        drafts.filter { $0.projectId == projectId }
    }

    func allDrafts() -> [NamedDraft] {
        drafts
    }

    // MARK: - Private

    private func ensureLoaded() async {
        await loadTask?.value
    }

    private func loadDrafts() async {
        let result = await store.load()
        loadFailed = result.loadFailed

        if loadFailed {
            drafts = []
            return
        }

        drafts = result.drafts.sorted { $0.lastModified > $1.lastModified }
    }

    private func persistNow() {
        guard !loadFailed else { return }

        let snapshot = drafts
        Task { [store] in
            do {
                try await store.save(snapshot, allowSave: true)
                await MainActor.run { [weak self] in
                    self?.lastSaveError = nil
                }
            } catch {
                await MainActor.run { [weak self] in
                    self?.lastSaveError = error
                }
            }
        }
    }
}
