import SwiftUI

/// Rankings view showing Cost by Project and Top Conversations
/// Matches TUI rankings layout with two side-by-side tables on iPad, stacked on iPhone
struct RankingsView: View {
    let snapshot: StatsSnapshot

    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    var body: some View {
        Group {
            if horizontalSizeClass == .regular {
                // iPad: Side-by-side layout
                HStack(alignment: .top, spacing: 16) {
                    CostByProjectTable(projects: snapshot.costByProject)
                    TopConversationsTable(conversations: snapshot.topConversations)
                }
            } else {
                // iPhone: Stacked layout
                VStack(spacing: 16) {
                    CostByProjectTable(projects: snapshot.costByProject)
                    TopConversationsTable(conversations: snapshot.topConversations)
                }
            }
        }
    }
}

// MARK: - Cost by Project Table

struct CostByProjectTable: View {
    let projects: [ProjectCost]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Cost by Project")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)

            if projects.isEmpty {
                EmptyTableView(message: "No cost data available")
            } else {
                ScrollView {
                    LazyVStack(spacing: 4) {
                        // Header
                        HStack {
                            Text("Project")
                                .font(.caption)
                                .fontWeight(.semibold)
                                .foregroundColor(.secondary)

                            Spacer()

                            Text("Cost")
                                .font(.caption)
                                .fontWeight(.semibold)
                                .foregroundColor(.secondary)
                                .frame(width: 80, alignment: .trailing)
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(Color.systemGray6)

                        // Rows
                        ForEach(Array(projects.enumerated()), id: \.offset) { index, project in
                            HStack {
                                Text(project.name)
                                    .font(.subheadline)
                                    .lineLimit(1)

                                Spacer()

                                Text(String(format: "$%.2f", project.cost))
                                    .font(.subheadline)
                                    .fontWeight(.medium)
                                    .foregroundColor(.green)
                                    .frame(width: 80, alignment: .trailing)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 10)
                            .background(index % 2 == 0 ? Color.systemBackground : Color.systemGray6.opacity(0.5))
                            .accessibilityElement(children: .combine)
                            .accessibilityLabel("\(project.name): \(String(format: "$%.2f", project.cost))")
                        }
                    }
                }
                .frame(maxHeight: 400)
                .background(Color.systemBackground)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.systemGray4, lineWidth: 1)
                )
            }
        }
        .padding()
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color.systemBackground)
                .shadow(color: Color.primary.opacity(0.05), radius: 8, x: 0, y: 2)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.systemGray5, lineWidth: 1)
        )
    }
}

// MARK: - Top Conversations Table

struct TopConversationsTable: View {
    let conversations: [TopConversation]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Top Conversations")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)

            if conversations.isEmpty {
                EmptyTableView(message: "No conversation data available")
            } else {
                ScrollView {
                    LazyVStack(spacing: 4) {
                        // Header
                        HStack {
                            Text("Conversation")
                                .font(.caption)
                                .fontWeight(.semibold)
                                .foregroundColor(.secondary)

                            Spacer()

                            Text("Runtime")
                                .font(.caption)
                                .fontWeight(.semibold)
                                .foregroundColor(.secondary)
                                .frame(width: 90, alignment: .trailing)
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(Color.systemGray6)

                        // Rows
                        ForEach(Array(conversations.enumerated()), id: \.offset) { index, conversation in
                            HStack {
                                Text(conversation.title)
                                    .font(.subheadline)
                                    .lineLimit(1)

                                Spacer()

                                Text(StatsSnapshot.formatRuntime(conversation.runtimeMs))
                                    .font(.subheadline)
                                    .fontWeight(.medium)
                                    .foregroundColor(.blue)
                                    .frame(width: 90, alignment: .trailing)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 10)
                            .background(index % 2 == 0 ? Color.systemBackground : Color.systemGray6.opacity(0.5))
                            .accessibilityElement(children: .combine)
                            .accessibilityLabel("\(conversation.title): \(StatsSnapshot.formatRuntime(conversation.runtimeMs))")
                        }
                    }
                }
                .frame(maxHeight: 400)
                .background(Color.systemBackground)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.systemGray4, lineWidth: 1)
                )
            }
        }
        .padding()
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color.systemBackground)
                .shadow(color: Color.primary.opacity(0.05), radius: 8, x: 0, y: 2)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.systemGray5, lineWidth: 1)
        )
    }
}

// MARK: - Empty Table View

struct EmptyTableView: View {
    let message: String

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "tablecells")
                .font(.largeTitle)
                .foregroundColor(.secondary)

            Text(message)
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 150)
    }
}

#Preview {
    RankingsView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            todayRuntimeMs: 0,
            avgDailyRuntimeMs: 0,
            activeDaysCount: 0,
            runtimeByDay: [],
            costByProject: [
                ProjectCost(aTag: "31933:abc:project1", name: "TENEX TUI Client", cost: 45.67),
                ProjectCost(aTag: "31933:abc:project2", name: "iOS App", cost: 23.45),
                ProjectCost(aTag: "31933:abc:project3", name: "Backend API", cost: 12.34)
            ],
            topConversations: [
                TopConversation(id: "abc123", title: "Implement Stats View", runtimeMs: 5_400_000),
                TopConversation(id: "def456", title: "Fix Activity Grid Bug", runtimeMs: 3_600_000),
                TopConversation(id: "ghi789", title: "Add Dark Mode", runtimeMs: 2_400_000)
            ],
            messagesByDay: [],
            activityByHour: [],
            maxTokens: 0,
            maxMessages: 0
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
