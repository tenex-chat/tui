import Foundation
#if os(iOS)
import ZIPFoundation
#endif

actor HtmlReportCache {
    static let shared = HtmlReportCache()

    private var inFlight: Set<String> = []
    private let fileManager = FileManager.default

    private init() {}

    func prefetch(_ reports: [HtmlReport]) async {
        await withTaskGroup(of: Void.self) { group in
            for report in reports where !report.url.isEmpty {
                group.addTask { [weak self] in
                    _ = try? await self?.source(for: report)
                }
            }
        }
    }

    func source(for report: HtmlReport) async throws -> HtmlReportSource {
        let cacheKey = reportCacheKey(report)
        let directory = cacheDirectory(for: cacheKey)

        if let cached = try? cachedSource(for: report, in: directory) {
            return cached
        }

        while inFlight.contains(cacheKey) {
            try await Task.sleep(for: .milliseconds(80))
            if let cached = try? cachedSource(for: report, in: directory) {
                return cached
            }
        }

        inFlight.insert(cacheKey)
        defer { inFlight.remove(cacheKey) }

        guard let url = URL(string: report.url) else {
            throw HtmlReportCacheError.invalidURL(report.url)
        }

        let (data, response) = try await URLSession.shared.data(from: url)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw HtmlReportCacheError.httpStatus(http.statusCode)
        }

        let contentType = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type") ?? ""
        let isZip = contentType.localizedCaseInsensitiveContains("zip") || report.isZip

        if fileManager.fileExists(atPath: directory.path) {
            try? fileManager.removeItem(at: directory)
        }
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)

        if isZip {
            return try extractZip(data: data, into: directory)
        }

        let htmlURL = directory.appendingPathComponent("report.html")
        try data.write(to: htmlURL, options: .atomic)
        let htmlString = decodeHTML(data)
        return .html(content: htmlString, baseURL: url)
    }

    private func cachedSource(for report: HtmlReport, in directory: URL) throws -> HtmlReportSource? {
        guard fileManager.fileExists(atPath: directory.path) else { return nil }

        if report.isZip {
            guard let indexURL = locateIndexHTML(in: directory), hasContent(at: indexURL) else { return nil }
            return .local(indexURL: indexURL, baseDirectory: directory)
        }

        let htmlURL = directory.appendingPathComponent("report.html")
        guard fileManager.fileExists(atPath: htmlURL.path) else { return nil }
        let data = try Data(contentsOf: htmlURL)
        guard !data.isEmpty else { return nil }
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
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
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
