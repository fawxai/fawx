import Observation
import SwiftUI

struct PairingSettingsPanel: View {
    @Bindable var appState: AppState
    @Bindable var settingsViewModel: SettingsViewModel
    let isReadOnly: Bool
    let openScanner: (() -> Void)?

    @State private var generatedPairingCode: PairingCodeResponse?
    @State private var isGeneratingPairingCode = false
    @State private var isShowingQRCode = false
    @State private var isGeneratingQRCode = false
    @State private var statusKind: ConnectionTestKind = .idle
    @State private var statusMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            if isReadOnly {
                iOSConnectionCard
            } else {
                macPairingCard
            }

            SetupStatusMessageView(kind: statusKind, message: statusMessage)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    @ViewBuilder
    private var macPairingCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("Connect another device")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Text("Generate a short pairing code for manual entry, or generate a QR code only when you want to scan it.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                Button(isGeneratingPairingCode ? "Generating..." : generatedPairingCode == nil ? "Generate Pairing Code" : "Refresh Pairing Code") {
                    Task {
                        await generatePairingCode()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(isGeneratingPairingCode)

                Button(isGeneratingQRCode ? "Generating QR..." : isShowingQRCode ? "Refresh QR Code" : "Generate QR Code") {
                    Task {
                        await regenerateQRCode()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(isGeneratingQRCode)
            }

            if let generatedPairingCode {
                pairingCodeCard(generatedPairingCode)
            }

            if isShowingQRCode {
                if let pairing = appState.qrPairingResponse {
                    qrCodeCard(pairing)
                } else {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text("QR pairing is unavailable right now.")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxText)

                        Text("Try generating the QR code again after the local server is available.")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                    .padding(FawxSpacing.paddingMD)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.fawxBackground)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }

    private var iOSConnectionCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            VStack(spacing: FawxSpacing.paddingSM) {
                Image(systemName: appState.isConfigured ? "checkmark.circle.fill" : "iphone.slash")
                    .font(.system(size: 36, weight: .medium))
                    .foregroundStyle(appState.isConfigured ? Color.fawxSuccess : Color.fawxTextSecondary)

                Text(appState.isConfigured ? "Connected" : "Not connected")
                    .font(FawxTypography.heading2)
                    .foregroundStyle(Color.fawxText)

                Text(connectionSubtitle)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)

            VStack(spacing: FawxSpacing.paddingSM) {
                serverInfoRow(label: "Server", value: appState.displayedHost)
                serverInfoRow(label: "Port", value: appState.displayedPort.map(String.init) ?? "—")
                serverInfoRow(label: "Status", value: appState.serverStatusLabel)
            }
            .padding(FawxSpacing.paddingMD)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            if let openScanner {
                Button(appState.isConfigured ? "Scan New QR Code" : "Scan QR Code") {
                    openScanner()
                }
                .buttonStyle(.bordered)
            }

            if appState.isConfigured {
                Button("Disconnect", role: .destructive) {
                    Task {
                        await disconnect()
                    }
                }
                .buttonStyle(.bordered)
            }
        }
        .frame(maxWidth: .infinity)
    }

    private var connectionSubtitle: String {
        if appState.displayedHost.contains(".ts.net") {
            return "Connected to your Mac over Tailscale"
        }

        return appState.isConfigured ? "Connected to your Fawx server" : "Scan a Mac pairing QR code or enter a pairing code to connect."
    }

    private func serverInfoRow(label: String, value: String) -> some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Text(label)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            Spacer(minLength: 0)

            Text(value)
                .font(label == "Server" ? FawxTypography.code : FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .multilineTextAlignment(.trailing)
        }
    }

    private func pairingCodeCard(_ pairingCode: PairingCodeResponse) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Pairing Code")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text(pairingCode.code)
                .font(.system(size: 28, weight: .bold, design: .monospaced))
                .foregroundStyle(Color.fawxText)

            TimelineView(.periodic(from: .now, by: 1)) { context in
                Text(pairingCodeExpirationText(pairingCode, now: context.date))
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Text("Enter this code in the Fawx app on your iPhone to finish pairing.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func qrCodeCard(_ pairing: QrPairingResponse) -> some View {
        VStack(alignment: .center, spacing: FawxSpacing.paddingLG) {
            QRCodeView(payload: pairing.schemeURL, size: 180)

            VStack(spacing: FawxSpacing.paddingSM) {
                serverInfoRow(label: "Hostname", value: pairing.displayHost)
                serverInfoRow(label: "Port", value: String(pairing.port))
                HStack {
                    Text("Transport")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                    Spacer(minLength: 0)
                    SetupTransportBadge(transport: pairing.transport)
                }
            }
            .padding(FawxSpacing.paddingMD)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            Text(
                pairing.sameNetworkOnly
                    ? "This QR code works while your iPhone is on the same local network."
                    : "This QR code works while your iPhone is on the same tailnet."
            )
            .font(FawxTypography.status)
            .foregroundStyle(pairing.sameNetworkOnly ? Color.fawxWarning : Color.fawxTextSecondary)
            .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
    }

    private func pairingCodeExpirationText(_ pairingCode: PairingCodeResponse, now: Date) -> String {
        let remaining = max(0, pairingCode.expiresAt - Int(now.timeIntervalSince1970))
        if remaining == 0 {
            return "Expired. Generate a new code to pair another device."
        }
        if remaining < 60 {
            return "Expires in \(remaining)s"
        }
        let minutes = remaining / 60
        let seconds = remaining % 60
        return "Expires in \(minutes)m \(seconds)s"
    }

    private func generatePairingCode() async {
        isGeneratingPairingCode = true
        defer { isGeneratingPairingCode = false }

        do {
            generatedPairingCode = try await appState.generatePairingCode()
            statusKind = .success
            statusMessage = "Pairing code generated."
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }

    private func regenerateQRCode() async {
        isGeneratingQRCode = true
        defer { isGeneratingQRCode = false }

        do {
            _ = try await appState.fetchPairingQRCode()
            isShowingQRCode = true
            statusKind = .success
            statusMessage = "QR code refreshed."
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }

    private func disconnect() async {
        await settingsViewModel.unpair()
        statusKind = .warning
        statusMessage = "This device has been disconnected."
    }
}
