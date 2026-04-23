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
        case readFailed(String)

        var errorDescription: String? {
            switch self {
            case .noReadableFileURL:
                return "Could not read dropped file URL."
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

    func mimeTypeForFile(url: URL) -> (mimeType: String, isImage: Bool) {
        switch url.pathExtension.lowercased() {
        case "png":  return ("image/png", true)
        case "jpg", "jpeg": return ("image/jpeg", true)
        case "gif":  return ("image/gif", true)
        case "webp": return ("image/webp", true)
        case "bmp":  return ("image/bmp", true)
        case "heic", "heif": return ("image/heic", true)
        default:
            let mimeType = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType
                ?? "application/octet-stream"
            return (mimeType, false)
        }
    }

    func loadDroppedFile(at fileURL: URL) throws -> (data: Data, mimeType: String, isImage: Bool) {
        let (mimeType, isImage) = mimeTypeForFile(url: fileURL)

        let hasSecurityScope = fileURL.startAccessingSecurityScopedResource()
        defer {
            if hasSecurityScope { fileURL.stopAccessingSecurityScopedResource() }
        }

        do {
            let data = try Data(contentsOf: fileURL)
            return (data: data, mimeType: mimeType, isImage: isImage)
        } catch {
            throw FileDropError.readFailed(fileURL.lastPathComponent)
        }
    }
    #endif
}
