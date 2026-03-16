import Observation
import SwiftUI

struct SynthesisSettingsPanel: View {
    @Bindable var viewModel: SynthesisViewModel
    @State private var isShowingClearConfirmation = false

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            Text("Set persistent guidance for how Fawx should behave. You can also change this by asking Fawx directly in chat.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            editor

            HStack {
                Text("\(viewModel.currentLength) / \(viewModel.maxLength) characters")
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
                Button(viewModel.isSaving ? "Saving..." : "Save") {
                    Task {
                        await viewModel.save()
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!viewModel.canSave)

                Button("Clear", role: .destructive) {
                    isShowingClearConfirmation = true
                }
                .buttonStyle(.bordered)
                .disabled(!viewModel.canClear || viewModel.isSaving)
            }

            SetupStatusMessageView(
                kind: viewModel.statusKind,
                message: viewModel.statusMessage
            )
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
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
        TextEditor(text: $viewModel.text)
            .font(FawxTypography.code)
            .frame(minHeight: 180)
            .padding(FawxSpacing.paddingSM)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(viewModel.isOverLimit ? Color.fawxError : Color.fawxBorder, lineWidth: 1)
            }
            .accessibilityLabel("Custom instructions")
    }
}
