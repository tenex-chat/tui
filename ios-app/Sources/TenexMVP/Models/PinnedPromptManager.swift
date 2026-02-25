import Foundation

actor PinnedPromptStore {
    private let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        return encoder
    }()

    private let decoder = JSONDecoder()

    private var fileURL: URL {
        let documentsDirectory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return documentsDirectory.appendingPathComponent("pinned_prompts.json")
    }

    func load() -> (prompts: [PinnedPrompt], loadFailed: Bool) {
        let url = fileURL

        guard FileManager.default.fileExists(atPath: url.path) else {
            return ([], false)
        }

        do {
            let data = try Data(contentsOf: url)
            let loaded = try decoder.decode([PinnedPrompt].self, from: data)
            return (loaded, false)
        } catch {
            let backupURL = url
                .deletingPathExtension()
                .appendingPathExtension("corrupted-\(Date().timeIntervalSince1970).json")
            try? FileManager.default.moveItem(at: url, to: backupURL)
            return ([], true)
        }
    }

    func save(_ prompts: [PinnedPrompt], allowSave: Bool) throws {
        guard allowSave else {
            throw PinnedPromptStoreError.saveForbidden(reason: "Cannot save - previous load failed and file was quarantined.")
        }

        let data = try encoder.encode(prompts)
        try data.write(to: fileURL, options: .atomic)
    }

    enum PinnedPromptStoreError: Error, LocalizedError {
        case saveForbidden(reason: String)

        var errorDescription: String? {
            switch self {
            case .saveForbidden(let reason):
                return "Save forbidden: \(reason)"
            }
        }
    }
}

@Observable
@MainActor
final class PinnedPromptManager {
    static let shared = PinnedPromptManager()

    private(set) var prompts: [PinnedPrompt] = []
    private(set) var loadFailed = false
    private(set) var loadCompleted = false
    private(set) var lastSaveError: Error?

    private let store: PinnedPromptStore
    private var loadTask: Task<Void, Never>?

    private init() {
        self.store = PinnedPromptStore()
        loadTask = Task { @MainActor in
            await loadPrompts()
            loadCompleted = true
        }
    }

    @discardableResult
    func pin(title: String, text: String) async -> PinnedPrompt {
        await ensureLoaded()

        guard let normalized = PinnedPrompt.normalized(title: title, text: text) else {
            let error = PinnedPromptManagerError.invalidInput
            lastSaveError = error
            return PinnedPrompt(title: title, text: text)
        }

        let prompt = PinnedPrompt(title: normalized.title, text: normalized.text)
        prompts.insert(prompt, at: 0)
        prompts.sort(by: PinnedPrompt.sortComparator(_:_:))
        persistNow()
        return prompt
    }

    func delete(_ id: String) async {
        await ensureLoaded()
        prompts.removeAll { $0.id == id }
        persistNow()
    }

    func all() -> [PinnedPrompt] {
        prompts
    }

    func markUsed(_ id: String) async {
        await ensureLoaded()
        guard let index = prompts.firstIndex(where: { $0.id == id }) else { return }
        prompts[index].markUsed()
        prompts.sort(by: PinnedPrompt.sortComparator(_:_:))
        persistNow()
    }

    private func ensureLoaded() async {
        await loadTask?.value
    }

    private func loadPrompts() async {
        let result = await store.load()
        loadFailed = result.loadFailed

        if loadFailed {
            prompts = []
            return
        }

        prompts = result.prompts.sorted(by: PinnedPrompt.sortComparator(_:_:))
    }

    private func persistNow() {
        guard !loadFailed else { return }
        lastSaveError = nil

        let snapshot = prompts
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

    enum PinnedPromptManagerError: Error, LocalizedError {
        case invalidInput

        var errorDescription: String? {
            switch self {
            case .invalidInput:
                return "Pinned prompt title and text must both be non-empty."
            }
        }
    }
}
