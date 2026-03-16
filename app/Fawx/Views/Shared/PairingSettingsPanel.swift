import Observation
import SwiftUI

struct PairingSettingsPanel: View {
    @Bindable var appState: AppState
    @Bindable var settingsViewModel: SettingsViewModel
    let isReadOnly: Bool
    let openScanner: (() -> Void)?

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
        if let pairing = appState.qrPairingResponse {
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

                Button("Regenerate QR Code") {
                    Task {
                        await regenerateQRCode()
                    }
                }
                .buttonStyle(.bordered)
            }
            .frame(maxWidth: .infinity)
        } else {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("iPhone pairing is unavailable.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text("Set up Tailscale HTTPS to show a scannable QR code for iPhone pairing.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
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
        if let pairing = appState.qrPairingResponse {
            switch pairing.transport {
            case "tailscale_https":
                return "Paired via Tailscale HTTPS"
            case "lan_http":
                return "Connected over your local network"
            default:
                return "Connected to your Mac"
            }
        }

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

    private func regenerateQRCode() async {
        do {
            _ = try await appState.fetchPairingQRCode()
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
