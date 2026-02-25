import SwiftUI

private enum NewAgentDefinitionStep: Int, CaseIterable, Identifiable {
    case basics
    case prompt
    case preview

    var id: Int { rawValue }

    var title: String {
        switch self {
        case .basics: return "Basics"
        case .prompt: return "Prompt"
        case .preview: return "Review"
        }
    }

    var subtitle: String {
        switch self {
        case .basics: return "Identity and metadata"
        case .prompt: return "Write and refine instructions"
        case .preview: return "Final check before publish"
        }
    }

    var symbol: String {
        switch self {
        case .basics: return "person.text.rectangle"
        case .prompt: return "text.alignleft"
        case .preview: return "checklist"
        }
    }
}

private struct PromptTransformOption: Identifiable {
    let id = UUID()
    let title: String
    let instruction: String
    let symbol: String
}

struct NewAgentDefinitionSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    var onCreated: (() -> Void)?

    @State private var step: NewAgentDefinitionStep = .basics

    @State private var name = ""
    @State private var description = ""
    @State private var role = "assistant"
    @State private var version = "1"
    @State private var instructions = ""

    @State private var isCreating = false
    @State private var showPromptAssistant = false
    @State private var promptAssistantSeedInstruction: String?
    @State private var errorMessage: String?

    private let quickPromptActions: [PromptTransformOption] = [
        PromptTransformOption(
            title: "Tighten",
            instruction: "Make this prompt concise while preserving all critical constraints and output requirements.",
            symbol: "scissors"
        ),
        PromptTransformOption(
            title: "Safer",
            instruction: "Strengthen safety boundaries and refusal behavior while keeping the same task scope.",
            symbol: "checkmark.shield"
        ),
        PromptTransformOption(
            title: "Structured",
            instruction: "Reformat into clear sections: role, objectives, constraints, workflow, and output format.",
            symbol: "square.split.2x2"
        ),
        PromptTransformOption(
            title: "More Helpful",
            instruction: "Improve helpfulness and coaching tone without becoming verbose. Keep decisive, actionable guidance.",
            symbol: "sparkles"
        )
    ]

    private var canProceed: Bool {
        switch step {
        case .basics:
            return !name.tenexTrimmed.isEmpty && !description.tenexTrimmed.isEmpty
        case .prompt:
            return !instructions.tenexTrimmed.isEmpty
        case .preview:
            return true
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    heroCard
                    stepRail
                    stepContent
                }
                .padding(.horizontal, 18)
                .padding(.top, 16)
                .padding(.bottom, 84)
            }
            .background(backgroundView)
            .navigationTitle("New Agent Definition")
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
        .tenexModalPresentation(detents: [.large])
        #if os(macOS)
        .frame(minWidth: 760, idealWidth: 860, minHeight: 640, idealHeight: 740)
        #endif
        .sheet(isPresented: $showPromptAssistant) {
            AIAssistedPromptRewriteSheet(
                currentPrompt: instructions,
                initialInstruction: promptAssistantSeedInstruction,
                onApply: { rewrittenPrompt in
                    instructions = rewrittenPrompt
                }
            )
            .environment(coreManager)
            #if os(macOS)
            .frame(minWidth: 700, idealWidth: 820, minHeight: 560, idealHeight: 680)
            #endif
        }
        .alert(
            "Unable to Create Agent",
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

    private var heroCard: some View {
        GlassPanel(
            title: "Publish a reusable agent",
            subtitle: "Create a kind:4199 definition with a polished prompt and versioned metadata."
        ) {
            HStack(spacing: 10) {
                statPill(label: "Name", value: name.tenexTrimmed.isEmpty ? "Pending" : name.tenexTrimmed)
                statPill(label: "Role", value: role.tenexTrimmed.isEmpty ? "assistant" : role.tenexTrimmed)
                statPill(label: "Version", value: version.tenexTrimmed.isEmpty ? "1" : version.tenexTrimmed)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
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
            ForEach(NewAgentDefinitionStep.allCases) { candidate in
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
        case .prompt:
            promptStep
        case .preview:
            previewStep
        }
    }

    private var basicsStep: some View {
        GlassPanel(
            title: "Identity",
            subtitle: "Use a clear name and one-line purpose so this definition is discoverable."
        ) {
            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Name", help: "Required")
                    TextField("Research Copilot", text: $name)
                        .textFieldStyle(.roundedBorder)
                }

                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Description", help: "Required")
                    TextField("Summarizes source material and proposes next actions", text: $description, axis: .vertical)
                        .lineLimit(2...5)
                        .textFieldStyle(.roundedBorder)
                }

                HStack(alignment: .top, spacing: 10) {
                    VStack(alignment: .leading, spacing: 5) {
                        fieldLabel("Role", help: "Optional")
                        TextField("assistant", text: $role)
                            .textFieldStyle(.roundedBorder)
                    }

                    VStack(alignment: .leading, spacing: 5) {
                        fieldLabel("Version", help: "Integer")
                        TextField("1", text: $version)
                            .textFieldStyle(.roundedBorder)
                            #if os(iOS)
                            .keyboardType(.numberPad)
                            #endif
                    }
                    .frame(width: 120)
                }
            }
        }
    }

    private var promptStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(
                title: "System Prompt",
                subtitle: "Define behavior, boundaries, and output structure in markdown."
            ) {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Menu {
                            Button("General assistant") {
                                instructions = PromptTemplates.generalAssistant
                            }
                            Button("Research analyst") {
                                instructions = PromptTemplates.researchAnalyst
                            }
                            Button("Code reviewer") {
                                instructions = PromptTemplates.codeReviewer
                            }
                        } label: {
                            Label("Insert Template", systemImage: "text.badge.plus")
                        }

                        Spacer()

                        Button {
                            promptAssistantSeedInstruction = nil
                            showPromptAssistant = true
                        } label: {
                            Label("AI Polish", systemImage: "sparkles")
                        }
                        .adaptiveGlassButtonStyle()
                    }

                    TextEditor(text: $instructions)
                        .font(.body.monospaced())
                        .frame(minHeight: 250)
                        .padding(8)
                        .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
            }

            GlassPanel(
                title: "Quick AI Transforms",
                subtitle: "Apply a focused rewrite mode, then fine-tune before applying."
            ) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(quickPromptActions) { action in
                            Button {
                                promptAssistantSeedInstruction = action.instruction
                                showPromptAssistant = true
                            } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: action.symbol)
                                    Text(action.title)
                                }
                                .font(.caption.weight(.semibold))
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.62), in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }

                Text("\(instructions.count) characters")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var previewStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(
                title: "Definition Preview",
                subtitle: "This is how collaborators will evaluate your agent before adding it."
            ) {
                VStack(alignment: .leading, spacing: 8) {
                    previewRow(title: "Name", value: name)
                    previewRow(title: "Role", value: role)
                    previewRow(title: "Version", value: version)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Description")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(description.tenexTrimmed.isEmpty ? "No description provided" : description)
                            .foregroundStyle(description.tenexTrimmed.isEmpty ? .secondary : .primary)
                    }
                }
            }

            GlassPanel(title: "Prompt Render", subtitle: nil) {
                Group {
                    if instructions.tenexTrimmed.isEmpty {
                        Text("No system prompt provided.")
                            .foregroundStyle(.secondary)
                    } else {
                        MarkdownView(content: instructions)
                    }
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.systemGray6.opacity(0.6), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
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

    private func fieldLabel(_ title: String, help: String) -> some View {
        HStack(spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
            Text(help)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private var primaryActionTitle: String {
        switch step {
        case .preview:
            return "Create"
        default:
            return "Next"
        }
    }

    private func goBack() {
        switch step {
        case .prompt:
            step = .basics
        case .preview:
            step = .prompt
        case .basics:
            break
        }
    }

    private func handlePrimaryAction() {
        switch step {
        case .basics:
            step = .prompt
        case .prompt:
            step = .preview
        case .preview:
            createAgentDefinition()
        }
    }

    private func createAgentDefinition() {
        let trimmedName = name.tenexTrimmed
        let trimmedDescription = description.tenexTrimmed
        let trimmedRole = role.tenexTrimmed
        let trimmedInstructions = instructions.tenexTrimmed
        let parsedVersion = Int(version.tenexTrimmed) ?? 1

        guard !trimmedName.isEmpty else {
            step = .basics
            return
        }
        guard !trimmedDescription.isEmpty else {
            step = .basics
            return
        }
        guard !trimmedInstructions.isEmpty else {
            step = .prompt
            return
        }

        isCreating = true

        Task {
            do {
                try await coreManager.safeCore.createAgentDefinition(
                    name: trimmedName,
                    description: trimmedDescription,
                    role: trimmedRole.isEmpty ? "assistant" : trimmedRole,
                    instructions: trimmedInstructions,
                    version: String(max(parsedVersion, 1)),
                    sourceId: nil,
                    isFork: false
                )

                await MainActor.run {
                    isCreating = false
                    onCreated?()
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    isCreating = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }

    private func previewRow(title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 72, alignment: .leading)
            Text(value.tenexTrimmed.isEmpty ? "Not set" : value)
                .font(.body)
                .foregroundStyle(value.tenexTrimmed.isEmpty ? .secondary : .primary)
            Spacer(minLength: 0)
        }
    }
}

private enum PromptRewriteStyle: String, CaseIterable, Identifiable {
    case tighten
    case expand
    case structure
    case safety
    case tone

    var id: String { rawValue }

    var title: String {
        switch self {
        case .tighten: return "Tighten"
        case .expand: return "Expand"
        case .structure: return "Structure"
        case .safety: return "Safety"
        case .tone: return "Tone"
        }
    }

    var subtitle: String {
        switch self {
        case .tighten: return "Shorter and sharper"
        case .expand: return "Add depth and detail"
        case .structure: return "Organized sections"
        case .safety: return "Stronger guardrails"
        case .tone: return "Adjust voice and style"
        }
    }

    var symbol: String {
        switch self {
        case .tighten: return "scissors"
        case .expand: return "arrow.up.left.and.arrow.down.right"
        case .structure: return "square.grid.3x2"
        case .safety: return "checkmark.shield"
        case .tone: return "quote.bubble"
        }
    }

    var rewriteInstruction: String {
        switch self {
        case .tighten:
            return "Rewrite to be concise and direct. Remove redundancy while preserving all constraints, role instructions, and output format requirements."
        case .expand:
            return "Rewrite with more explicit detail, examples, and operational guidance while maintaining the same goals and boundaries."
        case .structure:
            return "Rewrite into explicit markdown sections for role, objectives, constraints, workflow, and output contract."
        case .safety:
            return "Rewrite to strengthen safety boundaries, refusal conditions, and uncertainty handling while keeping useful guidance."
        case .tone:
            return "Rewrite to improve readability and tone while preserving intent, constraints, and expected outputs."
        }
    }
}

private struct AIAssistedPromptRewriteSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    let currentPrompt: String
    let initialInstruction: String?
    let onApply: (String) -> Void

    @State private var rewriteInstruction = ""
    @State private var generatedPrompt = ""
    @State private var model = "openai/gpt-4o-mini"
    @State private var selectedStyle: PromptRewriteStyle = .structure
    @State private var isGenerating = false
    @State private var showPreview = false
    @State private var hasAppliedInitialInstruction = false
    @State private var errorMessage: String?

    private let suggestions = [
        "Keep imperative verbs and remove vague language.",
        "Make success criteria explicit and measurable.",
        "Add stronger guidance for citing sources and unknowns.",
        "Prioritize concise bullet-based outputs."
    ]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    GlassPanel(
                        title: "AI Prompt Studio",
                        subtitle: "Use rewrite modes and custom direction, then apply only when the output is right."
                    ) {
                        HStack(spacing: 8) {
                            Text("Model")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            TextField("Model", text: $model)
                                .textFieldStyle(.roundedBorder)
                                .autocorrectionDisabled()
                                #if os(iOS)
                                .textInputAutocapitalization(.never)
                                #endif
                        }
                    }

                    if showPreview {
                        previewContent
                    } else {
                        composeContent
                    }
                }
                .padding(.horizontal, 18)
                .padding(.top, 16)
                .padding(.bottom, 90)
            }
            .background(
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
            )
            .navigationTitle("AI Prompt Editor")
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
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            rewriteActionBar
        }
        .onAppear {
            loadPreferredModel()
            applyInitialInstructionIfNeeded()
        }
        .alert(
            "Unable to Generate Prompt",
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
    }

    private var composeContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(title: "Rewrite Mode", subtitle: "Choose one primary strategy") {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(PromptRewriteStyle.allCases) { style in
                            Button {
                                selectedStyle = style
                            } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack(spacing: 6) {
                                        Image(systemName: style.symbol)
                                        Text(style.title)
                                    }
                                    .font(.caption.weight(.semibold))
                                    Text(style.subtitle)
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(
                                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                                        .fill(selectedStyle == style ? Color.agentBrand.opacity(0.2) : Color.systemBackground.opacity(reduceTransparency ? 1 : 0.62))
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

            GlassPanel(title: "Custom Direction", subtitle: "Optional constraints or style details") {
                TextEditor(text: $rewriteInstruction)
                    .frame(minHeight: 120)
                    .padding(8)
                    .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 10, style: .continuous))

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 8)], spacing: 8) {
                    ForEach(suggestions, id: \.self) { suggestion in
                        Button(suggestion) {
                            rewriteInstruction = suggestion
                        }
                        .adaptiveGlassButtonStyle()
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }

            GlassPanel(title: "Current Prompt", subtitle: "Reference context sent to the model") {
                Text(currentPrompt.tenexTrimmed.isEmpty ? "No prompt yet. The rewrite will generate from scratch." : currentPrompt)
                    .font(.caption.monospaced())
                    .lineLimit(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(Color.systemGray6.opacity(0.7), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
        }
    }

    private var previewContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(title: "Proposed Prompt", subtitle: "Review before replacing") {
                Text(generatedPrompt)
                    .font(.body.monospaced())
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(Color.systemGray6.opacity(0.7), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }

            GlassPanel(title: "Applied Rewrite Strategy", subtitle: selectedStyle.subtitle) {
                Text(composedInstruction)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var rewriteActionBar: some View {
        HStack(spacing: 10) {
            if showPreview {
                Button {
                    showPreview = false
                } label: {
                    Label("Edit", systemImage: "chevron.left")
                }
                .adaptiveGlassButtonStyle()
                .disabled(isGenerating)
            }

            Spacer(minLength: 0)

            Button {
                if showPreview {
                    onApply(generatedPrompt)
                    dismiss()
                } else {
                    generate()
                }
            } label: {
                if isGenerating {
                    ProgressView()
                        .controlSize(.small)
                        .frame(minWidth: 120)
                } else {
                    Text(showPreview ? "Apply" : "Generate")
                        .frame(minWidth: 120)
                }
            }
            .disabled(isGenerating || (showPreview && generatedPrompt.tenexTrimmed.isEmpty))
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

    private var composedInstruction: String {
        let custom = rewriteInstruction.tenexTrimmed
        if custom.isEmpty {
            return selectedStyle.rewriteInstruction
        }

        return """
        \(selectedStyle.rewriteInstruction)

        Additional user direction:
        \(custom)
        """
    }

    private func applyInitialInstructionIfNeeded() {
        guard !hasAppliedInitialInstruction else { return }
        hasAppliedInitialInstruction = true

        if let initialInstruction, !initialInstruction.tenexTrimmed.isEmpty {
            rewriteInstruction = initialInstruction
        }
    }

    private func loadPreferredModel() {
        Task {
            guard let settings = try? await coreManager.safeCore.getAiAudioSettings() else {
                return
            }
            guard let configuredModel = OpenRouterModelSelectionCodec.preferredModel(from: settings.openrouterModel) else {
                return
            }

            await MainActor.run {
                model = configuredModel
            }
        }
    }

    private func generate() {
        let chosenModel = model.tenexTrimmed.isEmpty ? "openai/gpt-4o-mini" : model.tenexTrimmed

        isGenerating = true
        errorMessage = nil

        Task {
            let keyResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()
            guard case .success(let apiKey) = keyResult else {
                await MainActor.run {
                    isGenerating = false
                    errorMessage = "OpenRouter API key not found. Add it in Settings > AI."
                }
                return
            }

            do {
                let rewritten = try await OpenRouterPromptRewriteService.rewritePrompt(
                    currentPrompt: currentPrompt,
                    rewriteInstruction: composedInstruction,
                    apiKey: apiKey,
                    model: chosenModel
                )

                await MainActor.run {
                    isGenerating = false
                    generatedPrompt = rewritten
                    showPreview = true
                }
            } catch {
                await MainActor.run {
                    isGenerating = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }
}

private enum PromptTemplates {
    static let generalAssistant = """
    You are a reliable AI assistant.

    ## Objectives
    - Understand the user's intent before answering.
    - Provide direct, actionable guidance.

    ## Constraints
    - Never fabricate facts.
    - If uncertain, say what is unknown and what to verify.

    ## Response Style
    - Start with a short answer, then details.
    - Use concise bullets for multi-step guidance.
    """

    static let researchAnalyst = """
    You are a research analyst focused on source-grounded synthesis.

    ## Workflow
    1. Extract key claims and evidence.
    2. Identify gaps, contradictions, or unknowns.
    3. Produce a prioritized recommendation list.

    ## Constraints
    - Distinguish fact, inference, and assumption.
    - Cite source context when possible.

    ## Output Contract
    - Summary (3-5 bullets)
    - Evidence and confidence
    - Next actions
    """

    static let codeReviewer = """
    You are a senior code reviewer.

    ## Objectives
    - Find correctness issues first, then maintainability risks.
    - Propose minimal, testable changes.

    ## Review Heuristics
    - Validate edge cases and error handling.
    - Flag potential regressions and missing tests.

    ## Output Contract
    - Findings ordered by severity.
    - Exact files and lines when available.
    - Optional patch strategy.
    """
}

private extension String {
    var tenexTrimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
