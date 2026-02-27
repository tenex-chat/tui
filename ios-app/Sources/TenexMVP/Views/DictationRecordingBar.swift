import SwiftUI

/// Compact inline recording bar that replaces the composer toolbar during dictation.
/// Shows an audio waveform, elapsed timer, and stop button — no labels, no overlay.
struct DictationRecordingBar: View {
    let audioLevelSamples: [Float]
    let recordingStartDate: Date?
    var error: String?
    let onStop: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            AudioWaveformView(samples: audioLevelSamples)
                .frame(maxWidth: .infinity)

            if let error {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.caption2)
                    .foregroundStyle(.red.opacity(0.8))
                    .help(error)
            }

            ElapsedTimeView(startDate: recordingStartDate)

            Button(action: onStop) {
                Image(systemName: "stop.fill")
                    .font(.system(size: 10))
                    .foregroundStyle(.primary)
                    .frame(width: 28, height: 28)
                    .background(
                        Circle()
                            .fill(Color.secondary.opacity(0.28))
                    )
            }
            .buttonStyle(.borderless)
            .help("Stop recording")
        }
    }
}

// MARK: - Audio Waveform

private struct AudioWaveformView: View {
    let samples: [Float]

    private let barCount = 60
    private let barSpacing: CGFloat = 2
    private let minBarHeight: CGFloat = 2
    private let maxBarHeight: CGFloat = 18

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: barSpacing) {
                ForEach(0..<barCount, id: \.self) { index in
                    let level = sampleLevel(at: index)
                    RoundedRectangle(cornerRadius: 1)
                        .fill(Color.secondary.opacity(0.5))
                        .frame(
                            width: max((geo.size.width - barSpacing * CGFloat(barCount - 1)) / CGFloat(barCount), 1),
                            height: minBarHeight + (maxBarHeight - minBarHeight) * CGFloat(level)
                        )
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(height: maxBarHeight)
    }

    private func sampleLevel(at index: Int) -> Float {
        guard !samples.isEmpty else { return 0.05 }

        // Map bar index to sample index, right-aligned (newest samples on the right)
        let sampleIndex = samples.count - barCount + index
        guard sampleIndex >= 0, sampleIndex < samples.count else { return 0.05 }
        return max(samples[sampleIndex], 0.05)
    }
}

// MARK: - Elapsed Time

private struct ElapsedTimeView: View {
    let startDate: Date?

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let elapsed = startDate.map { context.date.timeIntervalSince($0) } ?? 0
            let minutes = Int(elapsed) / 60
            let seconds = Int(elapsed) % 60
            Text(String(format: "%d:%02d", minutes, seconds))
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
    }
}
