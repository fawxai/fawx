# Spec: Swift Bootstrap Integration

**Status:** READY FOR IMPLEMENTATION (after bootstrap-command.md PR merges)
**PR Target:** `dev`
**Scope:** Rewire `completeLocalSetup()` to use the bundled `fawx-server bootstrap` command, make Ready screen truthful

---

## Problem

`AppState.completeLocalSetup()` calls `adoptLocalDevice()` against localhost, but on a fresh install no server is running. The Ready screen shows "Fawx is running on this Mac" regardless of actual state. The setup wizard reaches a success state before any real backend/install state exists.

## Solution

1. Run the bundled `fawx-server bootstrap --json` to create config + credentials
2. Install and load the LaunchAgent using the bundled binary path
3. Wait for the server to become healthy
4. Only then call `adoptLocalDevice()` and transition to connected state
5. Make the Ready screen truthful: show progress during bootstrap, real errors on failure

---

## Part 1: Bootstrap Process Integration

### New file: `app/Fawx/Services/LocalBootstrapService.swift`

```swift
import Foundation

struct BootstrapResult: Decodable, Sendable {
    let port: Int
    let host: String
    let bearerToken: String
    let dataDir: String
    let configPath: String
    let created: Bool
    
    enum CodingKeys: String, CodingKey {
        case port, host
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
        case processFailedToLaunch(Error)
        case processExitedWithError(code: Int32, message: String)
        case invalidOutput(String)
        case serverHealthTimeout
        case launchAgentInstallFailed(String)
        
        var errorDescription: String? {
            switch self {
            case .bundledBinaryNotFound:
                return "Fawx server binary not found in app bundle."
            case .processFailedToLaunch(let error):
                return "Could not launch Fawx server: \(error.localizedDescription)"
            case .processExitedWithError(_, let message):
                return message
            case .invalidOutput(let output):
                return "Unexpected output from Fawx bootstrap: \(output)"
            case .serverHealthTimeout:
                return "Fawx server did not start within the expected time."
            case .launchAgentInstallFailed(let message):
                return "Could not install auto-start agent: \(message)"
            }
        }
    }
    
    private let healthPollInterval: TimeInterval = 0.5
    private let healthTimeout: TimeInterval = 15.0
    
    /// Full bootstrap sequence: create config → install LaunchAgent → wait for health
    func performFullBootstrap() async throws -> BootstrapResult {
        let binaryURL = try bundledServerBinaryURL()
        let result = try await runBootstrapCommand(binaryURL: binaryURL)
        try await installAndLoadLaunchAgent(binaryURL: binaryURL, result: result)
        try await waitForServerHealth(host: result.host, port: result.port)
        return result
    }
    
    private func bundledServerBinaryURL() throws -> URL {
        let bundleURL = Bundle.main.bundleURL
        let binaryURL = bundleURL
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
                throw BootstrapFailure.processFailedToLaunch(error)
            }
            
            process.waitUntilExit()
            
            let outputData = stdout.fileHandleForReading.readDataToEndOfFile()
            let errorData = stderr.fileHandleForReading.readDataToEndOfFile()
            
            guard process.terminationStatus == 0 else {
                let errorString = String(data: errorData, encoding: .utf8)
                    ?? String(data: outputData, encoding: .utf8)
                    ?? "Unknown error"
                
                // Try to parse structured error
                if let bootstrapError = try? JSONDecoder().decode(BootstrapError.self, from: outputData) {
                    throw BootstrapFailure.processExitedWithError(
                        code: process.terminationStatus,
                        message: bootstrapError.error
                    )
                }
                
                throw BootstrapFailure.processExitedWithError(
                    code: process.terminationStatus,
                    message: errorString.trimmingCharacters(in: .whitespacesAndNewlines)
                )
            }
            
            guard let result = try? JSONDecoder().decode(BootstrapResult.self, from: outputData) else {
                let raw = String(data: outputData, encoding: .utf8) ?? "<binary>"
                throw BootstrapFailure.invalidOutput(raw)
            }
            
            return result
        }.value
    }
    
    private func installAndLoadLaunchAgent(binaryURL: URL, result: BootstrapResult) async throws {
        try await Task.detached(priority: .userInitiated) {
            let dataDir = URL(fileURLWithPath: result.dataDir)
            let logDir = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Logs/Fawx", isDirectory: true)
            try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
            let logPath = logDir.appendingPathComponent("server.log")
            
            // Generate plist content
            let plistContent = self.generatePlist(
                binaryPath: binaryURL.path,
                port: result.port,
                dataDir: dataDir.path,
                logPath: logPath.path
            )
            
            // Write plist
            let plistURL = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/LaunchAgents/ai.fawx.server.plist")
            let agentsDir = plistURL.deletingLastPathComponent()
            try? FileManager.default.createDirectory(at: agentsDir, withIntermediateDirectories: true)
            try plistContent.write(to: plistURL, atomically: true, encoding: .utf8)
            
            // Get current user UID for launchctl domain
            let uid = getuid()
            let domain = "gui/\(uid)"
            
            // Bootout existing (ignore errors — may not be loaded)
            let bootout = Process()
            bootout.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            bootout.arguments = ["bootout", domain, plistURL.path]
            try? bootout.run()
            bootout.waitUntilExit()
            
            // Bootstrap (load) the agent
            let bootstrap = Process()
            bootstrap.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            bootstrap.arguments = ["bootstrap", domain, plistURL.path]
            
            do {
                try bootstrap.run()
                bootstrap.waitUntilExit()
                
                guard bootstrap.terminationStatus == 0 else {
                    throw BootstrapFailure.launchAgentInstallFailed(
                        "launchctl bootstrap exited with code \(bootstrap.terminationStatus)"
                    )
                }
            } catch let error as BootstrapFailure {
                throw error
            } catch {
                throw BootstrapFailure.launchAgentInstallFailed(error.localizedDescription)
            }
        }.value
    }
    
    private nonisolated func generatePlist(
        binaryPath: String,
        port: Int,
        dataDir: String,
        logPath: String
    ) -> String {
        """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" \
        "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>Label</key>
            <string>ai.fawx.server</string>
            <key>ProgramArguments</key>
            <array>
                <string>\(xmlEscape(binaryPath))</string>
                <string>serve</string>
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
    
    private nonisolated func xmlEscape(_ string: String) -> String {
        string
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }
    
    private func waitForServerHealth(host: String, port: Int) async throws {
        let healthURL = URL(string: "http://\(host):\(port)/health")!
        let startTime = Date()
        
        while Date().timeIntervalSince(startTime) < healthTimeout {
            if Task.isCancelled { return }
            
            do {
                let (_, response) = try await URLSession.shared.data(from: healthURL)
                if let httpResponse = response as? HTTPURLResponse,
                   httpResponse.statusCode == 200 {
                    return // Server is healthy
                }
            } catch {
                // Server not ready yet, continue polling
            }
            
            try await Task.sleep(for: .milliseconds(Int(healthPollInterval * 1000)))
        }
        
        throw BootstrapFailure.serverHealthTimeout
    }
}
```

