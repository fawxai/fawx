import Foundation
import XCTest
@testable import Fawx

final class LocalInstallConfigurationTests: XCTestCase {
    func testLoadFromParsesHTTPSection() async throws {
        let directoryURL = makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directoryURL) }

        let configURL = directoryURL.appendingPathComponent("config.toml", isDirectory: false)
        try """
        [http]
        host = "localhost"
        port = 9500
        bearer_token = "secret-token"
        """.write(to: configURL, atomically: true, encoding: .utf8)

        let configuration = await LocalInstallConfiguration.load(from: configURL)

        XCTAssertEqual(
            configuration,
            LocalInstallConfiguration(
                host: "localhost",
                port: 9500,
                bearerToken: "secret-token",
                dataDirectoryURL: directoryURL
            )
        )
    }

    func testLoadFromReturnsNilWhenBearerTokenIsMissing() async throws {
        let directoryURL = makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directoryURL) }

        let configURL = directoryURL.appendingPathComponent("config.toml", isDirectory: false)
        try """
        [http]
        host = "localhost"
        port = 9500
        """.write(to: configURL, atomically: true, encoding: .utf8)

        let configuration = await LocalInstallConfiguration.load(from: configURL)

        XCTAssertNil(configuration)
    }

    private func makeTemporaryDirectory() -> URL {
        let directoryURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)

        try? FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return directoryURL
    }
}
