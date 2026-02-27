import SwiftUI

// MARK: - Delegation Tree View

/// Root view for the delegation tree. Mac/iPad only.
/// Takes a root conversation ID, loads the full delegation subtree, and renders it as
/// a scrollable canvas with bezier arrows connecting parent/child nodes.
struct DelegationTreeView: View {
    let rootConversationId: String

    @Environment(TenexCoreManager.self) var coreManager
    @StateObject private var viewModel = DelegationTreeViewModel()
    @State private var selectedNode: DelegationTreeNode?

    var body: some View {
        GeometryReader { proxy in
            HStack(spacing: 0) {
                canvasArea
                if let node = selectedNode {
                    DelegationDetailPanel(node: node) {
                        withAnimation(.spring(response: 0.28)) {
                            selectedNode = nil
                        }
                    }
                    .frame(width: detailPanelWidth(totalWidth: proxy.size.width))
                    .transition(.move(edge: .trailing))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
        .task {
            viewModel.safeCore = coreManager.safeCore
            await viewModel.loadTree(rootConversationId: rootConversationId)
        }
        .navigationTitle(viewModel.rootNode?.conversation.thread.title ?? "Delegation Tree")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                if !viewModel.isLoading {
                    HStack(spacing: 8) {
                        Label("\(viewModel.totalNodeCount) agents", systemImage: "person.2")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Label("\(viewModel.edges.count) delegations", systemImage: "arrow.triangle.branch")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        // Legend
                        HStack(spacing: 6) {
                            legendItem(color: Color(hex: "#86efac"), label: "Completed")
                            legendItem(color: Color(hex: "#fbbf24"), label: "Pending")
                        }
                    }
                }
            }
        }
    }

    private func detailPanelWidth(totalWidth: CGFloat) -> CGFloat {
        min(840, totalWidth * 0.5)
    }

    @ViewBuilder
    private var canvasArea: some View {
        if viewModel.isLoading {
            ProgressView("Loading delegation tree...")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let error = viewModel.loadError {
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.largeTitle)
                    .foregroundStyle(.secondary)
                Text(error)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            DelegationCanvasView(
                viewModel: viewModel,
                selectedNode: $selectedNode
            )
        }
    }

    private func legendItem(color: Color, label: String) -> some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 1)
                .fill(color)
                .frame(width: 16, height: 2)
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }
}

// MARK: - Delegation Canvas View

private struct DelegationCanvasView: View {
    @ObservedObject var viewModel: DelegationTreeViewModel
    @Binding var selectedNode: DelegationTreeNode?
    @State private var zoomScale: CGFloat = 1.0
    @GestureState private var pinchScale: CGFloat = 1.0

    private let minZoom: CGFloat = 0.55
    private let maxZoom: CGFloat = 2.4

    private var effectiveZoomScale: CGFloat {
        min(max(zoomScale * pinchScale, minZoom), maxZoom)
    }

    var body: some View {
        ScrollView([.horizontal, .vertical]) {
            ZStack(alignment: .topLeading) {
                // Layer 0: Arrow paths (Canvas, non-interactive)
                Canvas { context, _ in
                    drawArrows(context: context)
                }
                .frame(width: viewModel.canvasSize.width, height: viewModel.canvasSize.height)

                // Layer 1: Node cards
                if let root = viewModel.rootNode {
                    ForEach(allNodes(from: root)) { node in
                        if let pos = viewModel.nodePositions[node.id] {
                            DelegationNodeCard(
                                node: node,
                                isSelected: selectedNode?.id == node.id
                            ) {
                                withAnimation(.spring(response: 0.28)) {
                                    if selectedNode?.id == node.id {
                                        selectedNode = nil
                                    } else {
                                        selectedNode = node
                                    }
                                }
                            }
                            .frame(width: 270, height: 148)
                            .position(x: pos.x + 135, y: pos.y + 74)
                        }
                    }
                }

            }
            .frame(width: viewModel.canvasSize.width, height: viewModel.canvasSize.height)
            .scaleEffect(effectiveZoomScale, anchor: .topLeading)
            .frame(
                width: viewModel.canvasSize.width * effectiveZoomScale,
                height: viewModel.canvasSize.height * effectiveZoomScale,
                alignment: .topLeading
            )
        }
        .background(Color.systemGroupedBackground)
        .simultaneousGesture(
            MagnificationGesture()
                .updating($pinchScale) { value, state, _ in
                    state = value
                }
                .onEnded { value in
                    zoomScale = min(max(zoomScale * value, minZoom), maxZoom)
                }
        )
    }

