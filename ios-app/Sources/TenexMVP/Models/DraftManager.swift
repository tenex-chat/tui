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
    /// - Returns: Tuple of (drafts, loadFailed) where loadFailed indicates if file exists but couldn't be read
    func loadDrafts() -> (drafts: [String: Draft], loadFailed: Bool) {
        let fileURL = draftsFileURL

        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            return ([:], false)
        }

        do {
            let data = try Data(contentsOf: fileURL)
            let loadedDrafts = try decoder.decode([String: Draft].self, from: data)
            return (loadedDrafts, false)
        } catch {

            // CRITICAL DATA SAFETY: Quarantine corrupted file to prevent data loss
            // Move it to a backup location so user can potentially recover it
            let backupURL = fileURL.deletingPathExtension().appendingPathExtension("corrupted-\(Date().timeIntervalSince1970).json")
            do {
                try FileManager.default.moveItem(at: fileURL, to: backupURL)
            } catch {
            }

            return ([:], true)
        }
    }

    /// Save drafts to disk
    /// - Parameter drafts: The drafts to save
    /// - Parameter allowSave: Whether saving is allowed (false if load failed and file is quarantined)
    /// - Throws: Error if save fails or if saving is not allowed
    func saveDrafts(_ drafts: [String: Draft], allowSave: Bool) throws {
        guard allowSave else {
            throw DraftStoreError.saveForbidden(reason: "Cannot save - previous load failed and file was quarantined. Fix corruption first.")
        }

        let data = try encoder.encode(drafts)
        try data.write(to: draftsFileURL, options: .atomic)
    }

    enum DraftStoreError: Error, LocalizedError {
        case saveForbidden(reason: String)

        var errorDescription: String? {
            switch self {
            case .saveForbidden(let reason):
                return "Save forbidden: \(reason)"
            }
        }
    }
}

/// Manager for handling draft persistence with debounced auto-save.
/// Thread-safe with actor-isolated persistence operations.
/// Singleton to prevent multiple instances from overwriting each other's data.
@Observable
@MainActor
final class DraftManager {
    // MARK: - Singleton

    static let shared = DraftManager()

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

    /// Whether initial draft loading has completed
    private(set) var loadCompleted = false

    // MARK: - Private Properties

    private var saveTask: Task<Void, Never>?
    private let store = DraftStore()
    private var loadTask: Task<Void, Never>?

    // MARK: - Initialization

