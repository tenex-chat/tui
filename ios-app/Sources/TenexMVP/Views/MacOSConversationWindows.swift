#if os(macOS)
import SwiftUI

// MARK: - Conversation Summary Window

/// Wrapper view that resolves a conversation ID and displays ConversationDetailView in a macOS window.
/// Used as the content for the "conversation-summary" WindowGroup.
struct ConversationSummaryWindow: View {
    let conversationId: String
    @Environment(TenexCoreManager.self) var coreManager

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if let conversation {
                    ConversationAdaptiveDetailView(conversation: conversation)
                        .environment(coreManager)
                } else if isLoading {
                    ProgressView("Loading conversation...")
                } else {
                    ContentUnavailableView(
                        "Conversation Not Found",
                        systemImage: "doc.questionmark",
                        description: Text("Unable to load this conversation.")
                    )
                }
            }
        }
        .task {
            await resolveConversation()
        }
        .onChange(of: coreManager.conversations) { _, _ in
            if let updated = coreManager.conversationById[conversationId] {
                conversation = updated
            }
        }
    }

    private func resolveConversation() async {
        // Try local cache first
        if let cached = coreManager.conversationById[conversationId] {
            conversation = cached
            isLoading = false
            return
        }

        // Fall back to fetching by ID
        let fetched = await coreManager.safeCore.getConversationsByIds(conversationIds: [conversationId])
        conversation = fetched.first
        isLoading = false
    }
}

// MARK: - Full Conversation Window

/// Wrapper view that resolves a conversation ID, loads messages, and displays FullConversationSheet in a macOS window.
/// Used as the content for the "full-conversation" WindowGroup.
struct FullConversationWindow: View {
    let conversationId: String
    @Environment(TenexCoreManager.self) var coreManager

    @State private var conversation: ConversationFullInfo?
    @State private var messages: [Message] = []
    @State private var isLoading = true

    var body: some View {
        Group {
            if let conversation {
                FullConversationSheet(
                    conversation: conversation,
                    messages: messages
                )
                .environment(coreManager)
            } else if isLoading {
                ProgressView("Loading conversation...")
            } else {
                ContentUnavailableView(
                    "Conversation Not Found",
                    systemImage: "doc.questionmark",
                    description: Text("Unable to load this conversation.")
                )
            }
        }
        .task {
            await resolveConversation()
            await coreManager.ensureMessagesLoaded(conversationId: conversationId)
            messages = coreManager.messagesByConversation[conversationId] ?? []
        }
        .onChange(of: coreManager.conversations) { _, _ in
            if let updated = coreManager.conversationById[conversationId] {
                conversation = updated
            }
        }
        .onChange(of: coreManager.messagesByConversation) { _, _ in
            if let updated = coreManager.messagesByConversation[conversationId] {
                messages = updated
            }
        }
    }

    private func resolveConversation() async {
        if let cached = coreManager.conversationById[conversationId] {
            conversation = cached
            isLoading = false
            return
        }

        let fetched = await coreManager.safeCore.getConversationsByIds(conversationIds: [conversationId])
        conversation = fetched.first
        isLoading = false
    }
}
#endif