    private func drawArrows(context: GraphicsContext) {
        for edge in viewModel.edges {
            guard let fromPos = viewModel.nodePositions[edge.parentId],
                  let toPos = viewModel.nodePositions[edge.childId] else { continue }

            let fromCenter = CGPoint(x: fromPos.x + 270, y: fromPos.y + 74)
            let toCenter = CGPoint(x: toPos.x, y: toPos.y + 74)
            let arrowColor: Color = edge.isComplete ? Color(hex: "#86efac") : Color(hex: "#fbbf24")

            drawBezierArrow(
                context: context,
                from: fromCenter,
                to: toCenter,
                color: arrowColor
            )

            if let targetProjectLabel = edge.crossProjectTargetLabel {
                drawCrossProjectLabel(
                    context: context,
                    from: fromCenter,
                    to: toCenter,
                    label: "to \(targetProjectLabel)"
                )
            }
        }
    }

    private func drawBezierArrow(
        context: GraphicsContext,
        from: CGPoint,
        to: CGPoint,
        color: Color
    ) {
        let dx = abs(to.x - from.x) * 0.55
        var path = Path()
        let p0 = from
        let p1 = CGPoint(x: from.x + dx, y: from.y)
        let p2 = CGPoint(x: to.x - dx, y: to.y)
        let p3 = to
        path.move(to: p0)
        path.addCurve(to: p3, control1: p1, control2: p2)

        context.stroke(path, with: .color(color), lineWidth: 1.8)

        // Arrowhead at destination
        let arrowSize: CGFloat = 6
        let angle = atan2(p3.y - p2.y, p3.x - p2.x)
        var arrowPath = Path()
        arrowPath.move(to: p3)
        arrowPath.addLine(to: CGPoint(
            x: p3.x - arrowSize * cos(angle - .pi / 6),
            y: p3.y - arrowSize * sin(angle - .pi / 6)
        ))
        arrowPath.addLine(to: CGPoint(
            x: p3.x - arrowSize * cos(angle + .pi / 6),
            y: p3.y - arrowSize * sin(angle + .pi / 6)
        ))
        arrowPath.closeSubpath()
        context.fill(arrowPath, with: .color(color))
    }

    private func drawCrossProjectLabel(
        context: GraphicsContext,
        from: CGPoint,
        to: CGPoint,
        label: String
    ) {
        let center = CGPoint(
            x: (from.x + to.x) / 2,
            y: (from.y + to.y) / 2 - 14
        )

        let text = Text(label)
            .font(.caption2)
            .fontWeight(.semibold)
            .foregroundStyle(Color(hex: "#f1f5f9"))

        let resolved = context.resolve(text)
        let measured = resolved.measure(in: CGSize(width: 220, height: 24))

        let rect = CGRect(
            x: center.x - measured.width / 2 - 6,
            y: center.y - measured.height / 2 - 2,
            width: measured.width + 12,
            height: measured.height + 4
        )

        let badgePath = Path(roundedRect: rect, cornerRadius: 6)
        context.fill(badgePath, with: .color(Color.black.opacity(0.6)))
        context.stroke(badgePath, with: .color(Color.white.opacity(0.15)), lineWidth: 0.8)
        context.draw(resolved, at: center)
    }

    private func allNodes(from node: DelegationTreeNode) -> [DelegationTreeNode] {
        var result = [node]
        for child in node.children {
            result.append(contentsOf: allNodes(from: child))
        }
        return result
    }
}

