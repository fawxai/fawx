import Observation
import SwiftUI

struct ReadyStep: View {
    @Bindable var viewModel: SetupViewModel
    @Bindable var appState: AppState

    var body: some View {
        SetupWizardCard(maxWidth: 460) {
            VStack(alignment: .center, spacing: FawxSpacing.paddingLG) {
                Text("🎉")
                    .font(.system(size: 48))

                headline

                autoStartRow

                qrSection

                SetupStatusMessageView(
                    kind: viewModel.readyStatusKind,
                    message: viewModel.readyStatusMessage
                )

                HStack(spacing: FawxSpacing.paddingMD) {
                    Button("Back") {
                        viewModel.goBack()
                    }
                    .buttonStyle(.bordered)
                    .disabled(viewModel.isBootstrapping)

                    Spacer(minLength: 0)

                    Button(viewModel.isBootstrapping ? "Starting..." : "Start chatting") {
                        Task {
                            await viewModel.finishSetup()
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.fawxAccent)
                    .disabled(viewModel.isBootstrapping)
                }
            }
            .frame(maxWidth: .infinity)
        }
        .task {
            _ = await NotificationService.shared.requestPermission()
        }
    }

    @ViewBuilder
    private var headline: some View {
        if viewModel.isBootstrapping {
            VStack(spacing: FawxSpacing.paddingSM) {
                ProgressView()
                    .controlSize(.large)

                Text(viewModel.bootstrapProgress ?? "Setting up Fawx...")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .multilineTextAlignment(.center)
            }
        } else {
            Text("Ready to start")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(Color.fawxText)
                .multilineTextAlignment(.center)
        }
    }

    private var autoStartRow: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Start Fawx when you log in")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text("Launches automatically via LaunchAgent")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Spacer(minLength: 0)

            Toggle(
                "",
                isOn: Binding(
                    get: { viewModel.readyAutoStartEnabled },
                    set: { enabled in
                        Task {
                            await viewModel.setAutoStartEnabled(enabled)
                        }
                    }
                )
            )
            .labelsHidden()
            .tint(.fawxAccent)
            .disabled(viewModel.isTogglingAutoStart || viewModel.isBootstrapping)
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    @ViewBuilder
    private var qrSection: some View {
        if let pairing = viewModel.qrPairing {
            VStack(spacing: FawxSpacing.paddingMD) {
                QRCodeView(payload: pairing.schemeURL, size: 180)

                Text("Scan with your iPhone to connect")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                VStack(spacing: 6) {
                    Text(verbatim: "\(pairing.displayHost):\(pairing.port)")
                        .font(FawxTypography.code)
                        .foregroundStyle(Color.fawxTextSecondary)

                    SetupTransportBadge(transport: pairing.transport)
                }

                Text("Fawx only works while your iPhone is on the same tailnet or local network.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxWarning)
                    .multilineTextAlignment(.center)
            }
        } else {
            VStack(spacing: FawxSpacing.paddingSM) {
                Text("iPhone pairing isn’t available yet.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text("Set up Tailscale in Settings to enable secure QR pairing.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .multilineTextAlignment(.center)
            }
            .padding(FawxSpacing.paddingLG)
            .frame(maxWidth: .infinity)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}
