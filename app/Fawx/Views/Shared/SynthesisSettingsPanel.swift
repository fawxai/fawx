import Observation
import SwiftUI
#if os(macOS)
import AppKit
#endif

struct SynthesisSettingsPanel: View {
    @Bindable var viewModel: SynthesisViewModel
    @State private var isShowingClearConfirmation = false
#if os(macOS)
    @Environment(\.fawxAccentInvalidationToken) private var accentInvalidationToken
#endif

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            Text("Set persistent guidance for how Fawx should behave. You can also change this by asking Fawx directly in chat.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            personalityPicker

            editor

            HStack {
                Text(characterLimitText)
                    .font(FawxTypography.status)
                    .foregroundStyle(viewModel.isOverLimit ? Color.fawxError : Color.fawxTextSecondary)

                Spacer(minLength: 0)

                if let updatedAt = viewModel.updatedAt {
                    Text("Updated \(relativeTimestampString(updatedAt))")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                SettingsActionButton(
                    title: viewModel.isSaving ? "Saving..." : "Save",
                    role: .primary,
                    isDisabled: !viewModel.canSave,
                    accentColor: accentColor,
                    accentTextColor: accentTextColor
                ) {
                    Task {
                        await viewModel.save()
                    }
                }

                SettingsActionButton(
                    title: "Clear",
                    role: .secondary,
                    isDisabled: !viewModel.canClear || viewModel.isSaving,
                    accentColor: accentColor,
                    accentTextColor: accentTextColor
                ) {
                    isShowingClearConfirmation = true
                }
            }

            SetupStatusMessageView(
                kind: viewModel.statusKind,
                message: viewModel.statusMessage
            )
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .fawxSurface(.section)
        .task {
            await viewModel.refresh()
        }
        .confirmationDialog(
            "Clear custom instructions?",
            isPresented: $isShowingClearConfirmation,
            titleVisibility: .visible
        ) {
            Button("Clear Instructions", role: .destructive) {
                Task {
                    await viewModel.clear()
                }
            }

            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This removes the saved instructions used across conversations.")
        }
    }

    private var editor: some View {
        editorContent
            .frame(minHeight: 180)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(viewModel.isOverLimit ? Color.fawxError : Color.fawxBorder, lineWidth: 1)
            }
            .accessibilityLabel("Custom instructions")
    }

    @ViewBuilder
    private var editorContent: some View {
#if os(macOS)
        SettingsInstructionsTextEditor(
            text: Binding(
                get: { viewModel.text },
                set: { viewModel.updateText($0) }
            ),
            textColor: NSColor(Color.fawxText),
            insertionPointColor: .fawxTextInsertionPoint,
            font: .monospacedSystemFont(ofSize: FawxTypography.chatBodyPointSize - 1, weight: .regular)
        )
#else
        TextEditor(
            text: Binding(
                get: { viewModel.text },
                set: { viewModel.updateText($0) }
            )
        )
            .font(FawxTypography.code)
            .padding(FawxSpacing.paddingSM)
#endif
    }

    private var personalityPicker: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Personality")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            PersonalitySelectionControl(
                choices: viewModel.personalityChoices,
                selection: $viewModel.personalityID,
                accentColor: accentColor
            )

            Text(viewModel.selectedPersonalityDescription)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingMD)
        .fawxSurface(.field)
    }

    private var accentColor: Color {
#if os(macOS)
        _ = accentInvalidationToken
#endif
        return Color.fawxAccent
    }

    private var accentTextColor: Color {
        Color.fawxAccentText
    }

    private var characterLimitText: String {
        if viewModel.remainingLength >= 0 {
            return "\(viewModel.currentLength) / \(viewModel.maxLength) characters - \(viewModel.remainingLength) remaining"
        }
        return "\(viewModel.currentLength) / \(viewModel.maxLength) characters"
    }
}

private struct PersonalitySelectionControl: View {
    let choices: [AgentPersonalityChoice]
    @Binding var selection: String
    let accentColor: Color

    var body: some View {
        HStack(spacing: 0) {
            ForEach(choices) { choice in
                PersonalitySelectionSegment(
                    choice: choice,
                    isSelected: selection == choice.id,
                    accentColor: accentColor
                ) {
                    selection = choice.id
                }

                if choice.id != choices.last?.id {
                    Divider()
                        .frame(height: 18)
                        .overlay(Color.fawxText.opacity(0.16))
                }
            }
        }
        .padding(2)
        .background(Color.fawxText.opacity(0.04))
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM)
                .stroke(Color.fawxText.opacity(0.14), lineWidth: 1)
        }
        .accessibilityLabel("Personality")
        .accessibilityIdentifier("personalitySelectionControl")
    }
}

private struct PersonalitySelectionSegment: View {
    let choice: AgentPersonalityChoice
    let isSelected: Bool
    let accentColor: Color
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            Text(choice.title)
                .font(FawxTypography.status)
                .fontWeight(.semibold)
                .foregroundStyle(isSelected ? accentColor : Color.fawxTextSecondary)
                .frame(maxWidth: .infinity)
                .padding(.vertical, FawxSpacing.paddingXS)
                .padding(.horizontal, FawxSpacing.paddingSM)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM - 2)
                .fill(isSelected ? accentColor.opacity(0.14) : Color.clear)
        )
        .accessibilityIdentifier("personality\(choice.title)Button")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

private struct SettingsActionButton: View {
    enum Role {
        case primary
        case secondary
    }

    let title: String
    let role: Role
    let isDisabled: Bool
    let accentColor: Color
    let accentTextColor: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(FawxTypography.status)
                .fontWeight(.semibold)
                .foregroundStyle(foregroundColor)
                .padding(.horizontal, FawxSpacing.paddingMD)
                .padding(.vertical, FawxSpacing.paddingXS)
                .background(background)
                .overlay(border)
                .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM))
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .opacity(isDisabled ? 0.45 : 1)
        .accessibilityIdentifier("\(title)CustomPreferencesButton")
    }

    private var foregroundColor: Color {
        switch role {
        case .primary:
            return accentTextColor
        case .secondary:
            return Color.fawxText
        }
    }

    private var background: some View {
        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM)
            .fill(role == .primary ? accentColor : Color.fawxSurfaceActive)
    }

    private var border: some View {
        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM)
            .stroke(role == .primary ? accentColor.opacity(0.7) : Color.fawxBorder, lineWidth: 1)
    }
}

#if os(macOS)
private struct SettingsInstructionsTextEditor: NSViewRepresentable {
    @Binding var text: String

    let textColor: NSColor
    let insertionPointColor: NSColor
    let font: NSFont

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay

        let textView = NSTextView()
        textView.delegate = context.coordinator
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.isRichText = false
        textView.importsGraphics = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticDataDetectionEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isGrammarCheckingEnabled = false
        textView.allowsUndo = true
        textView.textContainerInset = NSSize(width: FawxSpacing.paddingSM, height: FawxSpacing.paddingSM)
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.string = text
        applyChrome(to: textView)

        scrollView.documentView = textView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? NSTextView else {
            return
        }

        if textView.string != text {
            textView.string = text
        }
        applyChrome(to: textView)
    }

    private func applyChrome(to textView: NSTextView) {
        textView.textColor = textColor
        textView.insertionPointColor = insertionPointColor
        textView.font = font
        textView.applyFawxTextSelectionChrome()
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        @Binding private var text: String

        init(text: Binding<String>) {
            _text = text
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else {
                return
            }
            text = textView.string
        }
    }
}
#endif
