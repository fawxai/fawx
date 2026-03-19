import Observation
import SwiftUI

struct SetupWizardView: View {
    @Bindable var viewModel: SetupViewModel
    @Bindable var appState: AppState

    var body: some View {
        ZStack {
            Color.fawxBackground.ignoresSafeArea()

            VStack(spacing: FawxSpacing.paddingLG) {
                if viewModel.step != .welcome {
                    SetupWizardProgressView(step: viewModel.step)
                }

                currentStepView
            }
            .padding(FawxSpacing.paddingXL)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .task(id: viewModel.refreshKey) {
            await viewModel.prepareCurrentStep()
        }
    }

    @ViewBuilder
    private var currentStepView: some View {
        switch viewModel.step {
        case .welcome:
            WelcomeStep(viewModel: viewModel, appState: appState)
        case .tailscale:
            TailscaleStep(viewModel: viewModel)
        case .provider:
            ProviderStep(viewModel: viewModel)
        case .ready:
            ReadyStep(viewModel: viewModel, appState: appState)
        }
    }
}

struct SetupWizardCard<Content: View>: View {
    let maxWidth: CGFloat
    let content: Content

    init(
        maxWidth: CGFloat = 500,
        @ViewBuilder content: () -> Content
    ) {
        self.maxWidth = maxWidth
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            content
        }
        .frame(maxWidth: maxWidth, alignment: .leading)
        .padding(FawxSpacing.paddingXL)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: 18))
        .overlay {
            RoundedRectangle(cornerRadius: 18)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(0.05), radius: 18, y: 10)
    }
}

struct SetupWizardHeader: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text(detail)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }
}

struct SetupWizardProgressView: View {
    let step: SetupStep

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            ForEach(SetupStep.allCases.filter { $0 != .welcome }) { candidate in
                Capsule()
                    .fill(candidate.rawValue <= step.rawValue ? Color.fawxSuccess : Color.fawxSurfaceHover)
                    .frame(width: 68, height: 4)
            }
        }
        .padding(.bottom, FawxSpacing.paddingSM)
    }
}

struct SetupStatusMessageView: View {
    let kind: ConnectionTestKind
    let message: String?

    var body: some View {
        if let message, !message.isEmpty {
            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(color)
        }
    }

    private var color: Color {
        switch kind {
        case .idle:
            .fawxTextSecondary
        case .success:
            .fawxSuccess
        case .warning:
            .fawxWarning
        case .failure:
            .fawxError
        }
    }
}

struct SetupChoiceCard: View {
    let isSelected: Bool
    let iconText: String
    let title: String
    let subtitle: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: FawxSpacing.paddingMD) {
                ZStack {
                    Circle()
                        .fill(isSelected ? Color.fawxAccentSubtle : Color.fawxSurfaceHover)
                        .frame(width: 40, height: 40)

                    Text(iconText)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(isSelected ? Color.fawxAccent : Color.fawxTextSecondary)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    Text(subtitle)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)

                if isSelected {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(Color.fawxAccent)
                }
            }
            .padding(FawxSpacing.paddingMD)
            .background(isSelected ? Color.fawxAccentSubtle : Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isSelected ? Color.fawxAccent.opacity(0.35) : Color.fawxBorder, lineWidth: 1)
            }
        }
        .buttonStyle(.plain)
    }
}

struct SetupRadioRow: View {
    let isSelected: Bool
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: FawxSpacing.paddingMD) {
                ZStack {
                    Circle()
                        .stroke(isSelected ? Color.fawxAccent : Color.fawxBorder, lineWidth: 1.5)
                        .frame(width: 18, height: 18)

                    if isSelected {
                        Circle()
                            .fill(Color.fawxAccent)
                            .frame(width: 8, height: 8)
                    }
                }

                Text(title)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: 0)
            }
            .padding(.vertical, FawxSpacing.paddingSM)
        }
        .buttonStyle(.plain)
    }
}

struct SetupChecklistRow: View {
    let title: String
    let isComplete: Bool

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: isComplete ? "checkmark.circle.fill" : "circle")
                .foregroundStyle(isComplete ? Color.fawxSuccess : Color.fawxTextSecondary)

            Text(title)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
    }
}

struct SetupTransportBadge: View {
    let transport: String

    var body: some View {
        Text(label)
            .font(FawxTypography.status)
            .foregroundStyle(foregroundColor)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 4)
            .background(backgroundColor)
            .clipShape(Capsule())
    }

    private var label: String {
        switch transport {
        case "tailscale_https":
            "Tailscale HTTPS"
        case "lan_http":
            "Local Network"
        default:
            transport.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    private var foregroundColor: Color {
        transport == "tailscale_https" ? .fawxSuccess : .fawxWarning
    }

    private var backgroundColor: Color {
        transport == "tailscale_https" ? Color.fawxSuccess.opacity(0.12) : Color.fawxWarning.opacity(0.12)
    }
}
