import Foundation
import os.log
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
    private let ffiFastLogLock = NSLock()
    private var ffiFastLogCounter: UInt64 = 0
    private let fileLogger = PerformanceFileLogger.shared
    private var runtimeMonitorsStarted = false

    private init() {
        logEvent(
            "Profiler initialized; file log at \(fileLogger.logPath)",
            category: .general
        )
    }

    var perfLogPath: String {
        fileLogger.logPath
    }

    // MARK: - Runtime Monitors

    /// Starts runtime monitors used for perf investigations.
    /// Safe to call multiple times.
    @MainActor
    func startRuntimeMonitorsIfNeeded() {
        guard !runtimeMonitorsStarted else { return }
        runtimeMonitorsStarted = true

        MemoryMonitor.shared.startMonitoring()
        FrameRateMonitor.shared.startMonitoring()
        MainThreadStallMonitor.shared.startMonitoring()
        logEvent("Runtime monitors started", category: .general)
    }

    @MainActor
    func stopRuntimeMonitors() {
        guard runtimeMonitorsStarted else { return }
        runtimeMonitorsStarted = false

        MemoryMonitor.shared.stopMonitoring()
        FrameRateMonitor.shared.stopMonitoring()
        MainThreadStallMonitor.shared.stopMonitoring()
        logEvent("Runtime monitors stopped", category: .general)
    }

    // MARK: - Lightweight Events

    /// Persist a point-in-time event to both OSLog and the perf file.
    func logEvent(_ message: String, category: ProfilingCategory = .general, level: OSLogType = .info) {
        let log = logForCategory(category)
        os_log(level, log: log, "%{public}@", message)
        fileLogger.log(category: category.label, message: message)
    }

    /// Dynamic-name measurement helper for call sites that cannot use StaticString.
    @discardableResult
    func measure<T>(
        _ name: String,
        category: ProfilingCategory = .general,
        slowThresholdMs: Double = 16,
        _ operation: () throws -> T
    ) rethrows -> T {
        let start = CFAbsoluteTimeGetCurrent()
        let result = try operation()
        let durationMs = (CFAbsoluteTimeGetCurrent() - start) * 1000

        let message = "\(name) took \(String(format: "%.2f", durationMs)) ms [thread=\(Foundation.Thread.isMainThread ? "main" : "background")]"
        let level: OSLogType = durationMs >= slowThresholdMs ? .error : .info
        logEvent(message, category: category, level: level)

        return result
    }

    @discardableResult
    func measureAsync<T>(
        _ name: String,
        category: ProfilingCategory = .general,
        slowThresholdMs: Double = 100,
        _ operation: () async throws -> T
    ) async rethrows -> T {
        let start = CFAbsoluteTimeGetCurrent()
        let result = try await operation()
        let durationMs = (CFAbsoluteTimeGetCurrent() - start) * 1000

        let message = "\(name) took \(String(format: "%.2f", durationMs)) ms"
        let level: OSLogType = durationMs >= slowThresholdMs ? .error : .info
        logEvent(message, category: category, level: level)

        return result
    }

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

        fileLogger.log(
            category: ProfilingCategory.general.label,
            message: "\(String(describing: name)) took \(String(format: "%.2f", duration)) ms"
        )

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

        fileLogger.log(
            category: ProfilingCategory.general.label,
            message: "\(String(describing: name)) took \(String(format: "%.2f", duration)) ms"
        )

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
        if shouldLogFFICall(durationMs: duration) {
            let level: OSLogType = duration >= 50 ? .error : (duration >= 10 ? .info : .debug)
            logEvent(
                "FFI \(name) took \(String(format: "%.2f", duration)) ms",
                category: .ffi,
                level: level
            )
        }

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
        if shouldLogFFICall(durationMs: duration) {
            let level: OSLogType = duration >= 50 ? .error : (duration >= 10 ? .info : .debug)
            logEvent(
                "FFI \(name) took \(String(format: "%.2f", duration)) ms",
                category: .ffi,
                level: level
            )
        }

        return result
    }

    private func shouldLogFFICall(durationMs: Double) -> Bool {
        // Always keep slow-call visibility.
        if durationMs >= 2 {
            return true
        }

        // For very fast calls, sample sparsely to avoid log I/O dominating app runtime.
        ffiFastLogLock.lock()
        defer { ffiFastLogLock.unlock() }
        ffiFastLogCounter += 1
        return ffiFastLogCounter.isMultiple(of: 200)
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
            fileLogger.log(
                category: ProfilingCategory.swiftUI.label,
                message: "Slow view body \(viewName) took \(String(format: "%.2f", duration)) ms"
            )
        }

        return result
    }

    // MARK: - Memory Profiling

    /// Log object allocation
    func logAllocation(_ typeName: String, address: UnsafeRawPointer? = nil) {
        #if DEBUG
        if let addr = address {
            os_log(.debug, log: memoryLog, "➕ Allocated: %{public}@ at %p", typeName, Int(bitPattern: addr))
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
            os_log(.debug, log: memoryLog, "➖ Deallocated: %{public}@ at %p", typeName, Int(bitPattern: addr))
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

