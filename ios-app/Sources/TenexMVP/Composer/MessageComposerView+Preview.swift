import SwiftUI

#Preview("New Conversation") {
    MessageComposerView(
        project: Project(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            isDeleted: false,
            pubkey: "",
            participants: [],
            agentDefinitionIds: [],
            mcpToolIds: [],
            createdAt: 0
        )
    )
    .environmentObject(TenexCoreManager())
}

#Preview("Reply") {
    MessageComposerView(
        project: Project(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            isDeleted: false,
            pubkey: "",
            participants: [],
            agentDefinitionIds: [],
            mcpToolIds: [],
            createdAt: 0
        ),
        conversationId: "conv-123",
        conversationTitle: "Test Conversation"
    )
    .environmentObject(TenexCoreManager())
}
