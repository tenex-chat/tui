import Foundation
import os.signpost
import SwiftUI  // For PerformanceOverlayView and Color types

// MARK: - Performance Profiler

/// Centralized profiling utilities for measuring performance across the app.
/// Uses OSLog signposts for Instruments integration.
///
/// ## Usage
///
/// ```swift
/// // Measure a sync operation
/// PerformanceProfiler.shared.measure("parseMarkdown") {
///     parseMarkdown()
/// }
///
/// // Measure an async operation
/// await PerformanceProfiler.shared.measureAsync("fetchMessages") {
///     await fetchMessages()
/// }
///
/// // Manual signpost control
/// let id = PerformanceProfiler.shared.beginSignpost("longOperation")
/// // ... do work ...
/// PerformanceProfiler.shared.endSignpost("longOperation", id: id)
/// ```
final class PerformanceProfiler {
    static let shared = PerformanceProfiler()

    // MARK: - OSLog Categories

    private let log = OSLog(subsystem: "com.tenex.app", category: "Performance")
    private let ffiLog = OSLog(subsystem: "com.tenex.app", category: "FFI")
    private let swiftUILog = OSLog(subsystem: "com.tenex.app", category: "SwiftUI")
    private let memoryLog = OSLog(subsystem: "com.tenex.app", category: "Memory")

    // MARK: - Signpost IDs

    private var signpostIDs: [String: OSSignpostID] = [:]
    private let lock = NSLock()

    private init() {}

    // MARK: - Measurement Methods

    /// Measure a synchronous operation and log its duration
    @discardableResult
    func measure<T>(_ name: StaticString, _ operation: () throws -> T) rethrows -> T {
        let signpostID = OSSignpostID(log: log)
        os_signpost(.begin, log: log, name: name, signpostID: signpostID)

        let startTime = CFAbsoluteTimeGetCurrent()
        let result = try operation()
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        os_signpost(.end, log: log, name: name, signpostID: signpostID, "%{public}.2f ms", duration)

        if duration > 16 { // > 1 frame at 60fps
            os_log(.info, log: log, "⚠️ Slow operation: %{public}@ took %.2f ms", String(describing: name), duration)
        }

        return result
    }

    /// Measure an async operation and log its duration
    @discardableResult
    func measureAsync<T>(_ name: StaticString, _ operation: () async throws -> T) async rethrows -> T {
        let signpostID = OSSignpostID(log: log)
        os_signpost(.begin, log: log, name: name, signpostID: signpostID)

        let startTime = CFAbsoluteTimeGetCurrent()
        let result = try await operation()
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        os_signpost(.end, log: log, name: name, signpostID: signpostID, "%{public}.2f ms", duration)

        if duration > 100 { // > 100ms for async operations
            os_log(.info, log: log, "⚠️ Slow async operation: %{public}@ took %.2f ms", String(describing: name), duration)
        }

        return result
    }

    // MARK: - FFI Profiling

    /// Measure an FFI call specifically
    @discardableResult
    func measureFFI<T>(_ name: String, _ operation: () throws -> T) rethrows -> T {
        let signpostID = OSSignpostID(log: ffiLog)
        os_signpost(.begin, log: ffiLog, name: "FFI Call", signpostID: signpostID, "%{public}@", name)

        let startTime = CFAbsoluteTimeGetCurrent()
        let result = try operation()
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        os_signpost(.end, log: ffiLog, name: "FFI Call", signpostID: signpostID, "%{public}@ took %.2f ms", name, duration)

        FFIMetrics.shared.recordCall(name: name, durationMs: duration)

        return result
    }

    /// Measure an async FFI call
    @discardableResult
    func measureFFIAsync<T>(_ name: String, _ operation: () async throws -> T) async rethrows -> T {
        let signpostID = OSSignpostID(log: ffiLog)
        os_signpost(.begin, log: ffiLog, name: "FFI Call", signpostID: signpostID, "%{public}@", name)

        let startTime = CFAbsoluteTimeGetCurrent()
        let result = try await operation()
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        os_signpost(.end, log: ffiLog, name: "FFI Call", signpostID: signpostID, "%{public}@ took %.2f ms", name, duration)

        FFIMetrics.shared.recordCall(name: name, durationMs: duration)

        return result
    }

