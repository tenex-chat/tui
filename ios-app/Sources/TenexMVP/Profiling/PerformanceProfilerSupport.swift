import Foundation
import os.log
import SwiftUI

// MARK: - Profiling Category

enum ProfilingCategory {
    case general
    case ffi
    case swiftUI
    case memory

    var label: String {
        switch self {
        case .general: return "GENERAL"
        case .ffi: return "FFI"
        case .swiftUI: return "SWIFTUI"
        case .memory: return "MEMORY"
        }
    }
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

// MARK: - Persistent Perf Log

/// Thread-safe, append-only file logger used to persist profiling output.
final class PerformanceFileLogger {
    static let shared = PerformanceFileLogger()

    private let queue = DispatchQueue(label: "com.tenex.app.perf-file-logger", qos: .utility)
    private let formatter = ISO8601DateFormatter()
    private let sessionStartUptime = ProcessInfo.processInfo.systemUptime
    private let maxLogBytes: UInt64 = 20 * 1024 * 1024
    private let logURL: URL

    var logPath: String {
        logURL.path
    }

    private init() {
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        logURL = Self.makeLogURL()
        prepareLogFile()
        log(category: "GENERAL", message: "----- New app session -----")
    }

    func log(category: String, message: String) {
        let now = Date()
        let uptimeMs = Int((ProcessInfo.processInfo.systemUptime - sessionStartUptime) * 1000)
        queue.async { [formatter, logURL] in
            let timestamp = formatter.string(from: now)
            let line = "[\(timestamp)] [+\(uptimeMs)ms] [\(category)] \(message)\n"
            Self.append(line: line, to: logURL)
        }
    }

    private func prepareLogFile() {
        let directoryURL = logURL.deletingLastPathComponent()
        do {
            try FileManager.default.createDirectory(
                at: directoryURL,
                withIntermediateDirectories: true,
                attributes: nil
            )
        } catch {
            return
        }

        let attributes = try? FileManager.default.attributesOfItem(atPath: logURL.path)
        let existingSize = attributes?[.size] as? UInt64 ?? 0
        if existingSize > maxLogBytes {
            try? FileManager.default.removeItem(at: logURL)
        }

        if !FileManager.default.fileExists(atPath: logURL.path) {
            FileManager.default.createFile(atPath: logURL.path, contents: nil)
        }
    }

    private static func makeLogURL() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
        return base
            .appendingPathComponent("tenex", isDirectory: true)
            .appendingPathComponent("logs", isDirectory: true)
            .appendingPathComponent("tenex-perf.log", isDirectory: false)
    }

    private static func append(line: String, to url: URL) {
        guard let data = line.data(using: .utf8) else { return }
        if let handle = try? FileHandle(forWritingTo: url) {
            defer { try? handle.close() }
            do {
                try handle.seekToEnd()
                try handle.write(contentsOf: data)
            } catch {
                return
            }
        }
    }
}

// MARK: - Main Thread Stall Monitor

/// Detects main-thread stalls by checking timer drift on the main queue.
final class MainThreadStallMonitor {
    static let shared = MainThreadStallMonitor()

    private let heartbeatInterval: TimeInterval = 0.05
    private let watchdogInterval: TimeInterval = 0.05
    private let stallThreshold: TimeInterval = 0.2
    private let stallLogRepeatInterval: TimeInterval = 1.0
    private let stallSampleThresholdMs: Double = 750
    private let stallSampleCooldown: TimeInterval = 20.0
    private let maxSamplesPerStallSequence: UInt64 = 6

    private let stateLock = NSLock()
    private let watchdogQueue = DispatchQueue(label: "com.tenex.app.stall-watchdog", qos: .userInitiated)

    private var mainHeartbeatTimer: DispatchSourceTimer?
    private var watchdogTimer: DispatchSourceTimer?
    private var isMonitoring = false
    private var lastHeartbeatUptime: TimeInterval = 0
    private var currentStallStartUptime: TimeInterval?
    private var currentStallSequence: UInt64 = 0
    private var currentStallSampleCount: UInt64 = 0
    private var lastStallLogUptime: TimeInterval = 0
    private var lastSampleUptime: TimeInterval = 0
    private var sampleCaptureCount: UInt64 = 0

