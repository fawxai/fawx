import XCTest
@testable import Fawx

final class KeychainHelperTests: XCTestCase {
    func testSaveTokenRoundTripsValue() throws {
        let account = uniqueAccount()
        let service = uniqueService()
        defer { try? KeychainHelper.deleteToken(forServer: account, service: service) }

        try KeychainHelper.saveToken("token-1", forServer: account, service: service)

        let token = try KeychainHelper.token(forServer: account, service: service)

        XCTAssertEqual(token, "token-1")
    }

    func testSaveTokenUpdatesExistingValue() throws {
        let account = uniqueAccount()
        let service = uniqueService()
        defer { try? KeychainHelper.deleteToken(forServer: account, service: service) }

        try KeychainHelper.saveToken("token-1", forServer: account, service: service)
        try KeychainHelper.saveToken("token-2", forServer: account, service: service)

        let token = try KeychainHelper.token(forServer: account, service: service)

        XCTAssertEqual(token, "token-2")
    }

    func testDeleteTokenRemovesStoredValue() throws {
        let account = uniqueAccount()
        let service = uniqueService()

        try KeychainHelper.saveToken("token-1", forServer: account, service: service)
        try KeychainHelper.deleteToken(forServer: account, service: service)

        let token = try KeychainHelper.token(forServer: account, service: service)

        XCTAssertNil(token)
    }

    private func uniqueAccount() -> String {
        "server-\(UUID().uuidString)"
    }

    private func uniqueService() -> String {
        "ai.fawx.app.tests.\(UUID().uuidString)"
    }
}
