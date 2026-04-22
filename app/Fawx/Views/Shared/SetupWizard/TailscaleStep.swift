import Observation
import SwiftUI

struct TailscaleStep: View {
    @Bindable var viewModel: SetupViewModel
    @Environment(\.openURL) private var openURL

    var body: some View {
        SetupWizardCard {
            SetupWizardHeader(
                title: "Secure Connectivity",
                detail: "Fawx uses Tailscale to securely connect your devices."
            )

            VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                SetupChecklistRow(
                    title: "Tailscale is installed",
                    isComplete: viewModel.tailscaleStatus?.installed == true
                )
                SetupChecklistRow(
                    title: "Tailscale is running and signed in",
                    isComplete: viewModel.tailscaleStatus?.running == true && viewModel.tailscaleStatus?.loggedIn == true
                )
                SetupChecklistRow(
                    title: "HTTPS certificate configured",
                    isComplete: viewModel.tailscaleStatus?.certReady == true
                )
            }

            progressBar

            if viewModel.tailscaleStatus?.installed == false {
                Button("Download Tailscale") {
                    if let url = URL(string: "https://tailscale.com/download") {
                        openURL(url)
                    }
                }
                .buttonStyle(.bordered)
            }

            if viewModel.tailscaleStatus?.installed == true,
               viewModel.tailscaleStatus?.running != true || viewModel.tailscaleStatus?.loggedIn != true {
                Text("Run `tailscale login` and come back once the device is connected.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            SetupStatusMessageView(
                kind: viewModel.tailscaleStatusKind,
                message: viewModel.tailscaleStatusMessage
            )

            HStack(spacing: FawxSpacing.paddingMD) {
                Button("Back") {
                    viewModel.goBack()
                }
                .buttonStyle(.bordered)

                Spacer(minLength: 0)

                Button("Skip") {
                    viewModel.skipTailscale()
                }
                .buttonStyle(.bordered)

                Button("Continue") {
                    viewModel.continueFromTailscale()
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(!viewModel.canContinueFromTailscale)
            }

            Text("Skipping means secure iPhone pairing will not be available during setup. You can configure it later in Settings.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private var progressBar: some View {
        GeometryReader { proxy in
            let totalWidth = max(proxy.size.width, 1)
            let completedChecks = [
                viewModel.tailscaleStatus?.installed == true,
                viewModel.tailscaleStatus?.running == true && viewModel.tailscaleStatus?.loggedIn == true,
                viewModel.tailscaleStatus?.certReady == true,
            ].filter { $0 }.count
            let progress = CGFloat(completedChecks) / 3

            RoundedRectangle(cornerRadius: 999)
                .fill(Color.fawxSurfaceHover)
                .overlay(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 999)
                        .fill(completedChecks == 3 ? Color.fawxSuccess : Color.fawxAccent)
                        .frame(width: totalWidth * progress)
                }
        }
        .frame(height: 6)
    }
}