    private init() {}

    func startMonitoring() {
        stateLock.lock()
        if isMonitoring {
            stateLock.unlock()
            return
        }
        isMonitoring = true
        let now = ProcessInfo.processInfo.systemUptime
        lastHeartbeatUptime = now
        currentStallStartUptime = nil
        lastStallLogUptime = 0
        lastSampleUptime = 0
        currentStallSampleCount = 0
        stateLock.unlock()

        let heartbeat = DispatchSource.makeTimerSource(queue: .main)
        heartbeat.schedule(
            deadline: .now() + heartbeatInterval,
            repeating: heartbeatInterval,
            leeway: .milliseconds(5)
        )
        heartbeat.setEventHandler { [weak self] in
            self?.recordMainHeartbeat()
        }
        mainHeartbeatTimer = heartbeat
        heartbeat.resume()

        let watchdog = DispatchSource.makeTimerSource(queue: watchdogQueue)
        watchdog.schedule(
            deadline: .now() + watchdogInterval,
            repeating: watchdogInterval,
            leeway: .milliseconds(10)
        )
        watchdog.setEventHandler { [weak self] in
            self?.handleWatchdogTick()
        }
        watchdogTimer = watchdog
        watchdog.resume()

        #if os(macOS)
        PerformanceProfiler.shared.logEvent(
            "MainThreadStallMonitor started heartbeatMs=\(Int(heartbeatInterval * 1000)) stallThresholdMs=\(Int(stallThreshold * 1000)) sampleThresholdMs=\(Int(stallSampleThresholdMs)) sampleDir=\(Self.stallSamplesDirectory.path)",
            category: .general
        )
        #else
        PerformanceProfiler.shared.logEvent(
            "MainThreadStallMonitor started heartbeatMs=\(Int(heartbeatInterval * 1000)) stallThresholdMs=\(Int(stallThreshold * 1000))",
            category: .general
        )
        #endif
    }

    func stopMonitoring() {
        mainHeartbeatTimer?.cancel()
        mainHeartbeatTimer = nil
        watchdogTimer?.cancel()
        watchdogTimer = nil

        stateLock.lock()
        isMonitoring = false
        currentStallStartUptime = nil
        lastStallLogUptime = 0
        stateLock.unlock()
    }

    private func recordMainHeartbeat() {
        stateLock.lock()
        lastHeartbeatUptime = ProcessInfo.processInfo.systemUptime
        stateLock.unlock()
    }

    private func handleWatchdogTick() {
        let now = ProcessInfo.processInfo.systemUptime
        var inProgressLog: String?
        var inProgressLogLevel: OSLogType = .error
        var recoveryLog: String?
        var recoveryLogLevel: OSLogType = .info
        var sampleRequest: (stallMs: Double, sequence: UInt64, captureCount: UInt64)?

        stateLock.lock()
        let heartbeatAge = now - lastHeartbeatUptime
        if heartbeatAge >= stallThreshold {
            if currentStallStartUptime == nil {
                currentStallStartUptime = lastHeartbeatUptime
                currentStallSequence &+= 1
                currentStallSampleCount = 0
                lastStallLogUptime = 0
            }

            let stallMs = heartbeatAge * 1000
            if lastStallLogUptime == 0 || (now - lastStallLogUptime) >= stallLogRepeatInterval {
                lastStallLogUptime = now
                inProgressLog = "Main thread stall in-progress seq=\(currentStallSequence) stallMs=\(String(format: "%.1f", stallMs)) heartbeatAgeMs=\(String(format: "%.1f", stallMs))"
                inProgressLogLevel = stallMs >= 1000 ? .error : .info
            }

            if stallMs >= stallSampleThresholdMs &&
                (now - lastSampleUptime) >= stallSampleCooldown &&
                currentStallSampleCount < maxSamplesPerStallSequence {
                lastSampleUptime = now
                sampleCaptureCount &+= 1
                currentStallSampleCount &+= 1
                sampleRequest = (stallMs, currentStallSequence, sampleCaptureCount)
            }
        } else if let stallStart = currentStallStartUptime {
            let totalMs = (now - stallStart) * 1000
            recoveryLog = "Main thread stall recovered seq=\(currentStallSequence) totalMs=\(String(format: "%.1f", totalMs))"
            recoveryLogLevel = totalMs >= 1000 ? .error : .info
            currentStallStartUptime = nil
            currentStallSampleCount = 0
            lastStallLogUptime = 0
        }
        stateLock.unlock()

        if let inProgressLog {
            PerformanceProfiler.shared.logEvent(
                inProgressLog,
                category: .general,
                level: inProgressLogLevel
            )
        }

        if let recoveryLog {
            PerformanceProfiler.shared.logEvent(
                recoveryLog,
                category: .general,
                level: recoveryLogLevel
            )
        }

        if let sampleRequest {
            #if os(macOS)
            captureStallSample(
                stallMs: sampleRequest.stallMs,
                sequence: sampleRequest.sequence,
                captureCount: sampleRequest.captureCount
            )
            #endif
        }
    }

