import SwiftUI

struct NudgeDraftSubmission {
    let title: String
    let description: String
    let content: String
    let hashtags: [String]
    let allowTools: [String]
    let denyTools: [String]
    let onlyTools: [String]
}

private enum NewNudgeStep: Int, CaseIterable, Identifiable {
    case basics
    case content
    case tools
    case review

    var id: Int { rawValue }

    var title: String {
        switch self {
        case .basics: return "Basics"
        case .content: return "Content"
        case .tools: return "Tools"
        case .review: return "Review"
        }
    }

    var symbol: String {
        switch self {
        case .basics: return "character.textbox"
        case .content: return "doc.text"
        case .tools: return "wrench.and.screwdriver"
        case .review: return "checklist"
        }
    }
}

private enum NudgeToolMode: String, CaseIterable, Identifiable {
    case additive
    case exclusive

    var id: String { rawValue }

    var title: String {
        switch self {
        case .additive: return "Allow + Deny"
        case .exclusive: return "Only Tools"
        }
    }

    var subtitle: String {
        switch self {
        case .additive:
            return "Modify an agent's default tool set."
        case .exclusive:
            return "Agent receives exactly this tool set."
        }
    }
}

struct NewNudgeSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    let sourceNudge: Nudge?
    let availableTools: [String]
    let onSubmit: (NudgeDraftSubmission) async -> Bool

    @State private var step: NewNudgeStep = .basics
    @State private var title: String
    @State private var description: String
    @State private var content: String
    @State private var hashtagsInput: String
    @State private var toolMode: NudgeToolMode
    @State private var allowTools: Set<String>
    @State private var denyTools: Set<String>
    @State private var onlyTools: Set<String>
    @State private var toolSearchText = ""
    @State private var isCreating = false
    @State private var errorMessage: String?

    init(
        sourceNudge: Nudge? = nil,
        availableTools: [String],
        onSubmit: @escaping (NudgeDraftSubmission) async -> Bool
    ) {
        self.sourceNudge = sourceNudge
        self.availableTools = availableTools
        self.onSubmit = onSubmit

        _title = State(initialValue: sourceNudge?.title ?? "")
        _description = State(initialValue: sourceNudge?.description ?? "")
        _content = State(initialValue: sourceNudge?.content ?? "")
        _hashtagsInput = State(initialValue: sourceNudge?.hashtags.joined(separator: ", ") ?? "")

        if let sourceNudge, !sourceNudge.onlyTools.isEmpty {
            _toolMode = State(initialValue: .exclusive)
            _allowTools = State(initialValue: [])
            _denyTools = State(initialValue: [])
            _onlyTools = State(initialValue: Set(sourceNudge.onlyTools))
        } else {
            _toolMode = State(initialValue: .additive)
            _allowTools = State(initialValue: Set(sourceNudge?.allowedTools ?? []))
            _denyTools = State(initialValue: Set(sourceNudge?.deniedTools ?? []))
            _onlyTools = State(initialValue: [])
        }
    }

    private var sheetTitle: String {
        sourceNudge == nil ? "New Nudge" : "Fork Nudge"
    }

    private var canProceed: Bool {
        switch step {
        case .basics:
            return !title.trimmed.isEmpty
        case .content:
            return !content.trimmed.isEmpty
        case .tools:
            if toolMode == .exclusive {
                return !onlyTools.isEmpty
            }
            return true
        case .review:
            return true
        }
    }

    private var sortedTools: [String] {
        availableTools.sorted { lhs, rhs in
            lhs.localizedCaseInsensitiveCompare(rhs) == .orderedAscending
        }
    }

    private var filteredTools: [String] {
        let query = toolSearchText.trimmed.lowercased()
        guard !query.isEmpty else { return sortedTools }
        return sortedTools.filter { $0.lowercased().contains(query) }
    }

    private var toolOverlapCount: Int {
        allowTools.intersection(denyTools).count
    }

    private var parsedHashtags: [String] {
        hashtagsInput
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .map { $0.hasPrefix("#") ? String($0.dropFirst()) : $0 }
            .filter { !$0.isEmpty }
            .reduce(into: [String]()) { partialResult, tag in
                if !partialResult.contains(tag) {
                    partialResult.append(tag)
                }
            }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    headerCard
                    stepRail
                    stepContent
                }
                .padding(.horizontal, 18)
                .padding(.top, 16)
                .padding(.bottom, 84)
            }
            .background(backgroundView)
            .navigationTitle(sheetTitle)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .disabled(isCreating)
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            actionBar
        }
        .onChange(of: toolMode) { _, newValue in
            switch newValue {
            case .additive:
                onlyTools.removeAll()
            case .exclusive:
                allowTools.removeAll()
                denyTools.removeAll()
            }
        }
        .alert(
            "Unable to Save Nudge",
            isPresented: Binding(
                get: { errorMessage != nil },
                set: { isPresented in
                    if !isPresented {
                        errorMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                errorMessage = nil
            }
        } message: {
            Text(errorMessage ?? "Unknown error")
        }
        .tenexModalPresentation(detents: [.large])
        #if os(macOS)
        .frame(minWidth: 780, idealWidth: 900, minHeight: 660, idealHeight: 760)
        #endif
    }

    private var backgroundView: some View {
        LinearGradient(
            colors: [
                Color.agentBrand.opacity(reduceTransparency ? 0.03 : 0.10),
                Color.systemGroupedBackground,
                Color.systemGroupedBackground
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
    }

    private var headerCard: some View {
        GlassPanel(
            title: sourceNudge == nil ? "Publish a nudge" : "Fork this nudge",
            subtitle: "Create a kind:4201 nudge with optional tool constraints."
        ) {
            HStack(spacing: 10) {
                statPill(label: "Title", value: title.trimmed.isEmpty ? "Pending" : title.trimmed)
                statPill(label: "Mode", value: toolMode == .additive ? "Additive" : "Exclusive")
                statPill(label: "Tools", value: "\(currentToolCount)")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var currentToolCount: Int {
        switch toolMode {
        case .additive:
            return allowTools.count + denyTools.count
        case .exclusive:
            return onlyTools.count
        }
    }

    private func statPill(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(value)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.55), in: Capsule())
    }

    private var stepRail: some View {
        HStack(spacing: 8) {
            ForEach(NewNudgeStep.allCases) { candidate in
                Button {
                    guard candidate.rawValue <= step.rawValue else { return }
                    step = candidate
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: candidate.symbol)
                            .font(.caption.weight(.semibold))
                        Text(candidate.title)
                            .font(.caption.weight(.semibold))
                    }
                    .foregroundStyle(step == candidate ? Color.agentBrand : .secondary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(step == candidate ? Color.agentBrand.opacity(0.18) : Color.systemGray6.opacity(0.6))
                    )
                }
                .buttonStyle(.plain)
                .disabled(isCreating)
            }
        }
    }

    @ViewBuilder
    private var stepContent: some View {
        switch step {
        case .basics:
            basicsStep
        case .content:
            contentStep
        case .tools:
            toolsStep
        case .review:
            reviewStep
        }
    }

    private var basicsStep: some View {
        GlassPanel(
            title: "Identity",
            subtitle: "Name and describe the nudge so teammates can discover it quickly."
        ) {
            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Title", help: "Required")
                    TextField("Write concise release notes with sources", text: $title)
                        .textFieldStyle(.roundedBorder)
                }

                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Description", help: "Optional")
                    TextField("A short summary of behavior", text: $description, axis: .vertical)
                        .lineLimit(2...5)
                        .textFieldStyle(.roundedBorder)
                }

                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Hashtags", help: "Comma-separated")
                    TextField("research, docs, planning", text: $hashtagsInput)
                        .textFieldStyle(.roundedBorder)
                }
            }
        }
    }

    private var contentStep: some View {
        GlassPanel(
            title: "Nudge Content",
            subtitle: "Define the full behavioral instruction in markdown."
        ) {
            TextEditor(text: $content)
                .font(.body.monospaced())
                .frame(minHeight: 280)
                .padding(10)
                .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            Text("\(content.count) characters")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var toolsStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(
                title: "Tool Permissions",
                subtitle: "Match TUI semantics: Additive (allow/deny) or Exclusive (only)."
            ) {
                VStack(alignment: .leading, spacing: 10) {
                    Picker("Tool Mode", selection: $toolMode) {
                        ForEach(NudgeToolMode.allCases) { mode in
                            Text(mode.title).tag(mode)
                        }
                    }
                    .pickerStyle(.segmented)

                    Text(toolMode.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    if toolMode == .additive, toolOverlapCount > 0 {
                        Text("\(toolOverlapCount) overlap(s) present. Deny wins on publish.")
                            .font(.caption)
                            .foregroundStyle(Color.askBrand)
                    }

                    TextField("Filter tools", text: $toolSearchText)
                        .textFieldStyle(.roundedBorder)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif
                }
            }

            GlassPanel(title: "Available Tools", subtitle: nil) {
                if filteredTools.isEmpty {
                    ContentUnavailableView(
                        "No Matching Tools",
                        systemImage: "magnifyingglass",
                        description: Text("Try adjusting your filter or refresh project status.")
                    )
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(filteredTools, id: \.self) { tool in
                            toolRow(tool)
                            Divider()
                                .opacity(0.24)
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.36))
                            .overlay(
                                RoundedRectangle(cornerRadius: 12, style: .continuous)
                                    .stroke(.white.opacity(reduceTransparency ? 0.06 : 0.14), lineWidth: 1)
                            )
                    )
                }
            }
        }
    }

    @ViewBuilder
    private func toolRow(_ tool: String) -> some View {
        HStack(spacing: 10) {
            Text(tool)
                .font(.body.monospaced())
                .lineLimit(1)

            Spacer(minLength: 0)

            switch toolMode {
            case .additive:
                permissionButton(
                    label: "Allow",
                    active: allowTools.contains(tool),
                    tint: Color.presenceOnline
                ) {
                    toggleTool(tool, in: &allowTools)
                }

                permissionButton(
                    label: "Deny",
                    active: denyTools.contains(tool),
                    tint: Color.askBrand
                ) {
                    toggleTool(tool, in: &denyTools)
                }
            case .exclusive:
                permissionButton(
                    label: "Only",
                    active: onlyTools.contains(tool),
                    tint: Color.agentBrand
                ) {
                    toggleTool(tool, in: &onlyTools)
                }
            }
        }
    }

    private func permissionButton(
        label: String,
        active: Bool,
        tint: Color,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Text(label)
                .font(.caption.weight(.semibold))
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .foregroundStyle(active ? .white : tint)
                .background(
                    Capsule()
                        .fill(active ? tint : tint.opacity(0.14))
                )
        }
        .buttonStyle(.plain)
    }

    private var reviewStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(
                title: "Review",
                subtitle: "Confirm fields and publish as a new nudge event."
            ) {
                VStack(alignment: .leading, spacing: 8) {
                    previewRow(title: "Title", value: title.trimmed)
                    previewRow(title: "Description", value: description.trimmed.isEmpty ? "(none)" : description.trimmed)
                    previewRow(title: "Hashtags", value: parsedHashtags.isEmpty ? "(none)" : parsedHashtags.map { "#\($0)" }.joined(separator: " "))
                    previewRow(title: "Tool Mode", value: toolMode == .additive ? "Additive" : "Exclusive")
                    previewRow(title: "Tools", value: toolsPreviewText)
                }
            }

            GlassPanel(title: "Content", subtitle: nil) {
                Group {
                    if content.trimmed.isEmpty {
                        Text("No nudge content provided.")
                            .foregroundStyle(.secondary)
                    } else {
                        MarkdownView(content: content)
                    }
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.systemGray6.opacity(0.6), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }

    private var toolsPreviewText: String {
        switch toolMode {
        case .additive:
            let allow = allowTools.sorted().joined(separator: ", ")
            let deny = denyTools.sorted().joined(separator: ", ")
            if allow.isEmpty, deny.isEmpty { return "(none)" }
            if allow.isEmpty { return "deny: \(deny)" }
            if deny.isEmpty { return "allow: \(allow)" }
            return "allow: \(allow) | deny: \(deny)"
        case .exclusive:
            let only = onlyTools.sorted().joined(separator: ", ")
            return only.isEmpty ? "(none)" : only
        }
    }

    private func previewRow(title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 92, alignment: .leading)
            Text(value)
                .font(.body)
                .lineLimit(3)
            Spacer(minLength: 0)
        }
    }

    private func fieldLabel(_ title: String, help: String) -> some View {
        HStack(spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
            Text(help)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private var actionBar: some View {
        HStack(spacing: 10) {
            if step != .basics {
                Button {
                    goBack()
                } label: {
                    Label("Back", systemImage: "chevron.left")
                        .frame(minWidth: 90)
                }
                .disabled(isCreating)
                .adaptiveGlassButtonStyle()
            }

            Spacer(minLength: 0)

            Button {
                handlePrimaryAction()
            } label: {
                if isCreating {
                    ProgressView()
                        .controlSize(.small)
                        .frame(minWidth: 110)
                } else {
                    Text(primaryActionTitle)
                        .frame(minWidth: 110)
                }
            }
            .disabled(isCreating || !canProceed)
            .adaptiveGlassButtonStyle()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.6))
                .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 8)
    }

    private var primaryActionTitle: String {
        step == .review ? "Create" : "Next"
    }

    private func goBack() {
        if let previous = NewNudgeStep(rawValue: step.rawValue - 1) {
            step = previous
        }
    }

    private func handlePrimaryAction() {
        if step == .review {
            submit()
            return
        }

        if let next = NewNudgeStep(rawValue: step.rawValue + 1) {
            step = next
        }
    }

    private func submit() {
        let trimmedTitle = title.trimmed
        let trimmedDescription = description.trimmed
        let trimmedContent = content.trimmed

        guard !trimmedTitle.isEmpty else {
            errorMessage = "Nudge title cannot be empty."
            step = .basics
            return
        }

        guard !trimmedContent.isEmpty else {
            errorMessage = "Nudge content cannot be empty."
            step = .content
            return
        }

        if toolMode == .exclusive, onlyTools.isEmpty {
            errorMessage = "Exclusive mode requires at least one tool."
            step = .tools
            return
        }

        let submission = NudgeDraftSubmission(
            title: trimmedTitle,
            description: trimmedDescription,
            content: trimmedContent,
            hashtags: parsedHashtags,
            allowTools: allowTools.sorted(),
            denyTools: denyTools.sorted(),
            onlyTools: onlyTools.sorted()
        )

        isCreating = true

        Task {
            let success = await onSubmit(submission)
            await MainActor.run {
                isCreating = false
                if success {
                    dismiss()
                } else {
                    errorMessage = "Failed to publish nudge."
                }
            }
        }
    }

    private func toggleTool(_ tool: String, in set: inout Set<String>) {
        if set.contains(tool) {
            set.remove(tool)
        } else {
            set.insert(tool)
        }
    }
}

private extension String {
    var trimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
