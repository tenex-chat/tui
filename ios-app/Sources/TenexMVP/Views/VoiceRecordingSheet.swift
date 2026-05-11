#if os(iOS)
import SwiftUI
import VoiceCaptureKit

struct VoiceRecordingSheet: View {
    var dictationManager: DictationManager
    var onStop: () -> Void

    private var currentLevel: Float {
        dictationManager.audioLevelSamples.last ?? 0
    }

    private var liveTranscript: String {
        if case .recording(let text) = dictationManager.state, !text.isEmpty {
            return text
        }
        return ""
    }

    var body: some View {
        VoiceCaptureSheet(
            state: VoiceCaptureSheetState(
                isStarting: false,
                isPaused: false,
                level: currentLevel,
                transcript: liveTranscript,
                statusMessage: "Listening",
                errorMessage: dictationManager.error
            ),
            gestureArea: .fullSurface,
            pauseBehavior: .disabled,
            onFinish: onStop
        )
    }
}
#endif
