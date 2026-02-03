import Foundation

struct LiveFeedItem: Identifiable, Hashable {
    let id: String
    let conversationId: String
    let message: MessageInfo
    let receivedAt: Date

    init(conversationId: String, message: MessageInfo, receivedAt: Date = Date()) {
        self.id = message.id
        self.conversationId = conversationId
        self.message = message
        self.receivedAt = receivedAt
    }
}
