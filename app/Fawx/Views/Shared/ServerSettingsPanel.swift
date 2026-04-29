import Observation
import SwiftUI

#if os(macOS)
import AppKit
#endif

struct ServerSettingsPanel: View {
    @Bindable var appState: AppState
    let isReadOnly: Bool

    @State private var portText = ""
    @State private var statusKind: ConnectionTestKind = .idle
    @State private var statusMessage: String?
    @State private var isUpdatingPort = false

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            statusSection

            if isReadOnly {
                preferencesReadOnlySection
            } else {
                preferencesSection
                actionsSection
                logsSection
            }

            SetupStatusMessageView(kind: statusKind, message: statusMessage)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .fawxSurface(.section)
        .onAppear {
            syncPortText()
        }
        .onChange(of: appState.localServerStatus?.port) { _, _ in
            syncPortText()
        }
    }

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Status")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            HStack(spacing: FawxSpacing.paddingSM) {
                Circle()
                    .fill(statusColor)
                    .frame(width: 9, height: 9)

                Text(appState.serverStatusLabel)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(statusColor)

                if let uptime = appState.localServerStatus?.uptimeSeconds, uptime > 0 {
                    Text("Uptime: \(uptimeString(uptime))")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }

            if let port = appState.displayedPort {
                Text(verbatim: "\(appState.displayedHost):\(port)")
                    .font(FawxTypography.code)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            if let serverStatusError = appState.serverStatusError {
                Text("Status refresh issue: \(serverStatusError)")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxWarning)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    @ViewBuilder
    private var preferencesReadOnlySection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Preferences")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            serverValueRow(label: "Port", value: appState.displayedPort.map(String.init) ?? "Unavailable")
            serverValueRow(label: "Start at login", value: appState.autoStartEnabled ? "On" : "Off")
        }
    }

    private var preferencesSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Preferences")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

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
                        get: { appState.autoStartEnabled },
                        set: { enabled in
                            Task {
                                await updateAutoStart(enabled)
                            }
                        }
                    )
                )
                .labelsHidden()
                .tint(.fawxAccent)
                .disabled(appState.isUpdatingServerSettings)
            }

            HStack(alignment: .bottom, spacing: FawxSpacing.paddingMD) {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    Text("Port")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxText)

                    TextField("8400", text: $portText)
                        .textFieldStyle(.roundedBorder)
#if os(macOS)
                        .frame(width: 120)
#endif
                }

                Button(isUpdatingPort ? "Saving..." : "Update") {
                    Task {
                        await savePort()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(isUpdatingPort || appState.isUpdatingServerSettings)
            }
        }
    }

    private var actionsSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Actions")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            HStack(spacing: FawxSpacing.paddingMD) {
                Button("Restart Server") {
                    Task {
                        await restartServer()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(appState.isUpdatingServerSettings || appState.connectionStatus == .connecting)

                Button("Stop Server") {
                    Task {
                        await stopServer()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(appState.isUpdatingServerSettings)

                if appState.launchAgentStatus?.installed == true || appState.autoStartEnabled {
                    Button("Uninstall LaunchAgent", role: .destructive) {
                        Task {
                            await updateAutoStart(false)
                        }
                    }
                    .buttonStyle(.bordered)
                    .disabled(appState.isUpdatingServerSettings)
                }
            }
        }
    }

    @ViewBuilder
    private var logsSection: some View {
#if os(macOS)
        if let url = appState.localLogFileURL {
            Button("View server logs") {
                NSWorkspace.shared.open(url)
            }
            .buttonStyle(.plain)
            .font(FawxTypography.chatBody)
            .foregroundStyle(Color.fawxAccent)
        }
#endif
    }

    private func serverValueRow(label: String, value: String) -> some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Text(label)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            Spacer(minLength: 0)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
    }

    private var statusColor: Color {
        switch appState.serverStatusLabel.lowercased() {
        case "running", "connected":
            .fawxSuccess
        case "starting", "connecting", "reconnecting":
            .fawxWarning
        case "stopped", "disconnected":
            .fawxError
        default:
            .fawxTextSecondary
        }
    }

    private func syncPortText() {
        if let port = appState.localServerStatus?.port ?? appState.displayedPort {
            portText = String(port)
        }
    }

    private func updateAutoStart(_ enabled: Bool) async {
        do {
            statusKind = .success
            statusMessage = try await appState.setLaunchAgentEnabled(enabled)
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }

    private func savePort() async {
        guard let newPort = Int(portText), newPort > 0 else {
            statusKind = .failure
            statusMessage = "Enter a valid port number."
            return
        }

        isUpdatingPort = true
        defer { isUpdatingPort = false }

        do {
            let response = try await appState.updateServerPort(newPort)
            statusKind = response.restartRequired ? .warning : .success
            statusMessage = response.restartRequired
                ? "Port updated. Restart the server to apply it."
                : "Server port updated."
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }

    private func restartServer() async {
        do {
            statusKind = .success
            statusMessage = try await appState.restartLocalServer()
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }

    private func stopServer() async {
        do {
            statusKind = .warning
            statusMessage = try await appState.stopLocalServer()
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
        }
    }
}
