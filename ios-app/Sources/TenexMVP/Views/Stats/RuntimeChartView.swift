import SwiftUI
import Charts

/// 14-day runtime bar chart using Swift Charts
/// Matches TUI runtime chart with proper date labels
struct RuntimeChartView: View {
    let snapshot: StatsSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("LLM Runtime (Last 14 Days)")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)

            if snapshot.runtimeByDay.isEmpty {
                EmptyChartView(message: "No runtime data available")
            } else {
                Chart {
                    ForEach(snapshot.runtimeByDay, id: \.dayStart) { dayData in
                        BarMark(
                            x: .value("Date", dayLabel(for: dayData.dayStart)),
                            y: .value("Runtime (ms)", dayData.runtimeMs)
                        )
                        .foregroundStyle(Color.blue.gradient)
                        .accessibilityLabel("\(dayLabel(for: dayData.dayStart)): \(StatsSnapshot.formatRuntime(dayData.runtimeMs))")
                    }
                }
                .chartYAxis {
                    AxisMarks(position: .leading) { value in
                        AxisValueLabel {
                            if let ms = value.as(UInt64.self) {
                                Text(StatsSnapshot.formatRuntime(ms))
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
                .frame(height: 300)
                .padding(.bottom, 40) // Extra space for rotated labels
            }
        }
        .padding()
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color.systemBackground)
                .shadow(color: Color.black.opacity(0.05), radius: 8, x: 0, y: 2)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.systemGray5, lineWidth: 1)
        )
    }

    private func dayLabel(for dayStart: UInt64) -> String {
        StatsSnapshot.formatDayLabel(dayStart, todayStart: StatsSnapshot.todayStart)
    }
}

// MARK: - Empty Chart View

struct EmptyChartView: View {
    let message: String

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "chart.bar.xaxis")
                .font(.largeTitle)
                .foregroundColor(.secondary)

            Text(message)
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 200)
    }
}

#Preview {
    let sampleData = (0..<14).map { offset in
        let secondsPerDay: UInt64 = 86400
        let now = UInt64(Date().timeIntervalSince1970)
        let todayStart = (now / secondsPerDay) * secondsPerDay
        let dayStart = todayStart - (UInt64(offset) * secondsPerDay)

        return DayRuntime(
            dayStart: dayStart,
            runtimeMs: UInt64.random(in: 0...7_200_000) // 0-2 hours
        )
    }

    return RuntimeChartView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            todayRuntimeMs: 0,
            avgDailyRuntimeMs: 0,
            activeDaysCount: 0,
            runtimeByDay: sampleData,
            costByProject: [],
            topConversations: [],
            messagesByDay: [],
            activityByHour: [],
            maxTokens: 0,
            maxMessages: 0
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