    #if os(macOS)
    private static let stallSamplesDirectory: URL = URL(
        fileURLWithPath: NSTemporaryDirectory(),
        isDirectory: true
    ).appendingPathComponent("tenex-stall-samples", isDirectory: true)

    private func captureStallSample(stallMs: Double, sequence: UInt64, captureCount: UInt64) {
        let pid = ProcessInfo.processInfo.processIdentifier
        let timestampMs = Int(Date().timeIntervalSince1970 * 1000)
        let sampleURL = Self.stallSamplesDirectory.appendingPathComponent(
            "sample-\(timestampMs)-seq\(sequence)-n\(captureCount).txt",
            isDirectory: false
        )

        watchdogQueue.async {
            do {
                try FileManager.default.createDirectory(
                    at: Self.stallSamplesDirectory,
                    withIntermediateDirectories: true,
                    attributes: nil
                )
            } catch {
                PerformanceProfiler.shared.logEvent(
                    "stall sample skipped seq=\(sequence) reason=create-directory-failed error=\(error.localizedDescription)",
                    category: .general,
                    level: .error
                )
                return
            }

            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/sample")
            process.arguments = ["\(pid)", "1", "-file", sampleURL.path]
            process.standardOutput = Pipe()
            let stderr = Pipe()
            process.standardError = stderr

            let startedAt = CFAbsoluteTimeGetCurrent()
            do {
                try process.run()
                process.waitUntilExit()
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                let stderrData = stderr.fileHandleForReading.readDataToEndOfFile()
                let stderrText = String(data: stderrData, encoding: .utf8)?
                    .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

                if process.terminationStatus == 0 {
                    Self.pruneStallSamples(maxFiles: 200, maxTotalBytes: 256 * 1024 * 1024)
                    PerformanceProfiler.shared.logEvent(
                        "stall sample captured seq=\(sequence) stallMs=\(String(format: "%.1f", stallMs)) capture=\(captureCount) elapsedMs=\(String(format: "%.1f", elapsedMs)) file=\(sampleURL.path)",
                        category: .general,
                        level: .error
                    )
                } else {
                    let stderrSuffix = stderrText.isEmpty ? "" : " stderr=\(stderrText)"
                    PerformanceProfiler.shared.logEvent(
                        "stall sample failed seq=\(sequence) capture=\(captureCount) exit=\(process.terminationStatus)\(stderrSuffix)",
                        category: .general,
                        level: .error
                    )
                }
            } catch {
                PerformanceProfiler.shared.logEvent(
                    "stall sample launch failed seq=\(sequence) capture=\(captureCount) error=\(error.localizedDescription)",
                    category: .general,
                    level: .error
                )
            }
        }
    }

