import SwiftUI
#if os(iOS)
import PhotosUI
#endif
#if os(macOS)
import UniformTypeIdentifiers
#endif

extension MessageComposerView {
    var imageAttachmentChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(localImageAttachments) { attachment in
                    ImageAttachmentChipView(attachment: attachment) {
                        removeImageAttachment(id: attachment.id)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    /// Handle image selected from picker - upload to Blossom.
    func handleImageSelected(data: Data, mimeType: String) {
        isUploadingImage = true
        imageUploadError = nil

        Task {
            do {
                try await uploadImageAttachment(data: data, mimeType: mimeType)
                isUploadingImage = false
            } catch {
                isUploadingImage = false
                imageUploadError = error.localizedDescription
                showImageUploadError = true
            }
        }
    }

    func uploadImageAttachment(data: Data, mimeType: String) async throws {
        let url = try await attachmentUploadService.uploadImage(data: data, mimeType: mimeType)

        let imageId = draft.addImageAttachment(url: url)
        let attachment = ImageAttachment(id: imageId, url: url)
        localImageAttachments.append(attachment)

        let marker = "[Image #\(imageId)] "
        localText.append(marker)

        isDirty = true
        if let projectId = selectedProject?.id {
            await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
            await draftManager.updateImageAttachments(localImageAttachments, conversationId: conversationId, projectId: projectId)
        }
    }

    /// Remove an image attachment.
    func removeImageAttachment(id: Int) {
        localImageAttachments.removeAll { $0.id == id }

        let marker = "[Image #\(id)]"
        localText = localText.replacingOccurrences(of: marker + " ", with: "")
        localText = localText.replacingOccurrences(of: marker, with: "")

        draft.removeImageAttachment(id: id)
        isDirty = true
        if let projectId = selectedProject?.id {
            Task {
                await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
                await draftManager.updateImageAttachments(localImageAttachments, conversationId: conversationId, projectId: projectId)
            }
        }
    }

    #if os(macOS)
    func handleFileDrop(providers: [NSItemProvider]) -> Bool {
        guard selectedProject != nil else {
            imageUploadError = "Select a project before dropping files."
            showImageUploadError = true
            return false
        }

        let fileProviders = providers.filter {
            $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier)
        }
        guard !fileProviders.isEmpty else { return false }

        Task {
            await uploadDroppedFiles(from: fileProviders)
        }
        return true
    }

    func uploadDroppedFiles(from providers: [NSItemProvider]) async {
        isUploadingImage = true
        imageUploadError = nil

        var uploadedCount = 0
        var failures: [String] = []

        for provider in providers {
            do {
                let fileURL = try await attachmentUploadService.loadDroppedFileURL(from: provider)
                try await uploadDroppedFile(at: fileURL)
                uploadedCount += 1
            } catch {
                failures.append(error.localizedDescription)
            }
        }

        isUploadingImage = false

        if !failures.isEmpty {
            let prefix = uploadedCount > 0 ? "Some files were skipped:\n" : ""
            imageUploadError = prefix + failures.joined(separator: "\n")
            showImageUploadError = true
        }
    }

    func uploadDroppedFile(at fileURL: URL) async throws {
        let droppedImage = try attachmentUploadService.loadDroppedImage(at: fileURL)
        try await uploadImageAttachment(data: droppedImage.data, mimeType: droppedImage.mimeType)
    }
    #endif
}

struct ImageAttachmentChipView: View {
    let attachment: ImageAttachment
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "photo.fill")
                .font(.caption)
                .foregroundStyle(Color.composerAction)

            Text("Image #\(attachment.id)")
                .font(.caption)
                .foregroundStyle(.primary)

            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.composerAction.opacity(0.1))
        )
    }
}

#if os(iOS)
struct ImagePicker: UIViewControllerRepresentable {
    let onImageSelected: (Data, String) -> Void

    func makeUIViewController(context: Context) -> PHPickerViewController {
        var config = PHPickerConfiguration()
        config.filter = .images
        config.selectionLimit = 1

        let picker = PHPickerViewController(configuration: config)
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: PHPickerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    class Coordinator: NSObject, PHPickerViewControllerDelegate {
        let parent: ImagePicker

        init(_ parent: ImagePicker) {
            self.parent = parent
        }

        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            picker.dismiss(animated: true)

            guard let result = results.first else { return }

            if result.itemProvider.canLoadObject(ofClass: UIImage.self) {
                result.itemProvider.loadObject(ofClass: UIImage.self) { [weak self] object, _ in
                    guard let image = object as? UIImage else { return }

                    let imageData: Data?
                    let mimeType: String

                    if let pngData = image.pngData() {
                        imageData = pngData
                        mimeType = "image/png"
                    } else if let jpegData = image.jpegData(compressionQuality: 0.9) {
                        imageData = jpegData
                        mimeType = "image/jpeg"
                    } else {
                        return
                    }

                    guard let data = imageData else { return }

                    Task { @MainActor in
                        self?.parent.onImageSelected(data, mimeType)
                    }
                }
            }
        }
    }
}
#endif
