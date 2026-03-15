import Observation
import SwiftUI

struct ModelSelectionList: View {
    @Bindable var appState: AppState
    let disableSelection: Bool
    let selectModel: (String) -> Void

    var body: some View {
        ScrollView {
            LazyVStack(spacing: FawxSpacing.paddingSM) {
                if appState.availableModels.isEmpty {
                    unavailableState
                } else {
                    ForEach(appState.availableModels) { model in
                        Button {
                            guard !disableSelection else {
                                return
                            }
                            selectModel(model.modelID)
                        } label: {
                            ModelSelectionRow(
                                model: model,
                                isSelected: model.modelID == appState.activeModel?.modelID
                            )
                        }
                        .buttonStyle(.plain)
                        .disabled(disableSelection)
                    }
                }
            }
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground)
    }

    private var unavailableState: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("No models available")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text("Connect to a server and refresh settings to load the available models.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct ModelSelectionRow: View {
    let model: ModelInfo
    let isSelected: Bool

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text(abbreviateModelName(model.modelID))
                    .font(.system(size: 15, weight: .semibold, design: .monospaced))
                    .foregroundStyle(Color.fawxText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .multilineTextAlignment(.leading)
                    .lineLimit(2)

                Text(modelMetadataSummary(model))
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            if isSelected {
                Image(systemName: "checkmark")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.fawxAccent)
                    .padding(.top, 2)
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(isSelected ? Color.fawxAccent.opacity(0.08) : Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(isSelected ? Color.fawxAccent.opacity(0.35) : Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}