    private static func pruneStallSamples(maxFiles: Int, maxTotalBytes: UInt64) {
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: stallSamplesDirectory,
            includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey, .isRegularFileKey],
            options: [.skipsHiddenFiles]
        ) else {
            return
        }

        let sampleFiles = contents.filter { $0.lastPathComponent.hasPrefix("sample-") }
        if sampleFiles.count <= maxFiles {
            let totalSize = sampleFiles.reduce(UInt64(0)) { partial, url in
                let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                return partial + UInt64(max(size, 0))
            }
            if totalSize <= maxTotalBytes {
                return
            }
        }

        let sortedByAge = sampleFiles.sorted { lhs, rhs in
            let lhsDate = (try? lhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            let rhsDate = (try? rhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            return lhsDate < rhsDate
        }

        var keptCount = sortedByAge.count
        var totalSize = sortedByAge.reduce(UInt64(0)) { partial, url in
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            return partial + UInt64(max(size, 0))
        }

        for url in sortedByAge {
            let size = UInt64(max((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0, 0))
            let overFileLimit = keptCount > maxFiles
            let overByteLimit = totalSize > maxTotalBytes
            if !overFileLimit && !overByteLimit {
                break
            }

            do {
                try FileManager.default.removeItem(at: url)
                keptCount -= 1
                totalSize = totalSize >= size ? (totalSize - size) : 0
            } catch {
                continue
            }
        }
    }
    #endif
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
        if currentMemoryMB > 300 { return .accentColor }
        if currentMemoryMB > 150 { return .yellow }
        return .accentColor
    }
}

// MARK: - Frame Rate Monitor

#if os(iOS)
/// Monitors frame rate for detecting UI jank using CADisplayLink
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

    func startMonitoring() {
        guard !isMonitoring else { return }
        isMonitoring = true

        displayLink = CADisplayLink(target: self, selector: #selector(handleDisplayLink(_:)))
        displayLink?.add(to: .main, forMode: .common)
    }

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

        if elapsed >= 1.0 {
            currentFPS = Double(frameCount) / elapsed

            let expectedFrames = Int(elapsed * 60)
            let dropped = max(0, expectedFrames - frameCount)
            droppedFrames += dropped

            frameCount = 0
            lastTimestamp = link.timestamp

            if currentFPS < 45 {
                os_log(.error, log: OSLog(subsystem: "com.tenex.app", category: "Performance"),
                       "⚠️ Low frame rate: %.1f FPS", currentFPS)
                PerformanceProfiler.shared.logEvent(
                    "Low frame rate detected: \(String(format: "%.1f", currentFPS)) FPS",
                    category: .general,
                    level: .error
                )
            }
        }
    }

    func resetDroppedFrames() {
        droppedFrames = 0
    }
}
#elseif os(macOS)
/// macOS stub - frame rate monitoring not available without CADisplayLink
@MainActor
final class FrameRateMonitor: ObservableObject {
    static let shared = FrameRateMonitor()

    @Published private(set) var currentFPS: Double = 60
    @Published private(set) var droppedFrames: Int = 0

    private init() {}

    func startMonitoring() {}
    func stopMonitoring() {}
    func resetDroppedFrames() { droppedFrames = 0 }
}
#endif

// MARK: - Performance Overlay View

/// Floating overlay showing real-time performance metrics
struct PerformanceOverlayView: View {
    @StateObject private var memoryMonitor = MemoryMonitor.shared
    @StateObject private var frameMonitor = FrameRateMonitor.shared
    @State private var isExpanded = false
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            // Compact indicator
            Button(action: {
                if reduceMotion {
                    isExpanded.toggle()
                } else {
                    withAnimation { isExpanded.toggle() }
                }
            }) {
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
                .background {
                    if reduceTransparency {
                        RoundedRectangle(cornerRadius: 6)
                            .fill(.regularMaterial)
                    } else if #available(iOS 26.0, macOS 26.0, *) {
                        RoundedRectangle(cornerRadius: 6)
                            .glassEffect(.clear)
                    } else {
                        RoundedRectangle(cornerRadius: 6)
                            .fill(.regularMaterial)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
            .buttonStyle(.borderless)

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
                .background {
                    if reduceTransparency {
                        RoundedRectangle(cornerRadius: 8)
                            .fill(.regularMaterial)
                    } else if #available(iOS 26.0, macOS 26.0, *) {
                        RoundedRectangle(cornerRadius: 8)
                            .glassEffect(.clear)
                    } else {
                        RoundedRectangle(cornerRadius: 8)
                            .fill(.regularMaterial)
                    }
                }
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
        if frameMonitor.currentFPS < 50 { return .accentColor }
        return .accentColor
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