    // MARK: - SwiftUI View Profiling

    /// Log when a view body is being evaluated (call in view body)
    func logViewBody(_ viewName: String, file: String = #file, line: Int = #line) {
        #if DEBUG
        os_log(.debug, log: swiftUILog, "📊 View body evaluated: %{public}@ at %{public}@:%d", viewName, file, line)
        #endif
    }

    /// Measure SwiftUI view computation
    @discardableResult
    func measureViewBody<T>(_ viewName: String, _ body: () -> T) -> T {
        let signpostID = OSSignpostID(log: swiftUILog)
        os_signpost(.begin, log: swiftUILog, name: "View Body", signpostID: signpostID, "%{public}@", viewName)

        let startTime = CFAbsoluteTimeGetCurrent()
        let result = body()
        let duration = (CFAbsoluteTimeGetCurrent() - startTime) * 1000

        os_signpost(.end, log: swiftUILog, name: "View Body", signpostID: signpostID, "%{public}@ took %.2f ms", viewName, duration)

        if duration > 8 { // > half frame
            os_log(.info, log: swiftUILog, "⚠️ Slow view body: %{public}@ took %.2f ms", viewName, duration)
        }

        return result
    }

    // MARK: - Memory Profiling

    /// Log object allocation
    func logAllocation(_ typeName: String, address: UnsafeRawPointer? = nil) {
        #if DEBUG
        if let addr = address {
            os_log(.debug, log: memoryLog, "➕ Allocated: %{public}@ at %p", typeName, addr)
        } else {
            os_log(.debug, log: memoryLog, "➕ Allocated: %{public}@", typeName)
        }
        MemoryMetrics.shared.recordAllocation(typeName)
        #endif
    }

    /// Log object deallocation
    func logDeallocation(_ typeName: String, address: UnsafeRawPointer? = nil) {
        #if DEBUG
        if let addr = address {
            os_log(.debug, log: memoryLog, "➖ Deallocated: %{public}@ at %p", typeName, addr)
        } else {
            os_log(.debug, log: memoryLog, "➖ Deallocated: %{public}@", typeName)
        }
        MemoryMetrics.shared.recordDeallocation(typeName)
        #endif
    }

    // MARK: - Manual Signpost Control

    /// Begin a named signpost (for operations spanning multiple methods)
    func beginSignpost(_ name: String, category: ProfilingCategory = .general) -> OSSignpostID {
        let log = logForCategory(category)
        let id = OSSignpostID(log: log)

        lock.lock()
        signpostIDs[name] = id
        lock.unlock()

        os_signpost(.begin, log: log, name: "Custom", signpostID: id, "%{public}@", name)
        return id
    }

    /// End a named signpost
    func endSignpost(_ name: String, id: OSSignpostID? = nil, category: ProfilingCategory = .general) {
        let log = logForCategory(category)

        let signpostID: OSSignpostID
        if let id = id {
            signpostID = id
        } else {
            lock.lock()
            guard let storedID = signpostIDs.removeValue(forKey: name) else {
                lock.unlock()
                os_log(.error, log: log, "No signpost found for: %{public}@", name)
                return
            }
            signpostID = storedID
            lock.unlock()
        }

        os_signpost(.end, log: log, name: "Custom", signpostID: signpostID, "%{public}@", name)
    }

    private func logForCategory(_ category: ProfilingCategory) -> OSLog {
        switch category {
        case .general: return log
        case .ffi: return ffiLog
        case .swiftUI: return swiftUILog
        case .memory: return memoryLog
        }
    }
}

// MARK: - Profiling Category

enum ProfilingCategory {
    case general
    case ffi
    case swiftUI
    case memory
}

// MARK: - FFI Metrics

