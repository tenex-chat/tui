import Foundation
#if os(macOS)
import UniformTypeIdentifiers
#endif

struct AttachmentUploadService {
    let core: CoreGateway

    func uploadImage(data: Data, mimeType: String) async throws -> String {
        try await core.uploadImage(data: data, mimeType: mimeType)
    }

    #if os(macOS)
    enum FileDropError: LocalizedError {
        case noReadableFileURL
        case unsupportedFileType(String)
        case readFailed(String)

        var errorDescription: String? {
            switch self {
            case .noReadableFileURL:
                return "Could not read dropped file URL."
            case .unsupportedFileType(let name):
                return "Unsupported file '\(name)'. Supported: png, jpg, jpeg, gif, webp, bmp."
            case .readFailed(let name):
                return "Failed to read '\(name)'."
            }
        }
    }

    func loadDroppedFileURL(from provider: NSItemProvider) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }
                if let url = item as? URL {
                    continuation.resume(returning: url)
                    return
                }
                if let data = item as? Data,
                   let url = URL(dataRepresentation: data, relativeTo: nil) {
                    continuation.resume(returning: url)
                    return
                }
                if let text = item as? String,
                   let url = URL(string: text) {
                    continuation.resume(returning: url)
                    return
                }
                continuation.resume(throwing: FileDropError.noReadableFileURL)
            }
        }
    }

    func mimeTypeForDroppedImage(url: URL) -> String? {
        switch url.pathExtension.lowercased() {
        case "png":
            return "image/png"
        case "jpg", "jpeg":
            return "image/jpeg"
        case "gif":
            return "image/gif"
        case "webp":
            return "image/webp"
        case "bmp":
            return "image/bmp"
        default:
            return nil
        }
    }

    func loadDroppedImage(at fileURL: URL) throws -> (data: Data, mimeType: String) {
        guard let mimeType = mimeTypeForDroppedImage(url: fileURL) else {
            throw FileDropError.unsupportedFileType(fileURL.lastPathComponent)
        }

        let hasSecurityScope = fileURL.startAccessingSecurityScopedResource()
        defer {
            if hasSecurityScope {
                fileURL.stopAccessingSecurityScopedResource()
            }
        }

        do {
            let data = try Data(contentsOf: fileURL)
            return (data: data, mimeType: mimeType)
        } catch {
            throw FileDropError.readFailed(fileURL.lastPathComponent)
        }
    }
    #endif
}
