import Foundation

struct LocalInstallConfiguration: Sendable, Equatable {
    let host: String
    let port: Int
    let bearerToken: String
    let dataDirectoryURL: URL

    var baseURLString: String {
        "http://\(host):\(port)"
    }

    var logFileURL: URL {
#if os(macOS)
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Logs", isDirectory: true)
            .appendingPathComponent("Fawx", isDirectory: true)
            .appendingPathComponent("server.log", isDirectory: false)
#else
        FileManager.default.temporaryDirectory
            .appendingPathComponent("Fawx", isDirectory: true)
            .appendingPathComponent("server.log", isDirectory: false)
#endif
    }

    static func loadDefault() -> LocalInstallConfiguration? {
#if os(macOS)
        let dataDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".fawx", isDirectory: true)
        return load(from: dataDir.appendingPathComponent("config.toml", isDirectory: false))
#else
        nil
#endif
    }

    static func load(from configURL: URL) -> LocalInstallConfiguration? {
        guard let rawConfig = try? String(contentsOf: configURL) else {
            return nil
        }

        var currentSection = ""
        var host = "127.0.0.1"
        var port = 8400
        var bearerToken: String?

        for rawLine in rawConfig.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !line.isEmpty, !line.hasPrefix("#") else {
                continue
            }

            if line.hasPrefix("[") && line.hasSuffix("]") {
                currentSection = String(line.dropFirst().dropLast())
                continue
            }

            guard let separatorIndex = line.firstIndex(of: "=") else {
                continue
            }

            let key = line[..<separatorIndex]
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased()
            let value = line[line.index(after: separatorIndex)...]
                .trimmingCharacters(in: .whitespacesAndNewlines)

            switch (currentSection.lowercased(), key) {
            case ("http", "host"):
                let parsedHost = parseTOMLString(value)
                if !parsedHost.isEmpty {
                    host = parsedHost
                }
            case ("http", "port"):
                if let parsedPort = Int(value), parsedPort > 0 {
                    port = parsedPort
                }
            case ("http", "bearer_token"):
                let parsedToken = parseTOMLString(value)
                if !parsedToken.isEmpty {
                    bearerToken = parsedToken
                }
            default:
                continue
            }
        }

        guard let bearerToken, !bearerToken.isEmpty else {
            return nil
        }

        return LocalInstallConfiguration(
            host: host,
            port: port,
            bearerToken: bearerToken,
            dataDirectoryURL: configURL.deletingLastPathComponent()
        )
    }

    private static func parseTOMLString(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count >= 2, trimmed.hasPrefix("\""), trimmed.hasSuffix("\"") else {
            return trimmed
        }
        return String(trimmed.dropFirst().dropLast())
    }
}
