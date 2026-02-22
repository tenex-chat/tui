import SwiftUI

/// Metric cards showing key stats (2-Week Cost)
/// Matches TUI metric cards with proper formatting
struct MetricCardsView: View {
    let snapshot: StatsSnapshot

    var body: some View {
        MetricCard(
            title: "Total Cost",
            value: String(format: "$%.2f", snapshot.totalCost14Days),
            subtitle: "past 2 weeks",
            color: Color.statCost
        )
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Stats Overview")
    }
}

// MARK: - Metric Card

struct MetricCard: View {
    let title: String
    let value: String
    let subtitle: String
    let color: Color

    var body: some View {
        VStack(spacing: 8) {
            Text(title)
                .font(.caption)
                .foregroundColor(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)

            Text(value)
                .font(.system(.title2, design: .rounded))
                .fontWeight(.bold)
                .foregroundColor(color)
                .frame(maxWidth: .infinity, alignment: .leading)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .accessibilityLabel("\(title): \(value)")

            Text(subtitle)
                .font(.caption2)
                .foregroundColor(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
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

#Preview {
    MetricCardsView(
        snapshot: StatsSnapshot(
            totalCost14Days: 123.45,
            costByProject: [],
            messagesByDay: [],
            activityByHour: [],
            maxTokens: 1000,
            maxMessages: 50
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