// MARK: - Delegation Node Card

private struct DelegationNodeCard: View {
    @Environment(TenexCoreManager.self) var coreManager
    let node: DelegationTreeNode
    let isSelected: Bool
    let onTap: () -> Void

    private var conversation: ConversationFullInfo { node.conversation }

    private var statusColor: Color {
        Color.conversationStatus(for: conversation.thread.statusLabel, isActive: conversation.isActive)
    }

    private var participantPubkey: String {
        node.participantPubkey
    }

    private var participantDisplayName: String {
        if node.role == .rootAuthor {
            return conversation.author
        }

        let resolved = coreManager.displayName(for: participantPubkey)
        if !resolved.isEmpty, resolved != participantPubkey {
            return resolved
        }

        if participantPubkey.count > 18 {
            return "\(participantPubkey.prefix(8))...\(participantPubkey.suffix(6))"
        }
        return participantPubkey
    }

    private var displayTimestamp: UInt64 {
        conversation.thread.effectiveLastActivity
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Header: avatar + name + active pulse
            HStack(spacing: 8) {
                AgentAvatarView(
                    agentName: participantDisplayName,
                    pubkey: participantPubkey,
                    size: 28
                )
                .environment(coreManager)

                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 4) {
                        Text(participantDisplayName)
                            .font(.caption)
                            .fontWeight(.semibold)
                            .lineLimit(1)

                        if conversation.isActive {
                            Circle()
                                .fill(Color.presenceOnline)
                                .frame(width: 6, height: 6)
                                .animation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true), value: conversation.isActive)
                        }
                    }

                    if let status = conversation.thread.statusLabel {
                        Text(status)
                            .font(.caption2)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(statusColor.opacity(0.15))
                            .foregroundStyle(statusColor)
                            .clipShape(Capsule())
                    }
                }

                Spacer()

                RelativeTimeText(timestamp: displayTimestamp, style: .localizedAbbreviated)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }

            // Title
            Text(conversation.thread.title)
                .font(.caption)
                .fontWeight(.medium)
                .lineLimit(2)

            // Message preview: summary, last message, or nothing
            if let summary = conversation.thread.summary {
                Text(summary)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            } else if let lastMessage = node.lastMessage,
                      !lastMessage.content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text(lastMessage.content.trimmingCharacters(in: .whitespacesAndNewlines))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer(minLength: 0)

            // Footer: message count
            HStack {
                if conversation.messageCount > 0 {
                    HStack(spacing: 2) {
                        Image(systemName: "bubble.left")
                            .font(.caption2)
                        Text("\(conversation.messageCount)")
                            .font(.caption2)
                    }
                    .foregroundStyle(.tertiary)
                }
                Spacer()
            }
        }
        .padding(10)
        .background(Color.systemBackground)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(isSelected ? Color.accentColor : Color.systemGray4, lineWidth: isSelected ? 2 : 0.5)
        )
        .shadow(color: .black.opacity(0.06), radius: 4, x: 0, y: 2)
        .onTapGesture(perform: onTap)
    }
}

// MARK: - Delegation Detail Panel

private struct DelegationDetailPanel: View {
    @Environment(TenexCoreManager.self) var coreManager
    let node: DelegationTreeNode
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Panel header with close button
            HStack {
                Text("Conversation Detail")
                    .font(.headline)
                Spacer()
                Button(action: onClose) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(Color.systemBackground)

            Divider()

            NavigationStack {
                ConversationAdaptiveDetailView(conversation: node.conversation)
                    .environment(coreManager)
            }
            .id(node.id)
        }
        .background(Color.systemBackground)
        .overlay(alignment: .leading) {
            Divider()
        }
    }
}

// MARK: - Color Hex Extension

extension Color {
    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&int)
        let a: UInt64
        let r: UInt64
        let g: UInt64
        let b: UInt64
        switch hex.count {
        case 3:
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6:
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8:
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255,
            green: Double(g) / 255,
            blue: Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
}
