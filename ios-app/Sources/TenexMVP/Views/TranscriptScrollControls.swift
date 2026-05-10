import SwiftUI

struct TranscriptJumpToBottomButton: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: "arrow.down")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(.primary)
                .frame(width: 44, height: 44)
                .background(.regularMaterial, in: Circle())
                .overlay(
                    Circle()
                        .stroke(Color.secondary.opacity(0.14), lineWidth: 1)
                )
                .shadow(color: .black.opacity(0.16), radius: 10, x: 0, y: 4)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Jump to bottom")
        .help("Jump to bottom")
    }
}

extension View {
    @ViewBuilder
    func transcriptBottomVisibilityTracking(
        isAtBottom: Binding<Bool>,
        threshold: CGFloat = 96
    ) -> some View {
        #if os(iOS)
        self.onScrollGeometryChange(for: Bool.self) { geometry in
            let visibleBottom = geometry.contentOffset.y + geometry.containerSize.height
            return visibleBottom >= geometry.contentSize.height - threshold
        } action: { _, newValue in
            guard isAtBottom.wrappedValue != newValue else { return }
            withAnimation(.easeInOut(duration: 0.16)) {
                isAtBottom.wrappedValue = newValue
            }
        }
        #else
        self
        #endif
    }
}
