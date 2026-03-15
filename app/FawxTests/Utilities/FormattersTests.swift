import XCTest
@testable import Fawx

final class FormattersTests: XCTestCase {
    func testCanonicalizeServerURLDefaultsSchemeAndStripsPathQueryAndFragment() {
        let url = canonicalizeServerURL("LOCALHOST:8400/v1/chat?foo=bar#frag")

        XCTAssertEqual(url, "http://localhost:8400")
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
