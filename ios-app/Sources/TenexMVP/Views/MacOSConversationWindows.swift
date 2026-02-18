#if os(macOS)
import SwiftUI

// MARK: - Conversation Summary Window

/// Wrapper view that resolves a conversation ID and displays ConversationDetailView in a macOS window.
/// Used as the content for the "conversation-summary" WindowGroup.
struct ConversationSummaryWindow: View {
    let conversationId: String
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if let conversation {
                    ConversationAdaptiveDetailView(conversation: conversation)
                        .environmentObject(coreManager)
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
        .onReceive(coreManager.$conversations) { conversations in
            if let updated = conversations.first(where: { $0.id == conversationId }) {
                conversation = updated
            }
        }
    }

    private func resolveConversation() async {
        // Try local cache first
        if let cached = coreManager.conversations.first(where: { $0.id == conversationId }) {
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
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var conversation: ConversationFullInfo?
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = true

    var body: some View {
        Group {
            if let conversation {
                FullConversationSheet(
                    conversation: conversation,
                    messages: messages
                )
                .environmentObject(coreManager)
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
        .onReceive(coreManager.$conversations) { conversations in
            if let updated = conversations.first(where: { $0.id == conversationId }) {
                conversation = updated
            }
        }
        .onReceive(coreManager.$messagesByConversation) { cache in
            if let updated = cache[conversationId] {
                messages = updated
            }
        }
    }

    private func resolveConversation() async {
        if let cached = coreManager.conversations.first(where: { $0.id == conversationId }) {
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