    private init() {
        loadTask = Task { @MainActor in
            await loadDrafts()
            loadCompleted = true
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
    /// - Note: Waits for initial draft loading to complete before returning
    func getOrCreateDraft(conversationId: String?, projectId: String) async -> Draft {
        // Wait for initial load to complete to avoid race conditions
        await loadTask?.value

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
    func updateContent(_ content: String, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

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
    func updateTitle(_ title: String, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

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
    func updateAgent(_ agentPubkey: String?, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

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

    /// Update a draft's image attachments with debounced auto-save
    /// - Parameters:
    ///   - imageAttachments: The image attachments
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateImageAttachments(_ imageAttachments: [ImageAttachment], conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.imageAttachments = imageAttachments
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId)
            } else {
                newDraft = Draft(projectId: projectId)
            }
            newDraft.imageAttachments = imageAttachments
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's text attachments with debounced auto-save
    /// - Parameters:
    ///   - textAttachments: The text attachments
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateTextAttachments(_ textAttachments: [TextAttachment], conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.setTextAttachments(textAttachments)
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId)
            } else {
                newDraft = Draft(projectId: projectId)
            }
            newDraft.setTextAttachments(textAttachments)
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Delete a draft
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func deleteDraft(conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        drafts.removeValue(forKey: key)
        scheduleSave()
    }

    /// Clear a draft's content but keep it in memory
    /// - Parameters:
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func clearDraft(conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

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
    func deleteProjectDrafts(_ projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        drafts = drafts.filter { $0.value.projectId != projectId }
        scheduleSave()
    }

    /// Update a draft's reference conversation ID with debounced auto-save
    /// - Parameters:
    ///   - referenceConversationId: The conversation ID to reference (nil to clear)
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateReferenceConversation(_ referenceConversationId: String?, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.setReferenceConversation(referenceConversationId)
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, referenceConversationId: referenceConversationId)
            } else {
                newDraft = Draft(projectId: projectId, referenceConversationId: referenceConversationId)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's reference report a-tag with debounced auto-save
    /// - Parameters:
    ///   - referenceReportATag: The report a-tag to reference (format: "30023:pubkey:slug", nil to clear)
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateReferenceReportATag(_ referenceReportATag: String?, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.setReferenceReportATag(referenceReportATag)
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, referenceReportATag: referenceReportATag)
            } else {
                newDraft = Draft(projectId: projectId, referenceReportATag: referenceReportATag)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's selected skill IDs with debounced auto-save
    /// - Parameters:
    ///   - skillIds: The set of selected skill IDs
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateSkillIds(_ skillIds: Set<String>, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.selectedSkillIds = skillIds
            draft.lastEdited = Date()
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, selectedSkillIds: skillIds)
            } else {
                newDraft = Draft(projectId: projectId, selectedSkillIds: skillIds)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Update a draft's selected nudge IDs with debounced auto-save
    /// - Parameters:
    ///   - nudgeIds: The set of selected nudge IDs
    ///   - conversationId: The conversation ID (nil for new thread)
    ///   - projectId: The project ID
    func updateNudgeIds(_ nudgeIds: Set<String>, conversationId: String?, projectId: String) async {
        // Wait for initial load to complete to avoid race conditions
        await ensureLoaded()

        let key = Draft.storageKey(for: conversationId, projectId: projectId)

        if var draft = drafts[key] {
            draft.selectedNudgeIds = nudgeIds
            draft.lastEdited = Date()
            drafts[key] = draft
        } else {
            var newDraft: Draft
            if let conversationId = conversationId {
                newDraft = Draft(conversationId: conversationId, projectId: projectId, selectedNudgeIds: nudgeIds)
            } else {
                newDraft = Draft(projectId: projectId, selectedNudgeIds: nudgeIds)
            }
            drafts[key] = newDraft
        }

        scheduleSave()
    }

    /// Force save immediately, cancelling any pending debounced save
    /// - Note: This is truly synchronous - blocks until save completes
    /// - Throws: Error if save fails (including if load failed and saves are blocked)
    func saveNow() async throws {
        // BLOCKER FIX: Wait for initial load to complete before saving
        // This prevents saveNow() from persisting empty snapshot before disk drafts are loaded
        await ensureLoaded()

        saveTask?.cancel()
        saveTask = nil
        hasPendingSave = false
        try await performSaveSyncWithThrow()
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

    /// Ensure initial load has completed before proceeding with mutations
    /// - Note: Prevents race conditions where mutations occur before drafts are loaded
    private func ensureLoaded() async {
        await loadTask?.value
    }

    private func loadDrafts() async {
        let loadResult = await store.loadDrafts()
        let loadedDrafts = loadResult.drafts
        loadFailed = loadResult.loadFailed

        // CRITICAL DATA SAFETY: If load failed, prevent any saves to avoid overwriting quarantined file
        if loadFailed {
            drafts = [:] // Start with empty drafts - do NOT overwrite quarantined file
            return
        }

        // Migration: Re-key drafts to match new storage key format
        // Old drafts might have been stored with different keys or missing projectId
        var migratedDrafts: [String: Draft] = [:]
        var legacyDrafts: [String: Draft] = [:]

        for (oldKey, draft) in loadedDrafts {
            // Preserve legacy drafts with empty projectId instead of deleting them
            // Store them under a special "legacy-{originalKey}" key for recovery
            if draft.projectId.isEmpty {
                let legacyKey = "legacy-\(oldKey)"
                legacyDrafts[legacyKey] = draft
                continue
            }

            // Calculate the correct storage key based on current format
            let correctKey = draft.storageKey

            // If the key changed, migrate it
            if oldKey != correctKey {
                migratedDrafts[correctKey] = draft
            } else {
                migratedDrafts[correctKey] = draft
            }
        }

        // Merge legacy drafts into migrated drafts so they're preserved on disk
        drafts = migratedDrafts.merging(legacyDrafts) { current, _ in current }

        // If we migrated any keys or found legacy drafts, save immediately to persist
        if drafts.keys.sorted() != loadedDrafts.keys.sorted() {
            if !legacyDrafts.isEmpty {
            }
            scheduleSave()
        }
    }

    private func scheduleSave() {
        // CRITICAL DATA SAFETY: Block all saves if load failed
        if loadFailed {
            return
        }

        // Cancel any existing save task
        saveTask?.cancel()
        hasPendingSave = true

        // Capture drafts snapshot for the save
        let draftsSnapshot = drafts
        let allowSave = !loadFailed

        // Schedule new save after debounce delay
        saveTask = Task { [weak self, store] in
            do {
                try await Task.sleep(nanoseconds: UInt64(Self.saveDelay * 1_000_000_000))

                // Check if task was cancelled
                try Task.checkCancellation()

                // Perform save on the actor (off main thread)
                try await store.saveDrafts(draftsSnapshot, allowSave: allowSave)

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
                }
            }
        }
    }

    private func performSave() {
        let draftsSnapshot = drafts
        let allowSave = !loadFailed

        Task { [weak self, store] in
            do {
                try await store.saveDrafts(draftsSnapshot, allowSave: allowSave)
                await MainActor.run {
                    self?.lastSaveError = nil
                }
            } catch {
                await MainActor.run {
                    self?.lastSaveError = error
                }
            }
        }
    }

    /// Perform synchronous save (blocks until complete)
    private func performSaveSync() async {
        // CRITICAL DATA SAFETY: Block all saves if load failed
        if loadFailed {
            lastSaveError = DraftStore.DraftStoreError.saveForbidden(reason: "Load failed - file quarantined")
            return
        }

        let draftsSnapshot = drafts
        let allowSave = !loadFailed

        do {
            try await store.saveDrafts(draftsSnapshot, allowSave: allowSave)
            lastSaveError = nil
        } catch {
            lastSaveError = error
        }
    }

    /// Perform synchronous save (blocks until complete) - throws on error
    /// - Throws: Error if save fails (including if load failed and saves are blocked)
    private func performSaveSyncWithThrow() async throws {
        // CRITICAL DATA SAFETY: Block all saves if load failed
        if loadFailed {
            let error = DraftStore.DraftStoreError.saveForbidden(reason: "Load failed - file quarantined")
            lastSaveError = error
            throw error
        }

        let draftsSnapshot = drafts
        let allowSave = !loadFailed

        do {
            try await store.saveDrafts(draftsSnapshot, allowSave: allowSave)
            lastSaveError = nil
        } catch {
            lastSaveError = error
            throw error
        }
    }
}
