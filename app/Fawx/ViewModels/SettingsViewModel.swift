import Foundation
import Observation

enum ConnectionTestKind {
    case idle
    case success
    case warning
    case failure
}

enum OnboardingStep: Int, Sendable {
    case serverURL
    case pairingCode
}

struct ScannedConnectionInfo {
    let serverURL: String
    let token: String?
}

@MainActor
@Observable
final class SettingsViewModel {
    var serverURL: String
    var pairingCode = ""
    var onboardingStep: OnboardingStep = .serverURL
    var isTestingConnection = false
    var isPairingDevice = false
    var isProcessingQRCode = false
    var testStatusKind: ConnectionTestKind = .idle
    var testStatusMessage: String?
    var pairingStatusKind: ConnectionTestKind = .idle
    var pairingStatusMessage: String?

    private let appState: AppState
    private var lastSuccessfulURL: String?

    init(appState: AppState) {
        self.appState = appState
        self.serverURL = appState.serverURLString
    }

    var canContinue: Bool {
        guard let canonicalURLString = canonicalizeServerURL(serverURL) else {
            return false
        }

        return testStatusKind == .success && lastSuccessfulURL == canonicalURLString
    }

    var canPair: Bool {
        canContinue && strippedPairingCode.count == 6 && !isPairingDevice
    }

    var isPaired: Bool {
        appState.isConfigured
    }

    var currentDeviceName: String {
        DeviceNameProvider.current()
    }

    var pairedDeviceName: String? {
        appState.pairedDeviceName
    }

    var formattedPairingCode: String {
        Self.formatPairingCode(pairingCode)
    }

    var strippedPairingCode: String {
        Self.sanitizePairingCode(pairingCode)
    }

    func updateServerURL(_ newValue: String) {
        serverURL = newValue

        if lastSuccessfulURL != canonicalizeServerURL(newValue) {
            testStatusKind = .idle
            testStatusMessage = nil
            if onboardingStep == .pairingCode {
                onboardingStep = .serverURL
            }
        }
    }

    func updatePairingCode(_ newValue: String) {
        pairingCode = Self.formatPairingCode(newValue)
        pairingStatusKind = .idle
        pairingStatusMessage = nil
    }

    func reloadStoredValues() {
        serverURL = appState.serverURLString
        pairingCode = ""
        onboardingStep = .serverURL
        testStatusKind = .idle
        testStatusMessage = nil
        pairingStatusKind = .idle
        pairingStatusMessage = nil
        lastSuccessfulURL = appState.isConfigured ? canonicalizeServerURL(appState.serverURLString) : nil
    }

    func testConnection() async {
        guard let canonicalURLString = canonicalizeServerURL(serverURL) else {
            testStatusKind = .failure
            testStatusMessage = serverURLValidationMessage(serverURL)
            return
        }
        guard let url = URL(string: canonicalURLString) else {
            testStatusKind = .failure
            testStatusMessage = serverURLValidationMessage(serverURL)
            return
        }

        isTestingConnection = true
        defer { isTestingConnection = false }

        let client = FawxClient(baseURL: url)
        let canonicalCurrentURL = canonicalizeServerURL(appState.serverURLString)
        let updatesCurrentConnection = appState.isConfigured && canonicalCurrentURL == canonicalURLString

        do {
            let health = try await client.health()
            serverURL = canonicalURLString
            lastSuccessfulURL = canonicalURLString

            if updatesCurrentConnection {
                await appState.revalidateConnection(allowReconnect: false)

                if appState.connectionStatus == .connected {
                    testStatusKind = .success
                    testStatusMessage = "Connected. Model: \(health.model)"
                } else {
                    testStatusKind = .failure
                    testStatusMessage = "Disconnected. \(appState.connectionError ?? "Check your pairing in Settings.")"
                }
            } else {
                testStatusKind = .success
                testStatusMessage = "Connected. Model: \(health.model)"
            }
        } catch {
            let userMessage = appState.userFacingConnectionMessage(for: error)

            if updatesCurrentConnection {
                appState.markDisconnected(from: error)
                testStatusMessage = "Disconnected. \(userMessage)"
            } else {
                testStatusMessage = userMessage
            }

            testStatusKind = .failure
        }
    }

    func continueToPairing() {
        guard canContinue else {
            testStatusKind = .failure
            testStatusMessage = "Run a successful health check before pairing."
            return
        }

        serverURL = lastSuccessfulURL ?? serverURL
        pairingCode = ""
        pairingStatusKind = .idle
        pairingStatusMessage = nil
        onboardingStep = .pairingCode
    }

    func returnToServerEntry() {
        onboardingStep = .serverURL
        pairingStatusKind = .idle
        pairingStatusMessage = nil
    }

