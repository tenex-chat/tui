import SwiftUI
import Charts

/// 14-day runtime chart with accumulated total.
struct RuntimeChartView: View {
    let snapshot: StatsSnapshot

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Accumulated Runtime (Last 14 Days)")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)

            if !snapshot.runtimeByDay.isEmpty {
                Text("Total: \(formatRuntime(totalRuntimeMs))")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            if snapshot.runtimeByDay.isEmpty {
                EmptyChartView(message: "No runtime data available")
            } else {
                Chart {
                    ForEach(snapshot.runtimeByDay, id: \.dayStart) { dayData in
                        BarMark(
                            x: .value("Date", dayLabel(for: dayData.dayStart)),
                            y: .value("Runtime", dayData.runtimeMs)
                        )
                        .foregroundStyle(Color.statRuntime.gradient)
                        .accessibilityLabel("\(dayLabel(for: dayData.dayStart)): \(formatRuntime(dayData.runtimeMs))")
                    }
                }
                .chartYAxis {
                    AxisMarks(position: .leading) { value in
                        AxisValueLabel {
                            if let ms = value.as(UInt64.self) {
                                Text(formatRuntime(ms))
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
                .padding(.bottom, 40)
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

    private var totalRuntimeMs: UInt64 {
        snapshot.runtimeByDay.reduce(0) { partial, day in
            partial + day.runtimeMs
        }
    }

    private func dayLabel(for dayStart: UInt64) -> String {
        let secondsPerDay: UInt64 = 86400
        let now = UInt64(Date().timeIntervalSince1970)
        let todayStart = (now / secondsPerDay) * secondsPerDay
        let daysDiff = (todayStart.saturatingSubtracting(dayStart)) / secondsPerDay

        switch daysDiff {
        case 0:
            return "Today"
        case 1:
            return "Yest."
        default:
            return Self.dayFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(dayStart)))
        }
    }

    private func formatRuntime(_ ms: UInt64) -> String {
        let seconds = ms / 1000
        if seconds == 0 && ms > 0 {
            return "\(ms)ms"
        }
        if seconds == 0 {
            return "0s"
        }
        if seconds < 60 {
            return "\(seconds)s"
        }
        if seconds < 3600 {
            let mins = seconds / 60
            let secs = seconds % 60
            return secs > 0 ? "\(mins)m \(secs)s" : "\(mins)m"
        }

        let hours = seconds / 3600
        let mins = (seconds % 3600) / 60
        return mins > 0 ? "\(hours)h \(mins)m" : "\(hours)h"
    }
}

private extension UInt64 {
    func saturatingSubtracting(_ value: UInt64) -> UInt64 {
        self >= value ? self - value : 0
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
            runtimeMs: UInt64.random(in: 0...7_200_000)
        )
    }

    RuntimeChartView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            costByProject: [],
            messagesByDay: [],
            runtimeByDay: sampleData,
            activityByHour: [],
            maxTokens: 0,
            maxMessages: 0
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