---

## Part 2: Rewire `completeLocalSetup()`

### Changes to `AppState.swift`

Replace the current `completeLocalSetup()` with:

```swift
func completeLocalSetup() async throws {
    await awaitPersistedStateLoad()
    
    // Check if we already have a valid local install
    let existingConfig = await refreshLocalInstallConfiguration()
    if let existingConfig, !existingConfig.bearerToken.isEmpty {
        // Existing valid install — just adopt and connect
        try await adoptAndConnect(
            serverURL: existingConfig.baseURLString,
            bearerToken: existingConfig.bearerToken
        )
        return
    }
    
    // Fresh install — run bootstrap
    let bootstrapService = LocalBootstrapService()
    let result = try await bootstrapService.performFullBootstrap()
    
    // Now the server is running — adopt the local device
    let serverURL = "http://\(result.host):\(result.port)"
    try await adoptAndConnect(serverURL: serverURL, bearerToken: result.bearerToken)
}

private func adoptAndConnect(serverURL: String, bearerToken: String? = nil) async throws {
    guard
        let canonicalURLString = canonicalizeServerURL(serverURL),
        let url = URL(string: canonicalURLString)
    else {
        throw APIError.invalidURL(serverURL)
    }
    
    let setupClient: FawxClient
    if let bearerToken {
        setupClient = FawxClient(baseURL: url, bearerToken: bearerToken)
    } else {
        setupClient = FawxClient(baseURL: url)
    }
    
    let requestedDeviceName = Self.defaultLocalDeviceName()
    let response = try await setupClient.adoptLocalDevice(deviceName: requestedDeviceName)
    let pairedDeviceName = response.deviceName?.trimmingCharacters(in: .whitespacesAndNewlines)
    let resolvedDeviceName = pairedDeviceName?.nonEmpty ?? requestedDeviceName
    
    try await savePairing(
        serverURLString: canonicalURLString,
        token: response.token,
        deviceName: resolvedDeviceName,
        connectionMode: .local
    )
    isSetupComplete = true
    await persistence.setSetupComplete(true)
    await bootstrap()
}
```