    func submitPairing() async {
        guard let canonicalURLString = lastSuccessfulURL ?? canonicalizeServerURL(serverURL) else {
            pairingStatusKind = .failure
            pairingStatusMessage = "Enter a valid server URL first."
            return
        }
        guard let url = URL(string: canonicalURLString) else {
            pairingStatusKind = .failure
            pairingStatusMessage = "Enter a valid server URL first."
            return
        }
        guard strippedPairingCode.count == 6 else {
            pairingStatusKind = .failure
            pairingStatusMessage = "Enter the 6-character pairing code from `fawx pair`."
            return
        }

        isPairingDevice = true
        defer { isPairingDevice = false }

        let client = FawxClient(baseURL: url)
        let requestedDeviceName = currentDeviceName

        do {
            let response = try await client.pair(code: strippedPairingCode, deviceName: requestedDeviceName)
            let pairedDeviceName = response.deviceName?.trimmingCharacters(in: .whitespacesAndNewlines)
            let resolvedDeviceName = pairedDeviceName?.nonEmpty ?? requestedDeviceName

            try await appState.savePairing(
                serverURLString: canonicalURLString,
                token: response.token,
                deviceName: resolvedDeviceName,
                connectionMode: .remote
            )
            pairingStatusKind = .success
            pairingStatusMessage = "Paired as \(resolvedDeviceName)."
            lastSuccessfulURL = canonicalURLString
            serverURL = canonicalURLString
            pairingCode = ""
            await appState.bootstrap()
        } catch {
            pairingStatusKind = .failure
            pairingStatusMessage = error.localizedDescription
        }
    }

    func applyScannedConnectionLink(_ rawValue: String) async {
        guard let connection = Self.parseScannedConnection(rawValue) else {
            pairingStatusKind = .failure
            pairingStatusMessage = "That QR code does not contain a valid Fawx connection."
            return
        }

        isProcessingQRCode = true
        defer { isProcessingQRCode = false }

        serverURL = connection.serverURL

        if let directToken = connection.token, !directToken.isEmpty {
            do {
                try await appState.savePairing(
                    serverURLString: connection.serverURL,
                    token: directToken,
                    deviceName: currentDeviceName,
                    connectionMode: .remote
                )
                pairingStatusKind = .success
                pairingStatusMessage = "Connected to \(connection.serverURL)."
                lastSuccessfulURL = connection.serverURL
                await appState.bootstrap()
                return
            } catch {
                pairingStatusKind = .failure
                pairingStatusMessage = error.localizedDescription
                return
            }
        }

        await testConnection()
        if canContinue {
            continueToPairing()
            pairingStatusKind = .warning
            pairingStatusMessage = "Server detected. Enter the pairing code to finish connecting."
        }
    }

    func unpair() async {
        do {
            try await appState.unpair()
            reloadStoredValues()
        } catch {
            testStatusKind = .failure
            testStatusMessage = error.localizedDescription
        }
    }

    private static func sanitizePairingCode(_ rawValue: String) -> String {
        let filtered = rawValue.uppercased().filter { character in
            character.isLetter || character.isNumber
        }
        return String(filtered.prefix(6))
    }

    private static func formatPairingCode(_ rawValue: String) -> String {
        let stripped = sanitizePairingCode(rawValue)
        guard stripped.count > 3 else {
            return stripped
        }

        let splitIndex = stripped.index(stripped.startIndex, offsetBy: 3)
        return "\(stripped[..<splitIndex])-\(stripped[splitIndex...])"
    }

    static func parseScannedConnection(_ rawValue: String) -> ScannedConnectionInfo? {
        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return nil
        }

        if let canonicalURL = canonicalizeServerURL(trimmed) {
            return ScannedConnectionInfo(serverURL: canonicalURL, token: nil)
        }

        guard let components = URLComponents(string: trimmed) else {
            return nil
        }

        let items = Dictionary(uniqueKeysWithValues: (components.queryItems ?? []).map { item in
            (item.name.lowercased(), item.value ?? "")
        })

        let host = items["host"] ?? components.host
        let port = items["port"].flatMap(Int.init) ?? components.port ?? 8400
        guard let host, !host.isEmpty else {
            return nil
        }

        let preferredScheme = preferredScannedConnectionScheme(
            host: host,
            explicitScheme: items["scheme"]?.lowercased(),
            transport: items["transport"]?.lowercased()
        )

        guard let canonicalURL = canonicalizeServerURL("\(preferredScheme)://\(host):\(port)") else {
            return nil
        }

        let token = items["token"]
        let resolvedToken: String?
        if let token, !token.isEmpty, token.uppercased() != "REDACTED" {
            resolvedToken = token
        } else {
            resolvedToken = nil
        }

        return ScannedConnectionInfo(serverURL: canonicalURL, token: resolvedToken)
    }

    private static func preferredScannedConnectionScheme(
        host: String,
        explicitScheme: String?,
        transport: String?
    ) -> String {
        if explicitScheme == "http" || transport == "lan_http" {
            return "http"
        }

        if explicitScheme == "https" || transport == "tailscale_https" || host.lowercased().contains(".ts.net") {
            return "https"
        }

        let normalizedHost = host
            .trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
            .lowercased()

        if normalizedHost == "localhost" || normalizedHost == "127.0.0.1" || normalizedHost == "::1" {
            return "http"
        }

        return "https"
    }
}
