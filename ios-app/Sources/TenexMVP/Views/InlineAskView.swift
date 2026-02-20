import SwiftUI

// MARK: - Inline Ask View

/// Wrapper for inline ask event UI within a conversation.
/// Shows interactive AskAnswerView when unanswered, collapses to summary when answered.
struct InlineAskView: View {
    let askEvent: AskEvent
    let askEventId: String
    let askAuthorPubkey: String
    let conversationId: String
    let projectId: String

    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var isAlreadyAnswered = false
    @State private var didAnswerInSession = false
    @State private var replyContent: String = ""
    @State private var answerSummary: String = ""

    var body: some View {
        Group {
            if isAlreadyAnswered || didAnswerInSession {
                answeredView
            } else {
                AskAnswerView(
                    askEvent: askEvent,
                    askEventId: askEventId,
                    askAuthorPubkey: askAuthorPubkey,
                    conversationId: conversationId,
                    projectId: projectId
                ) {
                    withAnimation(.easeInOut(duration: 0.3)) {
                        answerSummary = generateAnswerSummary()
                        didAnswerInSession = true
                    }
                }
                .environmentObject(coreManager)
            }
        }
        .task {
            let messages = await coreManager.safeCore.getMessages(conversationId: askEventId)
            if let reply = messages.first(where: { $0.replyTo == askEventId }) {
                isAlreadyAnswered = true
                replyContent = reply.content
            }
        }
    }

    // MARK: - Answered View

    private var answeredView: some View {
        HStack(spacing: 10) {
            Image(systemName: "checkmark.circle.fill")
                .font(.title3)
                .foregroundStyle(Color.statusActive)

            VStack(alignment: .leading, spacing: 2) {
                if let title = askEvent.title {
                    Text(title)
                        .font(.subheadline)
                        .fontWeight(.medium)
                } else {
                    Text("Questions Answered")
                        .font(.subheadline)
                        .fontWeight(.medium)
                }

                if isAlreadyAnswered, !replyContent.isEmpty {
                    Text(replyContent)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                } else if !answerSummary.isEmpty {
                    Text(answerSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }

            Spacer()

            if didAnswerInSession {
                Button {
                    withAnimation {
                        didAnswerInSession = false
                    }
                } label: {
                    Image(systemName: "chevron.down")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
            }
        }
        .padding(12)
        .background(Color.presenceOnlineBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.presenceOnline.opacity(0.3), lineWidth: 1)
        )
    }

    // MARK: - Helpers

    private func generateAnswerSummary() -> String {
        // Generate a brief summary of the questions
        let questionCount = askEvent.questions.count
        if questionCount == 1 {
            return "1 question answered"
        } else {
            return "\(questionCount) questions answered"
        }
    }
}

// MARK: - Preview

#Preview {
    VStack(spacing: 20) {
        // Unanswered state
        InlineAskView(
            askEvent: AskEvent(
                title: "Project Setup",
                context: "Please answer the following questions.",
                questions: [
                    .singleSelect(
                        title: "Language",
                        question: "What language?",
                        suggestions: ["Swift", "Rust", "TypeScript"]
                    )
                ]
            ),
            askEventId: "test-id",
            askAuthorPubkey: "test-pubkey",
            conversationId: "test-conv",
            projectId: "test-project"
        )
        .environmentObject(TenexCoreManager())
    }
    .padding()
}