---

## Part 3: Truthful Ready Screen

### Changes to `ReadyStep.swift`

Replace static "Fawx is running on this Mac" with dynamic state:

```swift
// Add a bootstrapping state to SetupViewModel
var isBootstrapping = false
var bootstrapProgress: String?
```

In ReadyStep body, replace the headline:

```swift
// BEFORE:
Text("Fawx is running on this Mac")

// AFTER:
Group {
    if viewModel.isBootstrapping {
        VStack(spacing: FawxSpacing.paddingSM) {
            ProgressView()
                .controlSize(.large)
            Text(viewModel.bootstrapProgress ?? "Setting up Fawx...")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    } else {
        Text("Ready to start")
            .font(.system(size: 22, weight: .bold))
            .foregroundStyle(Color.fawxText)
    }
}
```

### Changes to `SetupViewModel.swift`

Update `finishSetup()`:

```swift
func finishSetup() async {
    isBootstrapping = true
    bootstrapProgress = "Creating Fawx configuration..."
    defer { isBootstrapping = false }
    
    do {
        try await appState.completeLocalSetup()
    } catch {
        readyStatusKind = .failure
        readyStatusMessage = error.localizedDescription
    }
}
```

---

## Part 4: Existing completeLocalSetup Callers

Search for all callers of `completeLocalSetup()` and verify they handle the new async bootstrap time (it now takes up to ~15s instead of being near-instant):

- `SetupViewModel.finishSetup()` — already async, just needs the loading state
- Any other callers should be audited

---

## Testing

### Unit tests:
1. `bootstrapResult_decodesValidJSON` — verify BootstrapResult CodingKeys
2. `bootstrapError_decodesErrorJSON` — verify BootstrapError CodingKeys
3. `xmlEscape_handlesSpecialCharacters` — verify &, <, > escaping
4. `generatePlist_containsExpectedFields` — verify plist has label, binary, port

### Integration tests (require real app bundle, may be manual):
1. Fresh VM: open DMG → drag to Applications → launch → complete wizard → verify server running
2. Existing install: launch app → verify existing config detected and reused
3. Port conflict: occupy 8400, launch app → verify next port selected
4. Permission error: read-only home dir → verify error message shown

---

## File Summary

| File | Action |
|------|--------|
| `app/Fawx/Services/LocalBootstrapService.swift` | NEW |
| `app/Fawx/ViewModels/AppState.swift` | MODIFY `completeLocalSetup()`, add `adoptAndConnect()` |
| `app/Fawx/ViewModels/SetupViewModel.swift` | ADD `isBootstrapping`, `bootstrapProgress`; MODIFY `finishSetup()` |
| `app/Fawx/Views/Shared/SetupWizard/ReadyStep.swift` | MODIFY headline to show progress/state |
