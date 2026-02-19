import SwiftUI

#Preview("New Conversation") {
    MessageComposerView(
        project: ProjectInfo(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            createdAt: 0,
            agentIds: [],
            mcpToolIds: [],
            isDeleted: false
        )
    )
    .environmentObject(TenexCoreManager())
}

#Preview("Reply") {
    MessageComposerView(
        project: ProjectInfo(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            createdAt: 0,
            agentIds: [],
            mcpToolIds: [],
            isDeleted: false
        ),
        conversationId: "conv-123",
        conversationTitle: "Test Conversation"
    )
    .environmentObject(TenexCoreManager())
}
