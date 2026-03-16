import Observation
import SwiftUI

struct TelemetrySettingsPanel: View {
    @Bindable var viewModel: TelemetryViewModel

    private let neverCollectItems = [
        "Conversations",
        "Files",
        "API Keys",
        "IP Addresses",
        "Device IDs"
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            Text("Telemetry is off by default. If you opt in, Fawx only shares anonymous product signals you choose.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            masterToggleRow

            if viewModel.isLoading && viewModel.categories.isEmpty && viewModel.canManageTelemetry {
                ProgressView("Loading telemetry preferences...")
                    .frame(maxWidth: .infinity, minHeight: 120)
            } else if !viewModel.canManageTelemetry {
                Text("Connect to a server to manage privacy and telemetry settings.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else if viewModel.categories.isEmpty {
                Text("No telemetry categories were provided by the server.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                categoriesSection
            }

            neverCollectSection

            SetupStatusMessageView(
                kind: viewModel.errorMessage == nil ? .idle : .failure,
                message: viewModel.errorMessage
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
    }

    private var masterToggleRow: some View {
        HStack(alignment: .center, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Share anonymous usage data")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Text("Choose which anonymous product signals you want Fawx to send.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Spacer(minLength: 0)

            Toggle(
                "",
                isOn: Binding(
                    get: { viewModel.isEnabled },
                    set: { enabled in
                        Task {
                            await viewModel.setEnabled(enabled)
                        }
                    }
                )
            )
            .labelsHidden()
            .disabled(
                viewModel.isLoading
                    || viewModel.isUpdatingMaster
                    || !viewModel.pendingCategories.isEmpty
                    || !viewModel.canManageTelemetry
            )
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private var categoriesSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Anonymous Signal Categories")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            if !viewModel.isEnabled {
                Text("Turn on anonymous usage sharing to enable individual categories.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            VStack(spacing: FawxSpacing.paddingSM) {
                ForEach(viewModel.categories) { category in
                    categoryRow(category)
                }
            }
            .opacity(viewModel.isEnabled ? 1 : 0.6)
        }
    }

    private func categoryRow(_ category: TelemetryCategory) -> some View {
        let isPending = viewModel.pendingCategories.contains(category.name)

        return HStack(alignment: .center, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 4) {
                Text(category.title)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text(category.description)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 0)

            if isPending {
                ProgressView()
                    .controlSize(.small)
            }

            Toggle(
                "",
                isOn: Binding(
                    get: { category.enabled },
                    set: { enabled in
                        Task {
                            await viewModel.setCategoryEnabled(category.name, enabled: enabled)
                        }
                    }
                )
            )
            .labelsHidden()
            .disabled(
                !viewModel.isEnabled
                    || viewModel.isLoading
                    || viewModel.isUpdatingMaster
                    || isPending
                    || !viewModel.canManageTelemetry
            )
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private var neverCollectSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("What We Never Collect")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 150), spacing: FawxSpacing.paddingSM)],
                alignment: .leading,
                spacing: FawxSpacing.paddingSM
            ) {
                ForEach(neverCollectItems, id: \.self) { item in
                    HStack(spacing: FawxSpacing.paddingSM) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(Color.fawxError)

                        Text(item)
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxText)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingMD)
                    .background(Color.fawxBackground)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }
}
