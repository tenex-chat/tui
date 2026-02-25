#if os(iOS)
import SwiftUI

/// Overlay view for voice dictation that shows recording state and transcription editing.
struct DictationOverlayView: View {
    @Bindable var manager: DictationManager
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency
    let onComplete: (String) -> Void
    let onCancel: () -> Void

    var body: some View {
        ZStack {
            // Semi-transparent background
            Color.black.opacity(0.4)
                .ignoresSafeArea()
                .onTapGesture {
                    // Cancel on background tap
                    manager.cancelRecording()
                    onCancel()
                }

            // Content card
            VStack(spacing: 0) {
                switch manager.state {
                case .idle:
                    // Show final text if available, otherwise nothing
                    if !manager.finalText.isEmpty {
                        FinalTranscriptionView(
                            originalText: manager.finalText,
                            phoneticLearner: manager.phoneticLearner,
                            onConfirm: { finalText in
                                onComplete(finalText)
                                manager.reset()
                            },
                            onCancel: {
                                manager.reset()
                                onCancel()
                            }
                        )
                    }

                case .recording(let partialText):
                    RecordingView(
                        partialText: partialText,
                        onStop: {
                            Task {
                                await manager.stopRecording()
                            }
                        },
                        onCancel: {
                            manager.cancelRecording()
                            onCancel()
                        }
                    )
                }

                // Error display
                if let error = manager.error {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(Color.recordingActive)
                        .padding(.top, 8)
                }
            }
            .padding(20)
            .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
            .clipShape(RoundedRectangle(cornerRadius: 20))
            .shadow(radius: 20)
            .padding(.horizontal, 24)
        }
    }
}

// MARK: - Recording View

struct RecordingView: View {
    let partialText: String
    let onStop: () -> Void
    let onCancel: () -> Void

    @State private var pulseAnimation = false
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var body: some View {
        VStack(spacing: 16) {
            // Pulsing mic indicator
            ZStack {
                Circle()
                    .fill(Color.recordingActiveBackground)
                    .frame(width: 80, height: 80)
                    .scaleEffect(pulseAnimation ? 1.2 : 1.0)
                    .animation(reduceMotion ? nil : .easeInOut(duration: 0.8).repeatForever(autoreverses: true), value: pulseAnimation)

                Circle()
                    .fill(Color.recordingActive)
                    .frame(width: 60, height: 60)

                Image(systemName: "mic.fill")
                    .font(.title)
                    .foregroundStyle(.white)
            }
            .onAppear {
                if !reduceMotion {
                    pulseAnimation = true
                }
            }

            Text("Listening...")
                .font(.headline)
                .foregroundStyle(.primary)

            // Partial transcription
            if !partialText.isEmpty {
                Text(partialText)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
                    .padding()
                    .background(Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
            }

            // Controls
            HStack(spacing: 20) {
                Button(action: onCancel) {
                    Label("Cancel", systemImage: "xmark.circle.fill")
                        .font(.subheadline)
                }
                .adaptiveGlassButtonStyle()

                Button(action: onStop) {
                    Label("Done", systemImage: "stop.circle.fill")
                        .font(.subheadline)
                }
                .adaptiveProminentGlassButtonStyle()
                .tint(Color.recordingActive)
            }
        }
    }
}

// MARK: - Final Transcription View

struct FinalTranscriptionView: View {
    @State private var editedText: String
    let originalText: String
    let phoneticLearner: PhoneticLearner
    let onConfirm: (String) -> Void
    let onCancel: () -> Void

    init(originalText: String, phoneticLearner: PhoneticLearner, onConfirm: @escaping (String) -> Void, onCancel: @escaping () -> Void) {
        _editedText = State(initialValue: originalText)
        self.originalText = originalText
        self.phoneticLearner = phoneticLearner
        self.onConfirm = onConfirm
        self.onCancel = onCancel
    }

    var body: some View {
        VStack(spacing: 16) {
            Text("Edit Transcription")
                .font(.headline)

            // Editable text area
            TextEditor(text: $editedText)
                .font(.body)
                .frame(minHeight: 100)
                .padding(8)
                .background(Color.systemGray6)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .scrollContentBackground(.hidden)

            // Original text for reference (only if different)
            if editedText != originalText {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Original:")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Text(originalText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            // Actions
            HStack(spacing: 16) {
                Button("Cancel") { onCancel() }
                    .adaptiveGlassButtonStyle()

                Button("Insert") {
                    learnFromEdits()
                    onConfirm(editedText)
                }
                .adaptiveProminentGlassButtonStyle()
            }
        }
    }

    private func learnFromEdits() {
        // Compare word-by-word and record phonetically similar corrections
        let originalWords = originalText.split(separator: " ").map(String.init)
        let editedWords = editedText.split(separator: " ").map(String.init)

        for (orig, edited) in zip(originalWords, editedWords) where orig != edited {
            phoneticLearner.recordCorrection(original: orig, replacement: edited)
        }
    }
}

// MARK: - Preview

#Preview("Recording") {
    DictationOverlayView(
        manager: {
            let m = DictationManager()
            return m
        }(),
        onComplete: { _ in },
        onCancel: { }
    )
}
#endif
