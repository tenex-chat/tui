#if os(iOS)
import SwiftUI

struct VoiceRecordingSheet: View {
    var dictationManager: DictationManager
    var onStop: () -> Void

    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    private var currentLevel: Float {
        dictationManager.audioLevelSamples.last ?? 0
    }

    private var transcriptText: String {
        if case .recording(let text) = dictationManager.state, !text.isEmpty {
            return text
        }
        return "Listening…"
    }

    var body: some View {
        ZStack {
            VoiceWaveBackground(level: currentLevel)
                .ignoresSafeArea()

            VStack(spacing: 24) {
                Spacer()

                Text(transcriptText)
                    .font(.body)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.primary)
                    .padding(.horizontal, 32)
                    .frame(maxWidth: .infinity)

                Spacer()

                Button(action: onStop) {
                    Image(systemName: "mic.fill")
                        .font(.system(size: 32))
                        .foregroundStyle(.primary)
                        .frame(width: 72, height: 72)
                        .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
                        .clipShape(Circle())
                }
                .buttonStyle(.plain)

                Text("Tap to finish")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Spacer()
                    .frame(height: 8)
            }
            .padding(.vertical, 20)
        }
    }
}

private struct VoiceWaveBackground: View {
    var level: Float
    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 60.0)) { timeline in
            let t = timeline.date.timeIntervalSinceReferenceDate
            let driven = max(0, min(1, Double(level)))
            Canvas { context, size in
                let baseY = size.height / 2
                let idleAmp: CGFloat = 8
                let liveAmp = CGFloat(driven) * size.height * 0.42
                let amp = idleAmp + liveAmp
                let layers: [(speed: Double, freq: Double, offset: Double, scale: CGFloat, opacity: Double)] = [
                    (1.7, 1.4, 0.0, 1.00, 0.55),
                    (1.1, 2.2, 1.3, 0.62, 0.32),
                    (0.6, 3.0, 2.6, 0.38, 0.18)
                ]
                for layer in layers {
                    var path = Path()
                    let steps = max(60, Int(size.width / 3))
                    for i in 0...steps {
                        let progress = Double(i) / Double(steps)
                        let x = CGFloat(progress) * size.width
                        let envelope = sin(progress * .pi)
                        let y = baseY + sin(progress * .pi * 2 * layer.freq + t * layer.speed + layer.offset)
                            * Double(amp * layer.scale) * envelope
                        let pt = CGPoint(x: x, y: CGFloat(y))
                        if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
                    }
                    context.stroke(path, with: .color(Color.accentColor.opacity(layer.opacity)),
                        style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
                }
            }
        }
    }
}
#endif
