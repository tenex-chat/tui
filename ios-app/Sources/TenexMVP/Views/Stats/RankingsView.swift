import SwiftUI

/// Rankings view showing Cost by Project
struct RankingsView: View {
    let snapshot: StatsSnapshot

    var body: some View {
        CostByProjectTable(projects: snapshot.costByProject)
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
                                    .foregroundColor(Color.statCost)
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
            costByProject: [
                ProjectCost(aTag: "31933:abc:project1", name: "TENEX TUI Client", cost: 45.67),
                ProjectCost(aTag: "31933:abc:project2", name: "iOS App", cost: 23.45),
                ProjectCost(aTag: "31933:abc:project3", name: "Backend API", cost: 12.34)
            ],
            messagesByDay: [],
            runtimeByDay: [],
            activityByHour: [],
            maxTokens: 0,
            maxMessages: 0
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
