import Observation
import SwiftUI

struct OnboardingView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState

#if os(iOS)
    @State private var isShowingQRScanner = false
#endif

    var body: some View {
        ZStack {
            Color.fawxBackground.ignoresSafeArea()

            VStack(spacing: FawxSpacing.paddingLG) {
                titleSection

#if os(iOS)
                scanCard
#endif

                onboardingCard

#if os(macOS)
                if appState.canOpenRemoteOnboarding {
                    Button("Set up this Mac instead") {
                        appState.returnToLocalSetup()
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Color.fawxAccent)
                }
#endif
            }
            .padding(FawxSpacing.paddingXL)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
#if os(iOS)
        .sheet(isPresented: $isShowingQRScanner) {
            QRCodeScannerSheet(
                onCancel: {
                    isShowingQRScanner = false
                },
                onCodeScanned: { rawValue in
                    isShowingQRScanner = false
                    Task {
                        await settingsViewModel.applyScannedConnectionLink(rawValue)
                    }
                }
            )
        }
#endif
    }

    private var onboardingCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            switch settingsViewModel.onboardingStep {
            case .serverURL:
                serverStep
            case .pairingCode:
                pairingStep
            }
        }
        .padding(FawxSpacing.paddingXL)
        .frame(maxWidth: 460)
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private var titleSection: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            Group {
#if os(macOS)
                Text("Connect to another Fawx server")
#else
                Text("Connect to Fawx")
#endif
            }
            .font(FawxTypography.heading1)
            .foregroundStyle(Color.fawxText)

            Text(titleDetail)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
        }
    }

#if os(iOS)
    private var scanCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Scan a pairing QR code from your Mac to fill in the connection automatically.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            Button(settingsViewModel.isProcessingQRCode ? "Scanning..." : "Scan QR Code") {
                isShowingQRScanner = true
            }
            .buttonStyle(.borderedProminent)
            .tint(.fawxAccent)
            .disabled(settingsViewModel.isProcessingQRCode)
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: 460)
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
#endif

    private var titleDetail: String {
#if os(macOS)
        "Enter the server URL and pairing code to use a Fawx server running on another machine."
#else
        "Scan the QR code from your Mac or enter the server URL and pairing code manually."
#endif
    }

    private var serverStep: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            stepHeader(
                number: 1,
                title: "Connect to your server",
                detail: "Enter the Fawx server URL, then run a health check before pairing."
            )

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("Server URL")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                TextField(
                    "http://your-fawx-host:8400",
                    text: Binding(
                        get: { settingsViewModel.serverURL },
                        set: { settingsViewModel.updateServerURL($0) }
                    )
                )
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("serverURLField")
                .onSubmit {
                    Task {
                        await settingsViewModel.testConnection()
                    }
                }
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                Button(settingsViewModel.isTestingConnection ? "Checking..." : "Run Health Check") {
                    Task {
                        await settingsViewModel.testConnection()
                    }
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("testConnectionButton")
                .disabled(settingsViewModel.isTestingConnection)

                Button("Next") {
                    settingsViewModel.continueToPairing()
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .accessibilityIdentifier("continueButton")
                .disabled(!settingsViewModel.canContinue)
            }

            if let status = settingsViewModel.testStatusMessage {
                Text(status)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(color(for: settingsViewModel.testStatusKind))
            }
        }
    }

    private var pairingStep: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            stepHeader(
                number: 2,
                title: "Enter pairing code",
                detail: "Run `fawx pair` on your server, then enter the code below."
            )

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("Pairing Code")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                TextField(
                    "ABC-123",
                    text: Binding(
                        get: { settingsViewModel.formattedPairingCode },
                        set: { settingsViewModel.updatePairingCode($0) }
                    )
                )
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 22, weight: .semibold, design: .monospaced))
                .multilineTextAlignment(.center)
                .accessibilityIdentifier("bearerTokenField")
                .onSubmit {
                    Task {
                        await settingsViewModel.submitPairing()
                    }
                }

                Text("Pairing as \(settingsViewModel.currentDeviceName)")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                Button("Back") {
                    settingsViewModel.returnToServerEntry()
                }
                .buttonStyle(.bordered)

                Button(settingsViewModel.isPairingDevice ? "Pairing..." : "Pair Device") {
                    Task {
                        await settingsViewModel.submitPairing()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(!settingsViewModel.canPair)
            }

            if let status = settingsViewModel.pairingStatusMessage {
                Text(status)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(color(for: settingsViewModel.pairingStatusKind))
            }
        }
    }

    private func stepHeader(number: Int, title: String, detail: String) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Step \(number) of 2")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            Text(title)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text(detail)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private func color(for kind: ConnectionTestKind) -> Color {
        switch kind {
        case .success:
            return .fawxSuccess
        case .warning:
            return .fawxWarning
        case .failure:
            return .fawxError
        case .idle:
            return .fawxTextSecondary
        }
    }
}
