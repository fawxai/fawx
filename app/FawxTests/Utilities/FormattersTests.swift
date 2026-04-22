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

    func testCanonicalizeServerURLAllowsExplicitBonjourHTTP() {
        let url = canonicalizeServerURL("http://pairing-host.local:8400")

        XCTAssertEqual(url, "http://pairing-host.local:8400")
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
        XCTAssertEqual(displayProviderName("fireworks"), "Fireworks AI")
    }

    func testDisplayAuthMethodNameFormatsKnownAuthTokens() {
        XCTAssertEqual(displayAuthMethodName("api_key"), "API Key")
        XCTAssertEqual(displayAuthMethodName("setup_token"), "Setup Token")
        XCTAssertEqual(displayAuthMethodName("oauth"), "OAuth")
    }

    func testModelDataTrustClassifiesDirectProviderModels() {
        let model = makeModel("anthropic/claude-sonnet-4-6", provider: "anthropic")

        XCTAssertEqual(model.dataTrust, .providerDirect)
        XCTAssertEqual(modelMetadataSummary(model), "Anthropic · API Key")
    }

    func testModelDataTrustClassifiesKnownRouterModels() {
        let model = makeModel(
            "accounts/fireworks/routers/kimi-k2p5-turbo",
            provider: "fireworks"
        )

        XCTAssertEqual(model.dataTrust, .knownRouter)
        XCTAssertEqual(modelMetadataSummary(model), "Fireworks AI · API Key")
    }

    func testModelDataTrustClassifiesFireworksRouterSeparatelyFromStandardModels() {
        let standardModel = makeModel("accounts/fireworks/models/glm-5", provider: "fireworks")
        let routerModel = makeModel(
            "accounts/fireworks/routers/kimi-k2p5-turbo",
            provider: "fireworks"
        )

        XCTAssertEqual(standardModel.dataTrust, .providerDirect)
        XCTAssertEqual(routerModel.dataTrust, .knownRouter)
    }

    func testModelDataTrustClassifiesOpenRouterFreeModelsAsUntrusted() {
        let model = makeModel("z-ai/glm-4.5-air:free", provider: "openrouter")

        XCTAssertEqual(model.dataTrust, .freeOrUntrusted)
        XCTAssertEqual(modelMetadataSummary(model), "OpenRouter · API Key")
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
                ModelSelectionProviderOption(id: "fireworks", title: "Fireworks AI")
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
            scope: .all,
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
            scope: .all,
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

    func testModelSelectionCatalogSearchMatchesDataTrustLabel() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "anthropic"),
                makeModel("z-ai/glm-4.5-air:free", provider: "openrouter")
            ],
            scope: .all,
            providerFilterID: ModelSelectionCatalog.allProvidersID,
            query: "Free/Untrusted"
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "openrouter",
                    title: "OpenRouter",
                    models: [makeModel("z-ai/glm-4.5-air:free", provider: "openrouter")]
                )
            ]
        )
    }

    func testModelSelectionCatalogFiltersByDataTrust() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "anthropic"),
                makeModel("accounts/fireworks/routers/kimi-k2p5-turbo", provider: "fireworks"),
                makeModel("z-ai/glm-4.5-air:free", provider: "openrouter")
            ],
            scope: .all,
            providerFilterID: ModelSelectionCatalog.allProvidersID,
            query: "",
            dataTrustFilter: .knownRouter
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "fireworks",
                    title: "Fireworks AI",
                    models: [makeModel("accounts/fireworks/routers/kimi-k2p5-turbo", provider: "fireworks")]
                )
            ]
        )
    }

    func testModelSelectionCatalogDefaultsToRecommendedScope() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter", recommended: true),
                makeModel("openai/gpt-4.1-mini", provider: "openrouter", recommended: false)
            ],
            scope: .recommended,
            providerFilterID: ModelSelectionCatalog.allProvidersID,
            query: ""
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "openrouter",
                    title: "OpenRouter",
                    models: [makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter", recommended: true)]
                )
            ]
        )
    }

    func testModelSelectionCatalogFiltersFavoriteScopeByPersistedModelIDs() {
        let sections = ModelSelectionCatalog.filteredSections(
            models: [
                makeModel("anthropic/claude-sonnet-4-6", provider: "openrouter", recommended: true),
                makeModel("openai/gpt-5.4", provider: "openrouter", recommended: true),
                makeModel("accounts/fireworks/models/llama-v3p1-8b-instruct", provider: "fireworks")
            ],
            scope: .favorites,
            favoriteModelIDs: ["openai/gpt-5.4"],
            providerFilterID: ModelSelectionCatalog.allProvidersID,
            query: ""
        )

        XCTAssertEqual(
            sections,
            [
                ModelSelectionSection(
                    providerID: "openrouter",
                    title: "OpenRouter",
                    models: [makeModel("openai/gpt-5.4", provider: "openrouter", recommended: true)]
                )
            ]
        )
    }

    func testDisplayModelNamePrefersCatalogDisplayName() {
        let name = displayModelName(
            makeModel(
                "accounts/fireworks/models/llama-v3p1-8b-instruct",
                provider: "fireworks",
                displayName: "Llama 3.1 8B Instruct"
            )
        )

        XCTAssertEqual(name, "Llama 3.1 8B Instruct")
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
        authMethod: String = "api_key",
        displayName: String? = nil,
        recommended: Bool = true
    ) -> ModelInfo {
        ModelInfo(
            modelID: modelID,
            provider: provider,
            authMethod: authMethod,
            displayName: displayName,
            recommended: recommended
        )
    }
}
