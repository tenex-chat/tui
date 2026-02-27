import SwiftUI
import Charts

/// Messages chart showing user vs all messages per day
/// Matches TUI messages chart with dual-bar visualization
struct MessagesChartView: View {
    let snapshot: StatsSnapshot
    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Messages (Last 14 Days)")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)

            if snapshot.messagesByDay.isEmpty {
                EmptyChartView(message: "No message data available")
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    // Legend
                    HStack(spacing: 16) {
                        LegendItem(color: Color.statUserMessages, label: "You")
                        LegendItem(color: Color.statAllMessages, label: "All")
                    }
                    .font(.caption)

                    // Chart
                    Chart {
                        ForEach(snapshot.messagesByDay, id: \.dayStart) { dayData in
                            // User messages bar
                            BarMark(
                                x: .value("Date", dayLabel(for: dayData.dayStart)),
                                y: .value("Count", dayData.userCount)
                            )
                            .foregroundStyle(Color.statUserMessages.gradient)
                            .position(by: .value("Type", "User"))
                            .accessibilityLabel("\(dayLabel(for: dayData.dayStart)) - You: \(dayData.userCount) messages")

                            // All messages bar
                            BarMark(
                                x: .value("Date", dayLabel(for: dayData.dayStart)),
                                y: .value("Count", dayData.allCount)
                            )
                            .foregroundStyle(Color.statAllMessages.gradient)
                            .position(by: .value("Type", "All"))
                            .accessibilityLabel("\(dayLabel(for: dayData.dayStart)) - All: \(dayData.allCount) messages")
                        }
                    }
                    .chartYAxis {
                        AxisMarks(position: .leading) { value in
                            AxisValueLabel {
                                if let count = value.as(UInt64.self) {
                                    Text("\(count)")
                                        .font(.caption2)
                                }
                            }
                            AxisGridLine()
                        }
                    }
                    .chartXAxis {
                        AxisMarks { value in
                            AxisValueLabel {
                                if let label = value.as(String.self) {
                                    Text(label)
                                        .font(.caption2)
                                        .rotationEffect(.degrees(-45))
                                }
                            }
                        }
                    }
                    .chartLegend(.hidden) // Use custom legend above
                    .frame(height: 300)
                    .padding(.bottom, 40) // Extra space for rotated labels
                }
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

    private func dayLabel(for dayStart: UInt64) -> String {
        Self.dayFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(dayStart)))
    }
}

// MARK: - Legend Item

struct LegendItem: View {
    let color: Color
    let label: String

    var body: some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 2)
                .fill(color)
                .frame(width: 12, height: 12)

            Text(label)
                .foregroundColor(.secondary)
        }
    }
}

#Preview {
    let sampleData = (0..<14).map { offset in
        let secondsPerDay: UInt64 = 86400
        let now = UInt64(Date().timeIntervalSince1970)
        let todayStart = (now / secondsPerDay) * secondsPerDay
        let dayStart = todayStart - (UInt64(offset) * secondsPerDay)

        let allCount = UInt64.random(in: 10...100)
        let userCount = UInt64.random(in: 5...allCount)

        return DayMessages(
            dayStart: dayStart,
            userCount: userCount,
            allCount: allCount
        )
    }
    MessagesChartView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            costByProject: [],
            messagesByDay: sampleData,
            runtimeByDay: [],
            activityByHour: [],
            maxTokens: 0,
            maxMessages: 0
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
