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

    func testCanonicalizeServerURLPreservesExplicitHTTPS() {
        let url = canonicalizeServerURL("https://Example.com:8400/path")

        XCTAssertEqual(url, "https://example.com:8400")
    }

    func testAbbreviateModelNameDropsProviderPrefix() {
        let name = abbreviateModelName("anthropic/claude-opus-4-6")

        XCTAssertEqual(name, "claude-opus-4-6")
    }

    func testCompactModelNameTruncatesLongModelIdentifier() {
        let name = compactModelName("anthropic/claude-opus-4-6", limit: 12)

        XCTAssertEqual(name, "claude…s-4-6")
    }

    func testDisplayProviderNamePreservesKnownBrandCasing() {
        XCTAssertEqual(displayProviderName("openai"), "OpenAI")
        XCTAssertEqual(displayProviderName("openrouter"), "OpenRouter")
        XCTAssertEqual(displayProviderName("fireworks"), "Fireworks")
    }

    func testDisplayAuthMethodNameFormatsKnownAuthTokens() {
        XCTAssertEqual(displayAuthMethodName("api_key"), "API Key")
        XCTAssertEqual(displayAuthMethodName("setup_token"), "Setup Token")
        XCTAssertEqual(displayAuthMethodName("oauth"), "OAuth")
    }

    func testModelSelectionCatalogBuildsProviderOptionsFromAvailableModels() {
        let options = ModelSelectionCatalog.providerOptions(
            for: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter"),
                makeModel("openai/gpt-5.4", provider: "openrouter"),
                makeModel("accounts/fireworks/models/llama-v3p1-8b-instruct", provider: "fireworks")
            ]
        )

        XCTAssertEqual(
            options,
            [
                ModelSelectionProviderOption(id: ModelSelectionCatalog.allProvidersID, title: "All Providers"),
                ModelSelectionProviderOption(id: "openrouter", title: "OpenRouter"),
                ModelSelectionProviderOption(id: "fireworks", title: "Fireworks")
            ]
        )
    }

    func testModelSelectionCatalogFiltersSectionsByProviderAndQuery() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter"),
                makeModel("openai/gpt-5.4", provider: "openrouter"),
                makeModel("accounts/fireworks/models/llama-v3p1-8b-instruct", provider: "fireworks")
            ],
            providerFilterID: "openrouter",
            query: "gpt"
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "openrouter",
                    title: "OpenRouter",
                    models: [makeModel("openai/gpt-5.4", provider: "openrouter")]
                )
            ]
        )
    }

    func testModelSelectionCatalogSearchMatchesDisplayedAuthMethodLabel() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter", authMethod: "api_key")
            ],
            providerFilterID: ModelSelectionCatalog.allProvidersID,
            query: "API Key"
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "openrouter",
                    title: "OpenRouter",
                    models: [makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter", authMethod: "api_key")]
                )
            ]
        )
    }

    func testDisplayThinkingLevelMarksAdaptiveAsDefaultForClaude46Models() {
        let label = displayThinkingLevel(.adaptive, modelID: "anthropic/claude-opus-4-6-20260301")

        XCTAssertEqual(label, "Adaptive (default)")
    }

    func testDisplayThinkingLevelLeavesAdaptiveUnchangedForNonClaude46Models() {
        let label = displayThinkingLevel(.adaptive, modelID: "openai/gpt-5.4")

        XCTAssertEqual(label, "Adaptive")
    }

    private func makeModel(
        _ modelID: String,
        provider: String,
        authMethod: String = "api_key"
    ) -> ModelInfo {
        ModelInfo(modelID: modelID, provider: provider, authMethod: authMethod)
    }
}