/// Collects aggregate metrics for FFI calls
final class FFIMetrics {
    static let shared = FFIMetrics()

    private var callCounts: [String: Int] = [:]
    private var totalDurations: [String: Double] = [:]
    private var maxDurations: [String: Double] = [:]
    private let lock = NSLock()

    private init() {}

    func recordCall(name: String, durationMs: Double) {
        lock.lock()
        defer { lock.unlock() }

        callCounts[name, default: 0] += 1
        totalDurations[name, default: 0] += durationMs
        maxDurations[name] = max(maxDurations[name, default: 0], durationMs)
    }

    /// Get summary statistics
    var summary: [FFICallSummary] {
        lock.lock()
        defer { lock.unlock() }

        return callCounts.map { name, count in
            FFICallSummary(
                name: name,
                callCount: count,
                totalDurationMs: totalDurations[name] ?? 0,
                maxDurationMs: maxDurations[name] ?? 0,
                avgDurationMs: (totalDurations[name] ?? 0) / Double(count)
            )
        }.sorted { $0.totalDurationMs > $1.totalDurationMs }
    }

    func reset() {
        lock.lock()
        defer { lock.unlock() }
        callCounts.removeAll()
        totalDurations.removeAll()
        maxDurations.removeAll()
    }
}

struct FFICallSummary {
    let name: String
    let callCount: Int
    let totalDurationMs: Double
    let maxDurationMs: Double
    let avgDurationMs: Double
}

// MARK: - Memory Metrics

/// Tracks memory allocations for leak detection
final class MemoryMetrics {
    static let shared = MemoryMetrics()

    private var allocations: [String: Int] = [:]
    private var deallocations: [String: Int] = [:]
    private let lock = NSLock()

    private init() {}

    func recordAllocation(_ typeName: String) {
        lock.lock()
        defer { lock.unlock() }
        allocations[typeName, default: 0] += 1
    }

    func recordDeallocation(_ typeName: String) {
        lock.lock()
        defer { lock.unlock() }
        deallocations[typeName, default: 0] += 1
    }

    /// Get types with potential leaks (more allocations than deallocations)
    var potentialLeaks: [MemoryLeak] {
        lock.lock()
        defer { lock.unlock() }

        return allocations.compactMap { typeName, allocCount in
            let deallocCount = deallocations[typeName, default: 0]
            let leakedCount = allocCount - deallocCount
            guard leakedCount > 0 else { return nil }
            return MemoryLeak(typeName: typeName, allocations: allocCount, deallocations: deallocCount, leaked: leakedCount)
        }.sorted { $0.leaked > $1.leaked }
    }

    func reset() {
        lock.lock()
        defer { lock.unlock() }
        allocations.removeAll()
        deallocations.removeAll()
    }
}

struct MemoryLeak {
    let typeName: String
    let allocations: Int
    let deallocations: Int
    let leaked: Int
}

// MARK: - Profiled ViewModel Base

/// Base class for ViewModels that tracks allocation/deallocation
class ProfiledViewModel: ObservableObject {
    init() {
        PerformanceProfiler.shared.logAllocation(String(describing: type(of: self)))
    }

    deinit {
        PerformanceProfiler.shared.logDeallocation(String(describing: type(of: self)))
    }
}

// MARK: - View Modifier for Body Profiling

/// View modifier that logs when a view's body is evaluated
struct ProfileViewBody: ViewModifier {
    let viewName: String

    func body(content: Content) -> some View {
        PerformanceProfiler.shared.logViewBody(viewName)
        return content
    }
}

extension View {
    /// Add profiling to a view's body
    func profileBody(_ name: String) -> some View {
        modifier(ProfileViewBody(viewName: name))
    }
}

// MARK: - Debug Printing

#if DEBUG
extension View {
    /// SwiftUI's built-in debug helper - prints body evaluations to console
    func debugBodyChanges() -> some View {
        Self._printChanges()
        return self
    }
}
#endif

// MARK: - Real-Time Memory Monitor

