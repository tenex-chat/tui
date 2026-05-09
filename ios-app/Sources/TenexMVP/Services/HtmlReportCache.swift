import Foundation
import os.log
#if os(iOS)
import ZIPFoundation
#endif

private enum cacheLog {
    static func info(_ s: String) { print("[HtmlReportCache] \(s)") }
    static func error(_ s: String) { print("[HtmlReportCache][ERROR] \(s)") }
    static func debug(_ s: String) { print("[HtmlReportCache] \(s)") }
}

actor HtmlReportCache {
    static let shared = HtmlReportCache()

    private var inFlight: Set<String> = []
    private let fileManager = FileManager.default

    private init() {}

    /// Wipe the on-disk HTML report cache. Clears the current cache directory
    /// (Caches/) AND any leftover entries at the legacy Application Support
    /// path used before the iOS 26 sandbox-fix migration. Also clears
    /// URLCache.shared so a stale 4xx response can't haunt the next fetch.
    func clearAll() async -> (cleared: Int, errors: [String]) {
        var cleared = 0
        var errors: [String] = []
        let candidates: [URL] = [
            fileManager.urls(for: .cachesDirectory, in: .userDomainMask).first?
                .appendingPathComponent("tenex", isDirectory: true)
                .appendingPathComponent("html-report-cache", isDirectory: true),
            fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
                .appendingPathComponent("tenex", isDirectory: true)
                .appendingPathComponent("html-report-cache", isDirectory: true)
        ].compactMap { $0 }

        for dir in candidates where fileManager.fileExists(atPath: dir.path) {
            do {
                try fileManager.removeItem(at: dir)
                cleared += 1
                cacheLog.info("clearAll removed \(dir.path)")
            } catch {
                errors.append("\(dir.lastPathComponent): \(error.localizedDescription)")
                cacheLog.error("clearAll failed path=\(dir.path) error=\(error.localizedDescription)")
            }
        }

        URLCache.shared.removeAllCachedResponses()
        inFlight.removeAll()
        return (cleared, errors)
    }

    func prefetch(_ reports: [HtmlReport]) async {
        cacheLog.info("prefetch start count=\(reports.count)")
        await withTaskGroup(of: Void.self) { group in
            for report in reports where !report.url.isEmpty {
                group.addTask { [weak self] in
                    do {
                        _ = try await self?.source(for: report)
                    } catch {
                        cacheLog.error("prefetch failed eventId=\(report.eventId) error=\(error.localizedDescription)")
                    }
                }
            }
        }
        cacheLog.info("prefetch done count=\(reports.count)")
    }

    func source(for report: HtmlReport) async throws -> HtmlReportSource {
        let cacheKey = reportCacheKey(report)
        let directory = cacheDirectory(for: cacheKey)
        cacheLog.info("source request eventId=\(report.eventId) url=\(report.url) isZip=\(report.isZip) key=\(cacheKey)")

        if let cached = try? cachedSource(for: report, in: directory) {
            cacheLog.info("source cache-hit eventId=\(report.eventId)")
            return cached
        }

        var waitedMs: Int = 0
        while inFlight.contains(cacheKey) {
            cacheLog.debug("source waiting on inFlight eventId=\(report.eventId) waitedMs=\(waitedMs)")
            try await Task.sleep(for: .milliseconds(80))
            waitedMs += 80
            if let cached = try? cachedSource(for: report, in: directory) {
                cacheLog.info("source cache-hit-after-wait eventId=\(report.eventId) waitedMs=\(waitedMs)")
                return cached
            }
            if waitedMs > 30_000 {
                cacheLog.error("source inFlight wedged — giving up wait eventId=\(report.eventId) waitedMs=\(waitedMs)")
                break
            }
        }

        inFlight.insert(cacheKey)
        defer { inFlight.remove(cacheKey) }

        guard let url = URL(string: report.url) else {
            cacheLog.error("source invalid URL eventId=\(report.eventId) url=\(report.url)")
            throw HtmlReportCacheError.invalidURL(report.url)
        }

        cacheLog.info("source download start eventId=\(report.eventId)")
        let downloadStart = Date()
        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await URLSession.shared.data(from: url)
        } catch {
            cacheLog.error("source download failed eventId=\(report.eventId) error=\(error.localizedDescription)")
            throw error
        }
        let downloadMs = Int(Date().timeIntervalSince(downloadStart) * 1000)
        let httpStatus = (response as? HTTPURLResponse)?.statusCode ?? -1
        cacheLog.info("source download complete eventId=\(report.eventId) bytes=\(data.count) http=\(httpStatus) ms=\(downloadMs)")

        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw HtmlReportCacheError.httpStatus(http.statusCode)
        }

        let contentType = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type") ?? ""
        let isZip = contentType.localizedCaseInsensitiveContains("zip") || report.isZip
        cacheLog.debug("source content-type=\(contentType) isZip=\(isZip)")

        if fileManager.fileExists(atPath: directory.path) {
            try? fileManager.removeItem(at: directory)
        }
        do {
            try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        } catch {
            cacheLog.error("source createDirectory failed path=\(directory.path) error=\(error.localizedDescription)")
            throw error
        }

        if isZip {
            do {
                let extracted = try extractZip(data: data, into: directory)
                cacheLog.info("source zip extracted eventId=\(report.eventId)")
                return extracted
            } catch {
                cacheLog.error("source zip extract failed eventId=\(report.eventId) error=\(error.localizedDescription)")
                throw error
            }
        }

        let htmlURL = directory.appendingPathComponent("report.html")
        do {
            try data.write(to: htmlURL, options: .atomic)
        } catch {
            cacheLog.error("source write failed eventId=\(report.eventId) path=\(htmlURL.path) error=\(error.localizedDescription)")
            throw error
        }
        let htmlString = decodeHTML(data)
        cacheLog.info("source html cached eventId=\(report.eventId) chars=\(htmlString.count)")
        return .html(content: htmlString, baseURL: url)
    }

    private func cachedSource(for report: HtmlReport, in directory: URL) throws -> HtmlReportSource? {
        guard fileManager.fileExists(atPath: directory.path) else {
            cacheLog.debug("cachedSource miss-no-dir eventId=\(report.eventId) path=\(directory.path)")
            return nil
        }

        if report.isZip {
            guard let indexURL = locateIndexHTML(in: directory) else {
                cacheLog.debug("cachedSource miss-no-index eventId=\(report.eventId)")
                return nil
            }
            guard hasContent(at: indexURL) else {
                cacheLog.debug("cachedSource miss-empty-index eventId=\(report.eventId) path=\(indexURL.path)")
                return nil
            }
            return .local(indexURL: indexURL, baseDirectory: directory)
        }

        let htmlURL = directory.appendingPathComponent("report.html")
        guard fileManager.fileExists(atPath: htmlURL.path) else {
            cacheLog.debug("cachedSource miss-no-html eventId=\(report.eventId)")
            return nil
        }
        let data = try Data(contentsOf: htmlURL)
        guard !data.isEmpty else {
            cacheLog.debug("cachedSource miss-empty-html eventId=\(report.eventId)")
            return nil
        }
        let baseURL = URL(string: report.url) ?? htmlURL
        return .html(content: decodeHTML(data), baseURL: baseURL)
    }

    private func extractZip(data: Data, into directory: URL) throws -> HtmlReportSource {
        let zipURL = directory.appendingPathComponent("bundle.zip")
        try data.write(to: zipURL, options: .atomic)

        #if os(macOS)
        try runUnzip(zipURL: zipURL, into: directory)
        #else
        try fileManager.unzipItem(at: zipURL, to: directory)
        #endif
        try? fileManager.removeItem(at: zipURL)

        guard let indexURL = locateIndexHTML(in: directory) else {
            throw HtmlReportCacheError.missingIndex
        }

        return .local(indexURL: indexURL, baseDirectory: directory)
    }

    #if os(macOS)
    private func runUnzip(zipURL: URL, into directory: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-o", "-q", zipURL.path, "-d", directory.path]
        let stderr = Pipe()
        process.standardError = stderr
        process.standardOutput = Pipe()
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let errorData = stderr.fileHandleForReading.readDataToEndOfFile()
            let message = String(data: errorData, encoding: .utf8) ?? "unzip exited \(process.terminationStatus)"
            throw HtmlReportCacheError.unzipFailed(message)
        }
    }
    #endif

    private func locateIndexHTML(in directory: URL) -> URL? {
        guard let enumerator = fileManager.enumerator(
            at: directory,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles]
        ) else { return nil }

        var firstHtml: URL?
        var bestIndex: URL?
        var bestIndexDepth = Int.max

        for case let fileURL as URL in enumerator {
            guard (try? fileURL.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile == true else { continue }
            let name = fileURL.lastPathComponent.lowercased()
            let depth = fileURL.pathComponents.count
            if name == "index.html" {
                if depth < bestIndexDepth { bestIndex = fileURL; bestIndexDepth = depth }
            } else if (name.hasSuffix(".html") || name.hasSuffix(".htm")) && firstHtml == nil {
                firstHtml = fileURL
            }
        }
        return bestIndex ?? firstHtml
    }

    private func cacheDirectory(for key: String) -> URL {
        // Use Caches/ rather than Application Support/. WKWebView's WebContent
        // process can fail to read from Application Support/ even with
        // allowingReadAccessTo:, leaving local zip-extracted reports rendering
        // blank. Caches/ also matches the semantics of an HTTP cache (transient,
        // not backed up).
        let base = fileManager.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? fileManager.temporaryDirectory
        return base
            .appendingPathComponent("tenex", isDirectory: true)
            .appendingPathComponent("html-report-cache", isDirectory: true)
            .appendingPathComponent(key, isDirectory: true)
    }

    private func reportCacheKey(_ report: HtmlReport) -> String {
        let raw = report.eventId.isEmpty ? report.url : report.eventId
        let safe = raw.map { character -> Character in
            character.isLetter || character.isNumber ? character : "-"
        }
        return String(safe).prefix(96).description
    }

    private func decodeHTML(_ data: Data) -> String {
        String(data: data, encoding: .utf8) ?? String(data: data, encoding: .isoLatin1) ?? ""
    }

    private func hasContent(at url: URL) -> Bool {
        guard let values = try? url.resourceValues(forKeys: [.fileSizeKey]) else { return false }
        return (values.fileSize ?? 0) > 0
    }
}

enum HtmlReportCacheError: LocalizedError {
    case invalidURL(String)
    case httpStatus(Int)
    case unzipFailed(String)
    case missingIndex

    var errorDescription: String? {
        switch self {
        case .invalidURL(let url): return "Invalid URL: \(url)"
        case .httpStatus(let code): return "Server returned HTTP \(code)"
        case .unzipFailed(let message): return "Failed to unzip bundle: \(message)"
        case .missingIndex: return "Bundle does not contain an index.html"
        }
    }
}
