import XCTest
@testable import Fawx

final class FormattersTests: XCTestCase {
    func testCanonicalizeServerURLDefaultsSchemeAndStripsPathQueryAndFragment() {
        let url = canonicalizeServerURL("LOCALHOST:8400/v1/chat?foo=bar#frag")

        XCTAssertEqual(url, "http://localhost:8400")
    }

    func testCanonicalizeServerURLDefaultsRemoteHostsToHTTPS() {
        let url = canonicalizeServerURL("example.com:8400/v1/chat?foo=bar#frag")

        XCTAssertEqual(url, "https://example.com:8400")
    }

    func testCanonicalizeServerURLDefaultsBonjourHostsToHTTPS() {
        let url = canonicalizeServerURL("myserver.local:8400/v1/chat?foo=bar#frag")

        XCTAssertEqual(url, "https://myserver.local:8400")
    }

    func testCanonicalizeServerURLDefaultsLoopbackHostsToHTTP() {
        let url = canonicalizeServerURL("localhost:8400/v1/chat?foo=bar#frag")

        XCTAssertEqual(url, "http://localhost:8400")
    }

    func testCanonicalizeServerURLAllowsExplicitLocalNetworkHTTP() {
        let url = canonicalizeServerURL("http://192.168.1.10:8400")

        XCTAssertEqual(url, "http://192.168.1.10:8400")
    }

    func testCanonicalizeServerURLRejectsExplicitRemoteHTTP() {
        let url = canonicalizeServerURL("http://example.com:8400")

        XCTAssertNil(url)
    }

    func testCanonicalizeServerURLRejectsDoubleScheme() {
        let url = canonicalizeServerURL("http://https://example.com")

        XCTAssertNil(url)
    }

    func testAbbreviateModelNameDropsProviderPrefix() {
        let name = abbreviateModelName("anthropic/claude-opus-4-6")

        XCTAssertEqual(name, "claude-opus-4-6")
    }

    func testCompactModelNameTruncatesLongModelIdentifier() {
        let name = compactModelName("anthropic/claude-opus-4-6", limit: 12)

        XCTAssertEqual(name, "claude…s-4-6")
    }
}
