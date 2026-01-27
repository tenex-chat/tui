import Foundation
import Combine

/// Actor responsible for thread-safe draft file persistence.
/// All I/O operations happen off the main thread.
actor DraftStore {
    private let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        return encoder
    }()

    private let decoder = JSONDecoder()

    private var draftsFileURL: URL {
        let documentsDirectory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return documentsDirectory.appendingPathComponent("message_drafts.json")
    }

    /// Load drafts from disk
    func loadDrafts() -> [String: Draft] {
        let fileURL = draftsFileURL

        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            print("[DraftStore] No existing drafts file found")
            return [:]
        }

        do {
            let data = try Data(contentsOf: fileURL)
            let loadedDrafts = try decoder.decode([String: Draft].self, from: data)
            print("[DraftStore] Loaded \(loadedDrafts.count) drafts")
            return loadedDrafts
        } catch {
            print("[DraftStore] Failed to load drafts: \(error)")
            return [:]
        }
    }

    /// Save drafts to disk
    func saveDrafts(_ drafts: [String: Draft]) throws {
        let data = try encoder.encode(drafts)
        try data.write(to: draftsFileURL, options: .atomic)
        print("[DraftStore] Saved \(drafts.count) drafts")
    }
}

/// Manager for handling draft persistence with debounced auto-save.
/// Thread-safe with actor-isolated persistence operations.
@Observable
@MainActor
final class DraftManager {
    // MARK: - Constants

    private static let saveDelay: TimeInterval = 0.5 // 500ms debounce

    // MARK: - Properties

    /// Currently loaded drafts, keyed by their storage key
    private(set) var drafts: [String: Draft] = [:]

    /// Whether a save operation is pending
    private(set) var hasPendingSave = false

    /// Last save error, if any
    private(set) var lastSaveError: Error?

    /// Whether drafts failed to load (distinguishes from empty)
    private(set) var loadFailed = false

    // MARK: - Private Properties

    private var saveTask: Task<Void, Never>?
    private let store = DraftStore()

    // MARK: - Initialization

    init() {
        Task { @MainActor in
            await loadDrafts()
        }
    }

    // MARK: - Public API

    /// Get a draft for a conversation/project combination
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    /// - Returns: The draft if it exists, nil otherwise
    func getDraft(conversationId: String?, projectId: String) -> Draft? {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        return drafts[key]
    }

    /// Get or create a draft for a conversation/project combination
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    /// - Returns: The existing draft or a newly created one
    func getOrCreateDraft(conversationId: String?, projectId: String) -> Draft {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if let existing = drafts[key] {
            return existing
        }

        let newDraft: Draft
        if let conversationId = conversationId {
            newDraft = Draft(conversationId: conversationId, projectId: projectId)
        } else {
            newDraft = Draft(projectId: projectId)
        }

        drafts[key] = newDraft
        scheduleSave()
        return newDraft
    }

    /// Update a draft's content with debounced auto-save
    /// - Parameters:
    ///   - content: The new content
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateContent(_ content: String, conversationId: String?, projectId: String) {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.updateContent(content)
            drafts[key] = draft
        } else {
            let newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, content: content)
            } else {
                newDraft = Draft(projectId: projectId, content: content)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's title with debounced auto-save
    /// - Parameters:
    ///   - title: The new title
    ///   - projectId: The project ID
    func updateTitle(_ title: String, projectId: String) {
        let key = Draft.storageKey(for: nil, projectId: projectId)

        if var draft = drafts[key] {
            draft.updateTitle(title)
            drafts[key] = draft
        } else {
            let newDraft = Draft(projectId: projectId, title: title)
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's agent with debounced auto-save (single-select)
    /// - Parameters:
    ///   - agentPubkey: The selected agent pubkey (nil to clear)
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateAgent(_ agentPubkey: String?, conversationId: String?, projectId: String) {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.setAgent(agentPubkey)
            drafts[key] = draft
        } else {
            let newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, agentPubkey: agentPubkey)
            } else {
                newDraft = Draft(projectId: projectId, agentPubkey: agentPubkey)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Delete a draft
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func deleteDraft(conversationId: String?, projectId: String) {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        drafts.removeValue(forKey: key)
        scheduleSave()
    }

    /// Clear a draft's content but keep it in memory
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func clearDraft(conversationId: String?, projectId: String) {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.clear()
            drafts[key] = draft
            scheduleSave()
        }
    }

    /// Get all drafts for a project
    /// - Parameter projectId: The project ID
    /// - Returns: Array of drafts belonging to this project
    func getDraftsForProject(_ projectId: String) -> [Draft] {
        drafts.values.filter { $0.projectId == projectId }
    }

    /// Delete all drafts for a project
    /// - Parameter projectId: The project ID
    func deleteProjectDrafts(_ projectId: String) {
        drafts = drafts.filter { $0.value.projectId != projectId }
        scheduleSave()
    }

    /// Force save immediately, cancelling any pending debounced save
    func saveNow() {
        saveTask?.cancel()
        saveTask = nil
        hasPendingSave = false
        performSave()
    }

    /// Clean up old orphaned drafts (drafts older than specified age with no content)
    /// - Parameter maxAge: Maximum age of empty drafts to keep
    func cleanupOrphanedDrafts(maxAge: TimeInterval = 86400) { // 24 hours default
        let cutoffDate = Date().addingTimeInterval(-maxAge)

        drafts = drafts.filter { _, draft in
            // Keep drafts with content
            if draft.hasContent {
                return true
            }
            // Keep recent empty drafts
            return draft.lastEdited > cutoffDate
        }

        scheduleSave()
    }

    // MARK: - Private Methods

    private func loadDrafts() async {
        let loadedDrafts = await store.loadDrafts()
        drafts = loadedDrafts
        loadFailed = loadedDrafts.isEmpty && FileManager.default.fileExists(
            atPath: FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("message_drafts.json").path
        )
    }

    private func scheduleSave() {
        // Cancel any existing save task
        saveTask?.cancel()
        hasPendingSave = true

        // Capture drafts snapshot for the save
        let draftsSnapshot = drafts

        // Schedule new save after debounce delay
        saveTask = Task { [weak self, store] in
            do {
                try await Task.sleep(nanoseconds: UInt64(Self.saveDelay * 1_000_000_000))

                // Check if task was cancelled
                try Task.checkCancellation()

                // Perform save on the actor (off main thread)
                try await store.saveDrafts(draftsSnapshot)

                await MainActor.run {
                    self?.hasPendingSave = false
                    self?.lastSaveError = nil
                }
            } catch is CancellationError {
                // Task was cancelled, do nothing
            } catch {
                await MainActor.run {
                    self?.hasPendingSave = false
                    self?.lastSaveError = error
                    print("[DraftManager] Save failed: \(error)")
                }
            }
        }
    }

    private func performSave() {
        let draftsSnapshot = drafts

        Task { [weak self, store] in
            do {
                try await store.saveDrafts(draftsSnapshot)
                await MainActor.run {
                    self?.lastSaveError = nil
                }
            } catch {
                await MainActor.run {
                    self?.lastSaveError = error
                    print("[DraftManager] Save failed: \(error)")
                }
            }
        }
    }
}
