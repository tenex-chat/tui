import SwiftUI

/// Shared observable coordinator between ConversationWorkspaceView toolbar
/// and the MessageComposerView it hosts.
/// The workspace toolbar reads currentAgentPubkey to display the avatar;
/// the composer writes it whenever draft.agentPubkey changes.
/// The toolbar writes requestedAgentPubkey to ask the composer to switch agents.
///
/// Smart reply: the workspace writes smartReplySuggestions after calling the
/// prediction service; the composer renders them as chips. Tapping a chip sets
/// requestedSuggestionText, which the composer observes to fill localText.
@Observable
final class ComposerAgentCoordinator {
    var currentAgentPubkey: String?
    var requestedAgentPubkey: String?

    var smartReplySuggestions: [String] = []
    var requestedSuggestionText: String?
}

// MARK: - Environment key (optional default so callers that don't inject it are unaffected)

private struct ComposerAgentCoordinatorKey: EnvironmentKey {
    static let defaultValue: ComposerAgentCoordinator? = nil
}

extension EnvironmentValues {
    var composerAgentCoordinator: ComposerAgentCoordinator? {
        get { self[ComposerAgentCoordinatorKey.self] }
        set { self[ComposerAgentCoordinatorKey.self] = newValue }
    }
}
