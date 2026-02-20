import SwiftUI

// MARK: - Ask Answer View

/// Interactive view for answering an ask event.
/// Allows users to select answers for each question and submit them.
struct AskAnswerView: View {
    let askEvent: AskEvent
    let askEventId: String
    let askAuthorPubkey: String
    let conversationId: String
    let projectId: String
    let onSubmit: () -> Void

    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var currentQuestionIndex = 0
    @State private var answers: [Int: AnswerState] = [:]
    @State private var isSubmitting = false
    @State private var errorMessage: String?
    @State private var customTextInput = ""
    @State private var showCustomInput = false

    private var isComplete: Bool {
        answers.count == askEvent.questions.count
    }

    private var currentQuestion: AskQuestion? {
        guard currentQuestionIndex < askEvent.questions.count else { return nil }
        return askEvent.questions[currentQuestionIndex]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Header with title and progress
            headerSection

            // Context
            if !askEvent.context.isEmpty {
                Text(askEvent.context)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            Divider()

            // Current question
            if let question = currentQuestion {
                questionSection(question)
            }

            Spacer()

            // Navigation and submit
            bottomControls

            // Error message
            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.healthError)
            }
        }
        .padding(16)
        .background(Color.askBrandSubtleBackground)
        .clipShape(RoundedRectangle(cornerRadius: 16))
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(Color.askBrandBorder, lineWidth: 1)
        )
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "questionmark.circle.fill")
                    .foregroundStyle(Color.askBrand)
                    .font(.title2)

                if let title = askEvent.title {
                    Text(title)
                        .font(.headline)
                } else {
                    Text("Questions")
                        .font(.headline)
                }

                Spacer()

                // Progress indicator
                Text("\(currentQuestionIndex + 1) of \(askEvent.questions.count)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.askBrandBackground)
                    .clipShape(Capsule())
            }

            // Progress bar
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Rectangle()
                        .fill(Color.askBrandBackground)
                    Rectangle()
                        .fill(Color.askBrand)
                        .frame(width: geometry.size.width * CGFloat(answers.count) / CGFloat(askEvent.questions.count))
                }
            }
            .frame(height: 4)
            .clipShape(Capsule())
        }
    }

    // MARK: - Question Section

    @ViewBuilder
    private func questionSection(_ question: AskQuestion) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            // Question title
            Text(getTitle(question))
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(Color.askBrand)

            // Question text
            Text(getQuestionText(question))
                .font(.body)

            // Choices
            let choices = getChoices(question)
            let isMulti = isMultiSelect(question)

            VStack(spacing: 8) {
                ForEach(Array(choices.enumerated()), id: \.offset) { index, choice in
                    SelectableChoiceRow(
                        choice: choice,
                        isSelected: isChoiceSelected(index),
                        isMultiSelect: isMulti
                    ) {
                        toggleChoice(index, choice: choice, isMulti: isMulti)
                    }
                }

                // "Other" option for custom input
                SelectableChoiceRow(
                    choice: "Other (custom answer)",
                    isSelected: showCustomInput,
                    isMultiSelect: false
                ) {
                    showCustomInput.toggle()
                    if !showCustomInput {
                        customTextInput = ""
                    }
                }

                // Custom input field
                if showCustomInput {
                    TextField("Enter your answer...", text: $customTextInput, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(3...6)
                }
            }
        }
    }

    // MARK: - Bottom Controls

    private var bottomControls: some View {
        HStack {
            // Previous button
            if currentQuestionIndex > 0 {
                Button {
                    withAnimation {
                        currentQuestionIndex -= 1
                        loadAnswerState()
                    }
                } label: {
                    Label("Previous", systemImage: "chevron.left")
                }
                .buttonStyle(.bordered)
            }

            Spacer()

            // Next or Submit button
            if currentQuestionIndex < askEvent.questions.count - 1 {
                Button {
                    saveCurrentAnswer()
                    withAnimation {
                        currentQuestionIndex += 1
                        loadAnswerState()
                    }
                } label: {
                    Label("Next", systemImage: "chevron.right")
                }
                .buttonStyle(.borderedProminent)
                .tint(Color.askBrand)
                .disabled(!hasCurrentAnswer)
            } else {
                Button {
                    saveCurrentAnswer()
                    submitAnswers()
                } label: {
                    if isSubmitting {
                        ProgressView()
                            .progressViewStyle(CircularProgressViewStyle(tint: .white))
                    } else {
                        Label("Submit", systemImage: "paperplane.fill")
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(Color.statusActive)
                .disabled(!isComplete || isSubmitting)
            }
        }
    }

    // MARK: - Helper Methods

    private func getTitle(_ question: AskQuestion) -> String {
        switch question {
        case .singleSelect(let title, _, _): return title
        case .multiSelect(let title, _, _): return title
        }
    }

    private func getQuestionText(_ question: AskQuestion) -> String {
        switch question {
        case .singleSelect(_, let q, _): return q
        case .multiSelect(_, let q, _): return q
        }
    }

    private func getChoices(_ question: AskQuestion) -> [String] {
        switch question {
        case .singleSelect(_, _, let suggestions): return suggestions
        case .multiSelect(_, _, let options): return options
        }
    }

    private func isMultiSelect(_ question: AskQuestion) -> Bool {
        switch question {
        case .singleSelect: return false
        case .multiSelect: return true
        }
    }

    private var hasCurrentAnswer: Bool {
        if showCustomInput && !customTextInput.isEmpty {
            return true
        }
        guard let answer = answers[currentQuestionIndex] else { return false }
        switch answer {
        case .single(let value): return !value.isEmpty
        case .multi(let values): return !values.isEmpty
        case .custom(let value): return !value.isEmpty
        }
    }

    private func isChoiceSelected(_ index: Int) -> Bool {
        guard let answer = answers[currentQuestionIndex] else { return false }
        switch answer {
        case .single(let value):
            guard let question = currentQuestion else { return false }
            let choices = getChoices(question)
            return index < choices.count && choices[index] == value
        case .multi(let values):
            guard let question = currentQuestion else { return false }
            let choices = getChoices(question)
            return index < choices.count && values.contains(choices[index])
        case .custom:
            return false
        }
    }

    private func toggleChoice(_ index: Int, choice: String, isMulti: Bool) {
        showCustomInput = false
        customTextInput = ""

        if isMulti {
            var values: [String] = []
            if case .multi(let existing) = answers[currentQuestionIndex] {
                values = existing
            }

            if values.contains(choice) {
                values.removeAll { $0 == choice }
            } else {
                values.append(choice)
            }
            answers[currentQuestionIndex] = .multi(values)
        } else {
            answers[currentQuestionIndex] = .single(choice)
        }
    }

    private func saveCurrentAnswer() {
        if showCustomInput && !customTextInput.isEmpty {
            answers[currentQuestionIndex] = .custom(customTextInput)
        }
    }

    private func loadAnswerState() {
        showCustomInput = false
        customTextInput = ""

        if let answer = answers[currentQuestionIndex] {
            if case .custom(let value) = answer {
                showCustomInput = true
                customTextInput = value
            }
        }
    }

    private func submitAnswers() {
        guard isComplete else { return }

        isSubmitting = true
        errorMessage = nil

        // Convert answers to AskAnswer array
        var askAnswers: [AskAnswer] = []

        for (index, question) in askEvent.questions.enumerated() {
            guard let answer = answers[index] else { continue }

            let title = getTitle(question)
            let answerType: AskAnswerType

            switch answer {
            case .single(let value):
                answerType = .singleSelect(value: value)
            case .multi(let values):
                answerType = .multiSelect(values: values)
            case .custom(let value):
                answerType = .customText(value: value)
            }

            askAnswers.append(AskAnswer(questionTitle: title, answerType: answerType))
        }

        Task {
            do {
                _ = try await coreManager.safeCore.answerAsk(
                    askEventId: askEventId,
                    askAuthorPubkey: askAuthorPubkey,
                    conversationId: conversationId,
                    projectId: projectId,
                    answers: askAnswers
                )

                await MainActor.run {
                    isSubmitting = false
                    onSubmit()
                }
            } catch {
                await MainActor.run {
                    isSubmitting = false
                    errorMessage = "Failed to submit: \(error.localizedDescription)"
                }
            }
        }
    }
}

// MARK: - Answer State

private enum AnswerState {
    case single(String)
    case multi([String])
    case custom(String)
}

// MARK: - Selectable Choice Row

private struct SelectableChoiceRow: View {
    let choice: String
    let isSelected: Bool
    let isMultiSelect: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                // Selection indicator
                Image(systemName: isSelected ? (isMultiSelect ? "checkmark.square.fill" : "circle.fill") : (isMultiSelect ? "square" : "circle"))
                    .foregroundStyle(isSelected ? .orange : .secondary)
                    .font(.title3)

                // Choice text
                Text(choice)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.leading)

                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(isSelected ? Color.askBrandBackground : Color.systemGray6)
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.borderless)
    }
}

// MARK: - Preview

#Preview {
    AskAnswerView(
        askEvent: AskEvent(
            title: "Project Setup",
            context: "Please answer the following questions to help us set up your project.",
            questions: [
                .singleSelect(
                    title: "Language",
                    question: "What programming language would you like to use?",
                    suggestions: ["Swift", "Rust", "TypeScript", "Python"]
                ),
                .multiSelect(
                    title: "Features",
                    question: "Which features do you want to enable?",
                    options: ["Authentication", "Database", "API", "Testing"]
                )
            ]
        ),
        askEventId: "test-event-id",
        askAuthorPubkey: "test-pubkey",
        conversationId: "test-conversation",
        projectId: "test-project"
    ) {
    }
    .environmentObject(TenexCoreManager())
    .padding()
}
