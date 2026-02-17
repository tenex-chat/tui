import SwiftUI

// MARK: - Profiling View

/// Debug view that displays performance and memory metrics.
/// Access from Diagnostics tab or shake gesture in debug builds.
struct ProfilingView: View {
    @State private var ffiSummary: [FFICallSummary] = []
    @State private var memoryLeaks: [MemoryLeak] = []
    @State private var selectedTab = 0

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Tab picker
                Picker("Metrics", selection: $selectedTab) {
                    Text("FFI Calls").tag(0)
                    Text("Memory").tag(1)
                    Text("Tips").tag(2)
                }
                .pickerStyle(.segmented)
                .padding()

                Divider()

                // Content based on tab
                switch selectedTab {
                case 0:
                    ffiMetricsView
                case 1:
                    memoryMetricsView
                case 2:
                    performanceTipsView
                default:
                    ffiMetricsView
                }
            }
            .navigationTitle("Performance Profiling")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Refresh") {
                        refreshMetrics()
                    }
                }
            }
            .onAppear {
                refreshMetrics()
            }
        }
    }

    // MARK: - FFI Metrics View

    private var ffiMetricsView: some View {
        Group {
            if ffiSummary.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "chart.bar.xaxis")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No FFI calls recorded yet")
                        .font(.headline)
                    Text("Use the app to generate FFI call data")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxHeight: .infinity)
            } else {
                List {
                    Section {
                        ForEach(ffiSummary, id: \.name) { summary in
                            FFICallRow(summary: summary)
                        }
                    } header: {
                        HStack {
                            Text("Sorted by total time")
                            Spacer()
                            Button("Reset") {
                                FFIMetrics.shared.reset()
                                refreshMetrics()
                            }
                            .font(.caption)
                        }
                    }
                }
                #if os(iOS)
                .listStyle(.insetGrouped)
                #else
                .listStyle(.inset)
                #endif
            }
        }
    }

    // MARK: - Memory Metrics View

    private var memoryMetricsView: some View {
        Group {
            if memoryLeaks.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 60))
                        .foregroundStyle(Color.presenceOnline)
                    Text("No memory leaks detected")
                        .font(.headline)
                    Text("All tracked objects properly deallocated")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxHeight: .infinity)
            } else {
                List {
                    Section {
                        ForEach(memoryLeaks, id: \.typeName) { leak in
                            MemoryLeakRow(leak: leak)
                        }
                    } header: {
                        HStack {
                            Text("Potential leaks (alloc > dealloc)")
                            Spacer()
                            Button("Reset") {
                                MemoryMetrics.shared.reset()
                                refreshMetrics()
                            }
                            .font(.caption)
                        }
                    } footer: {
                        Text("Note: Some objects may be intentionally long-lived. Investigate objects with high leak counts.")
                    }
                }
                #if os(iOS)
                .listStyle(.insetGrouped)
                #else
                .listStyle(.inset)
                #endif
            }
        }
    }

    // MARK: - Performance Tips View

    private var performanceTipsView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                tipSection(
                    icon: "gauge.with.needle",
                    title: "Instruments Profiling",
                    tips: [
                        "Use Product → Profile in Xcode to launch Instruments",
                        "Time Profiler: Find slow functions",
                        "Allocations: Track memory usage",
                        "Leaks: Detect memory leaks",
                        "SwiftUI: View body computation time"
                    ]
                )

                tipSection(
                    icon: "swift",
                    title: "SwiftUI Best Practices",
                    tips: [
                        "Use LazyVStack/LazyHStack in ScrollViews",
                        "Avoid GeometryReader when possible",
                        "Cache expensive computations with @State",
                        "Use .id() for explicit list identity",
                        "Prefer @StateObject over @ObservedObject for ownership"
                    ]
                )

                tipSection(
                    icon: "memorychip",
                    title: "Memory Management",
                    tips: [
                        "Use [weak self] in async closures",
                        "Cancel Tasks in deinit/onDisappear",
                        "Remove NotificationCenter observers",
                        "Avoid retain cycles in callbacks",
                        "Profile with Instruments Allocations"
                    ]
                )

                tipSection(
                    icon: "cpu",
                    title: "FFI Optimization",
                    tips: [
                        "Batch FFI calls when possible",
                        "Cache results aggressively",
                        "Move heavy FFI work off main thread",
                        "Use signposts to measure FFI duration",
                        "Consider Result caching at Swift layer"
                    ]
                )
            }
            .padding()
        }
    }

    private func tipSection(icon: String, title: String, tips: [String]) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .foregroundStyle(Color.agentBrand)
                Text(title)
                    .font(.headline)
            }

            ForEach(tips, id: \.self) { tip in
                HStack(alignment: .top, spacing: 8) {
                    Text("•")
                        .foregroundStyle(.secondary)
                    Text(tip)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding()
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.systemGray6)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func refreshMetrics() {
        ffiSummary = FFIMetrics.shared.summary
        memoryLeaks = MemoryMetrics.shared.potentialLeaks
    }
}

// MARK: - FFI Call Row

private struct FFICallRow: View {
    let summary: FFICallSummary

    private var severityColor: Color {
        if summary.avgDurationMs > 50 { return .red }
        if summary.avgDurationMs > 16 { return .orange }
        return .green
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(summary.name)
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                Circle()
                    .fill(severityColor)
                    .frame(width: 10, height: 10)
            }

            HStack(spacing: 16) {
                metricBadge(label: "Calls", value: "\(summary.callCount)")
                metricBadge(label: "Total", value: formatDuration(summary.totalDurationMs))
                metricBadge(label: "Avg", value: formatDuration(summary.avgDurationMs))
                metricBadge(label: "Max", value: formatDuration(summary.maxDurationMs))
            }
        }
        .padding(.vertical, 4)
    }

    private func metricBadge(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.tertiary)
            Text(value)
                .font(.caption)
                .fontWeight(.medium)
                .monospacedDigit()
        }
    }

    private func formatDuration(_ ms: Double) -> String {
        if ms >= 1000 {
            return String(format: "%.1fs", ms / 1000)
        }
        return String(format: "%.1fms", ms)
    }
}

// MARK: - Memory Leak Row

private struct MemoryLeakRow: View {
    let leak: MemoryLeak

    private var severityColor: Color {
        if leak.leaked > 10 { return .red }
        if leak.leaked > 5 { return .orange }
        return .yellow
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(leak.typeName)
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                Circle()
                    .fill(severityColor)
                    .frame(width: 10, height: 10)
            }

            HStack(spacing: 16) {
                metricBadge(label: "Allocated", value: "\(leak.allocations)", color: .green)
                metricBadge(label: "Deallocated", value: "\(leak.deallocations)", color: .blue)
                metricBadge(label: "Leaked", value: "\(leak.leaked)", color: severityColor)
            }
        }
        .padding(.vertical, 4)
    }

    private func metricBadge(label: String, value: String, color: Color) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.tertiary)
            Text(value)
                .font(.caption)
                .fontWeight(.medium)
                .foregroundStyle(color)
                .monospacedDigit()
        }
    }
}

// MARK: - Preview

#Preview {
    ProfilingView()
}
