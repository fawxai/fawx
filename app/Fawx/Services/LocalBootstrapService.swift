import Foundation
#if os(macOS)
import Darwin
#endif

struct BootstrapResult: Decodable, Sendable {
    let port: Int
    let host: String
    let bearerToken: String
    let dataDir: String
    let configPath: String
    let created: Bool

    enum CodingKeys: String, CodingKey {
        case port
        case host
        case bearerToken = "bearer_token"
        case dataDir = "data_dir"
        case configPath = "config_path"
        case created
    }
}

struct BootstrapError: Decodable, Sendable {
    let error: String
    let portRange: [Int]?

    enum CodingKeys: String, CodingKey {
        case error
        case portRange = "port_range"
    }
}

actor LocalBootstrapService {
    enum BootstrapFailure: Error, LocalizedError {
        case bundledBinaryNotFound
        case unsupportedPlatform
        case processFailedToLaunch(String)
        case processExitedWithError(code: Int32, message: String)
        case invalidOutput(String)
        case invalidHealthCheckURL(host: String, port: Int)
        case serverHealthTimeout
        case launchAgentInstallFailed(String)

        var errorDescription: String? {
            switch self {
            case .bundledBinaryNotFound:
                return "The bundled Fawx server could not be found. Reinstall the app and try again."
            case .unsupportedPlatform:
                return "Local Fawx setup is only available on macOS."
            case .processFailedToLaunch(let message):
                return "Fawx couldn't start its setup helper: \(message)"
            case .processExitedWithError(let code, let message):
                return message.nonEmpty ?? "Fawx setup couldn't finish (exit code \(code))."
            case .invalidOutput:
                return "Fawx setup returned an unexpected response."
            case .invalidHealthCheckURL:
                return "Fawx returned an invalid local server address. Please try again."
            case .serverHealthTimeout:
                return "Fawx took too long to start. Please try again."
            case .launchAgentInstallFailed(let message):
                return "Fawx couldn't enable automatic startup: \(message)"
            }
        }
    }

    private static let launchAgentLabel = "ai.fawx.server"
    private let healthPollInterval: Duration = .milliseconds(500)
    private let healthTimeout: Duration = .seconds(15)

    func performFullBootstrap(
        progress: @escaping @MainActor @Sendable (String) -> Void = { _ in }
    ) async throws -> BootstrapResult {
#if os(macOS)
        await progress("Creating Fawx configuration...")
        let binaryURL = try bundledServerBinaryURL()
        let result = try await runBootstrapCommand(binaryURL: binaryURL)

        guard result.created else {
            await progress("Using your existing Fawx configuration...")
            return result
        }

        await progress("Installing Fawx to start automatically...")
        try await installAndLoadLaunchAgent(binaryURL: binaryURL, result: result)
        await progress("Starting the local Fawx server...")
        try await waitForServerHealth(host: result.host, port: result.port)
        return result
#else
        throw BootstrapFailure.unsupportedPlatform
#endif
    }

    nonisolated static func generatePlist(
        binaryPath: String,
        port: Int,
        dataDir: String,
        logPath: String
    ) -> String {
        // Keep this LaunchAgent template in sync with
        // engine/crates/fx-api/src/launchagent.rs::generate_plist.
        // Swift duplicates it for first-launch bootstrap before the Rust API is running.
        """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>Label</key>
            <string>\(launchAgentLabel)</string>
            <key>ProgramArguments</key>
            <array>
                <string>\(xmlEscape(binaryPath))</string>
                <string>serve</string>
                <string>--http</string>
                <string>--port</string>
                <string>\(port)</string>
                <string>--data-dir</string>
                <string>\(xmlEscape(dataDir))</string>
            </array>
            <key>RunAtLoad</key>
            <true/>
            <key>KeepAlive</key>
            <true/>
            <key>StandardOutPath</key>
            <string>\(xmlEscape(logPath))</string>
            <key>StandardErrorPath</key>
            <string>\(xmlEscape(logPath))</string>
        </dict>
        </plist>
        """
    }

    nonisolated static func xmlEscape(_ string: String) -> String {
        string
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }

#if os(macOS)
    private func bundledServerBinaryURL() throws -> URL {
        let binaryURL = Bundle.main.bundleURL
            .appendingPathComponent("Contents", isDirectory: true)
            .appendingPathComponent("MacOS", isDirectory: true)
            .appendingPathComponent("fawx-server", isDirectory: false)

        guard FileManager.default.isExecutableFile(atPath: binaryURL.path) else {
            throw BootstrapFailure.bundledBinaryNotFound
        }

        return binaryURL
    }

    private func runBootstrapCommand(binaryURL: URL) async throws -> BootstrapResult {
        try await Task.detached(priority: .userInitiated) {
            let process = Process()
            process.executableURL = binaryURL
            process.arguments = ["bootstrap", "--json"]

            let stdout = Pipe()
            let stderr = Pipe()
            process.standardOutput = stdout
            process.standardError = stderr

            do {
                try process.run()
            } catch {
                throw BootstrapFailure.processFailedToLaunch(error.localizedDescription)
            }

            process.waitUntilExit()

            let outputData = stdout.fileHandleForReading.readDataToEndOfFile()
            let errorData = stderr.fileHandleForReading.readDataToEndOfFile()

            guard process.terminationStatus == 0 else {
                throw Self.processFailure(
                    status: process.terminationStatus,
                    outputData: outputData,
                    errorData: errorData
                )
            }

            return try Self.decodeBootstrapResult(from: outputData)
        }.value
    }

    private func installAndLoadLaunchAgent(binaryURL: URL, result: BootstrapResult) async throws {
        try await Task.detached(priority: .userInitiated) {
            let plistURL = try Self.launchAgentPlistURL()
            let logURL = try Self.defaultLogURL()
            let dataDirURL = URL(fileURLWithPath: result.dataDir, isDirectory: true)
            let plistContent = Self.generatePlist(
                binaryPath: binaryURL.path,
                port: result.port,
                dataDir: dataDirURL.path,
                logPath: logURL.path
            )

            try Self.createParentDirectory(for: plistURL)
            try Self.createParentDirectory(for: logURL)
            try plistContent.write(to: plistURL, atomically: true, encoding: .utf8)

            let domain = "gui/\(getuid())"
            try? Self.runLaunchctl(arguments: ["bootout", domain, plistURL.path])
            try Self.runLaunchctl(arguments: ["bootstrap", domain, plistURL.path])
        }.value
    }

    private static func decodeBootstrapResult(from data: Data) throws -> BootstrapResult {
        do {
            return try JSONDecoder().decode(BootstrapResult.self, from: data)
        } catch {
            let raw = trimmedString(from: data) ?? "<no output>"
            throw BootstrapFailure.invalidOutput(raw)
        }
    }

    private static func processFailure(
        status: Int32,
        outputData: Data,
        errorData: Data
    ) -> BootstrapFailure {
        if let bootstrapError = decodeBootstrapError(from: outputData) ?? decodeBootstrapError(from: errorData) {
            return .processExitedWithError(code: status, message: bootstrapError.error)
        }

        let message = trimmedString(from: errorData) ?? trimmedString(from: outputData) ?? ""
        return .processExitedWithError(code: status, message: message)
    }

    private static func decodeBootstrapError(from data: Data) -> BootstrapError? {
        guard !data.isEmpty else {
            return nil
        }

        return try? JSONDecoder().decode(BootstrapError.self, from: data)
    }

    private static func trimmedString(from data: Data) -> String? {
        String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    private static func launchAgentPlistURL() throws -> URL {
        try homeDirectory()
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("LaunchAgents", isDirectory: true)
            .appendingPathComponent("\(launchAgentLabel).plist", isDirectory: false)
    }

    private static func defaultLogURL() throws -> URL {
        try homeDirectory()
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Logs", isDirectory: true)
            .appendingPathComponent("Fawx", isDirectory: true)
            .appendingPathComponent("server.log", isDirectory: false)
    }

    private static func homeDirectory() throws -> URL {
        let homePath = FileManager.default.homeDirectoryForCurrentUser.path
        guard !homePath.isEmpty else {
            throw BootstrapFailure.launchAgentInstallFailed("Your home folder could not be found.")
        }

        return FileManager.default.homeDirectoryForCurrentUser
    }

    private static func createParentDirectory(for fileURL: URL) throws {
        let directoryURL = fileURL.deletingLastPathComponent()

        do {
            try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        } catch {
            throw BootstrapFailure.launchAgentInstallFailed(error.localizedDescription)
        }
    }

    private static func runLaunchctl(arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = arguments

        let stderr = Pipe()
        process.standardError = stderr

        do {
            try process.run()
        } catch {
            throw BootstrapFailure.launchAgentInstallFailed(error.localizedDescription)
        }

        process.waitUntilExit()

        guard process.terminationStatus == 0 else {
            let message = trimmedString(from: stderr.fileHandleForReading.readDataToEndOfFile())
                ?? "launchctl exited with code \(process.terminationStatus)."
            throw BootstrapFailure.launchAgentInstallFailed(message)
        }
    }
#endif

    private func waitForServerHealth(host: String, port: Int) async throws {
        guard let healthURL = Self.healthURL(host: host, port: port) else {
            throw BootstrapFailure.invalidHealthCheckURL(host: host, port: port)
        }
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: healthTimeout)

        while clock.now < deadline {
            try Task.checkCancellation()

            do {
                let (_, response) = try await URLSession.shared.data(from: healthURL)
                if let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 {
                    return
                }
            } catch {
                // The server is still starting up.
            }

            try await Task.sleep(for: healthPollInterval)
        }

        throw BootstrapFailure.serverHealthTimeout
    }

    nonisolated private static func healthURL(host: String, port: Int) -> URL? {
        var components = URLComponents()
        components.scheme = "http"
        components.host = host
        components.port = port
        components.path = "/health"
        return components.url
    }
}
