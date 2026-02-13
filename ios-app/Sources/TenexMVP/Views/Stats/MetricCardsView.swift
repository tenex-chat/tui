import SwiftUI

/// Metric cards showing key stats (2-Week Cost, 24h Runtime, 14-day Average)
/// Matches TUI metric cards with proper formatting
struct MetricCardsView: View {
    let snapshot: StatsSnapshot

    var body: some View {
        VStack(spacing: 12) {
            // First row: 2-Week Cost + 24h Runtime
            HStack(spacing: 12) {
                MetricCard(
                    title: "Total Cost",
                    value: String(format: "$%.2f", snapshot.totalCost14Days),
                    subtitle: "past 2 weeks",
                    color: .green
                )

                MetricCard(
                    title: "24h Runtime",
                    value: StatsSnapshot.formatRuntime(snapshot.todayRuntimeMs),
                    subtitle: "today",
                    color: .blue
                )
            }

            // Second row: 14-day Average
            MetricCard(
                title: "Avg (\(snapshot.activeDaysCount)d)",
                value: StatsSnapshot.formatRuntime(snapshot.avgDailyRuntimeMs),
                subtitle: "per day",
                color: .purple
            )
        }
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
            todayRuntimeMs: 3_600_000,
            avgDailyRuntimeMs: 2_400_000,
            activeDaysCount: 10,
            runtimeByDay: [],
            costByProject: [],
            topConversations: [],
            messagesByDay: [],
            activityByHour: [],
            maxTokens: 1000,
            maxMessages: 50
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