/// Observable memory monitor for real-time tracking in UI
@MainActor
final class MemoryMonitor: ObservableObject {
    static let shared = MemoryMonitor()

    @Published private(set) var currentMemoryMB: Double = 0
    @Published private(set) var peakMemoryMB: Double = 0
    @Published private(set) var memoryDelta: Double = 0  // Change since last update

    private var timer: Timer?
    private var previousMemory: Double = 0
    private var isMonitoring = false

    private init() {}

    /// Start real-time memory monitoring
    func startMonitoring(interval: TimeInterval = 1.0) {
        guard !isMonitoring else { return }
        isMonitoring = true

        updateMemory()
        timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.updateMemory()
            }
        }
    }

    /// Stop memory monitoring
    func stopMonitoring() {
        timer?.invalidate()
        timer = nil
        isMonitoring = false
    }

    private func updateMemory() {
        let bytes = getMemoryUsage()
        let mb = Double(bytes) / (1024 * 1024)

        memoryDelta = mb - previousMemory
        previousMemory = currentMemoryMB
        currentMemoryMB = mb

        if mb > peakMemoryMB {
            peakMemoryMB = mb
        }
    }

    /// Get current memory usage in bytes
    nonisolated func getMemoryUsage() -> UInt64 {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size) / 4

        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: 1) {
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count)
            }
        }

        return result == KERN_SUCCESS ? info.resident_size : 0
    }

    /// Reset peak tracking
    func resetPeak() {
        peakMemoryMB = currentMemoryMB
    }

    /// Memory status description
    var statusDescription: String {
        if currentMemoryMB > 500 {
            return "Critical"
        } else if currentMemoryMB > 300 {
            return "High"
        } else if currentMemoryMB > 150 {
            return "Moderate"
        }
        return "Normal"
    }

    var statusColor: Color {
        if currentMemoryMB > 500 { return .red }
        if currentMemoryMB > 300 { return .orange }
        if currentMemoryMB > 150 { return .yellow }
        return .green
    }
}

// MARK: - Frame Rate Monitor

/// Monitors frame rate for detecting UI jank
@MainActor
final class FrameRateMonitor: ObservableObject {
    static let shared = FrameRateMonitor()

    @Published private(set) var currentFPS: Double = 60
    @Published private(set) var droppedFrames: Int = 0

    private var displayLink: CADisplayLink?
    private var lastTimestamp: CFTimeInterval = 0
    private var frameCount: Int = 0
    private var isMonitoring = false

    private init() {}

    /// Start frame rate monitoring
    func startMonitoring() {
        guard !isMonitoring else { return }
        isMonitoring = true

        displayLink = CADisplayLink(target: self, selector: #selector(handleDisplayLink(_:)))
        displayLink?.add(to: .main, forMode: .common)
    }

    /// Stop frame rate monitoring
    func stopMonitoring() {
        displayLink?.invalidate()
        displayLink = nil
        isMonitoring = false
    }

    @objc private func handleDisplayLink(_ link: CADisplayLink) {
        if lastTimestamp == 0 {
            lastTimestamp = link.timestamp
            return
        }

        frameCount += 1
        let elapsed = link.timestamp - lastTimestamp

        // Update FPS every second
        if elapsed >= 1.0 {
            currentFPS = Double(frameCount) / elapsed

            // Count dropped frames (target is 60 FPS)
            let expectedFrames = Int(elapsed * 60)
            let dropped = max(0, expectedFrames - frameCount)
            droppedFrames += dropped

            frameCount = 0
            lastTimestamp = link.timestamp

            // Log if FPS drops significantly
            if currentFPS < 45 {
                os_log(.warning, log: OSLog(subsystem: "com.tenex.app", category: "Performance"),
                       "⚠️ Low frame rate: %.1f FPS", currentFPS)
            }
        }
    }

    /// Reset dropped frame counter
    func resetDroppedFrames() {
        droppedFrames = 0
    }
}

// MARK: - Performance Overlay View

/// Floating overlay showing real-time performance metrics
struct PerformanceOverlayView: View {
    @StateObject private var memoryMonitor = MemoryMonitor.shared
    @StateObject private var frameMonitor = FrameRateMonitor.shared
    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            // Compact indicator
            Button(action: { withAnimation { isExpanded.toggle() } }) {
                HStack(spacing: 6) {
                    // FPS indicator
                    Text("\(Int(frameMonitor.currentFPS))")
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(fpsColor)

                    Text("FPS")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Divider()
                        .frame(height: 12)

                    // Memory indicator
                    Text(String(format: "%.0f", memoryMonitor.currentMemoryMB))
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(memoryMonitor.statusColor)

                    Text("MB")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(.ultraThinMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
            .buttonStyle(.plain)

            // Expanded details
            if isExpanded {
                VStack(alignment: .leading, spacing: 8) {
                    // Frame rate section
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Image(systemName: "speedometer")
                                .font(.caption)
                            Text("Frame Rate")
                                .font(.caption.bold())
                        }
                        .foregroundStyle(.secondary)

                        HStack {
                            Text("Current:")
                                .font(.caption2)
                            Spacer()
                            Text(String(format: "%.1f FPS", frameMonitor.currentFPS))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(fpsColor)
                        }

                        HStack {
                            Text("Dropped:")
                                .font(.caption2)
                            Spacer()
                            Text("\(frameMonitor.droppedFrames)")
                                .font(.caption.monospacedDigit())
                        }
                    }

                    Divider()

                    // Memory section
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Image(systemName: "memorychip")
                                .font(.caption)
                            Text("Memory")
                                .font(.caption.bold())
                        }
                        .foregroundStyle(.secondary)

                        HStack {
                            Text("Current:")
                                .font(.caption2)
                            Spacer()
                            Text(String(format: "%.1f MB", memoryMonitor.currentMemoryMB))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(memoryMonitor.statusColor)
                        }

                        HStack {
                            Text("Peak:")
                                .font(.caption2)
                            Spacer()
                            Text(String(format: "%.1f MB", memoryMonitor.peakMemoryMB))
                                .font(.caption.monospacedDigit())
                        }

                        HStack {
                            Text("Status:")
                                .font(.caption2)
                            Spacer()
                            Text(memoryMonitor.statusDescription)
                                .font(.caption2.bold())
                                .foregroundStyle(memoryMonitor.statusColor)
                        }
                    }

                    Divider()

                    // FFI stats quick view
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Image(systemName: "arrow.left.arrow.right")
                                .font(.caption)
                            Text("FFI Calls")
                                .font(.caption.bold())
                        }
                        .foregroundStyle(.secondary)

                        let topCalls = FFIMetrics.shared.summary.prefix(3)
                        ForEach(topCalls, id: \.name) { call in
                            HStack {
                                Text(call.name)
                                    .font(.caption2)
                                    .lineLimit(1)
                                Spacer()
                                Text(String(format: "%.1fms", call.avgDurationMs))
                                    .font(.caption2.monospacedDigit())
                            }
                        }
                    }
                }
                .padding(10)
                .background(.ultraThinMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .frame(width: 160)
            }
        }
        .padding(8)
        .onAppear {
            memoryMonitor.startMonitoring()
            frameMonitor.startMonitoring()
        }
        .onDisappear {
            memoryMonitor.stopMonitoring()
            frameMonitor.stopMonitoring()
        }
    }

    private var fpsColor: Color {
        if frameMonitor.currentFPS < 30 { return .red }
        if frameMonitor.currentFPS < 50 { return .orange }
        return .green
    }
}

// MARK: - View Modifier for Performance Overlay

struct PerformanceOverlayModifier: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        ZStack(alignment: .topTrailing) {
            content
            if enabled {
                PerformanceOverlayView()
            }
        }
    }
}

extension View {
    /// Add performance overlay to view (for debugging)
    func withPerformanceOverlay(enabled: Bool = true) -> some View {
        modifier(PerformanceOverlayModifier(enabled: enabled))
    }
}
