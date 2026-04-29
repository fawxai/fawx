import Foundation
import XCTest
@testable import Fawx

@MainActor
final class SkillsViewModelTests: XCTestCase {
    func testIsLoadedOnServerMatchesSkillsReturnedByServer() {
        let appState = AppState(startLoadingPersistedState: false)
        let sut = SkillsViewModel(appState: appState)
        sut.skills = [
            SkillSummary(name: "weather", description: nil, tools: [], capabilities: []),
        ]

        XCTAssertTrue(
            sut.isLoadedOnServer(
                MarketplaceSkillSummary(
                    name: "weather",
                    title: "Weather",
                    description: "Weather tools",
                    publisher: "Fawx",
                    signed: true
                )
            )
        )
        XCTAssertFalse(
            sut.isLoadedOnServer(
                MarketplaceSkillSummary(
                    name: "github",
                    title: "GitHub",
                    description: "GitHub tools",
                    publisher: "Fawx",
                    signed: true
                )
            )
        )
    }

    func testSkillSummaryDecodesLifecycleMetadataAndClassifiesBuiltIns() throws {
        let skill = try JSONDecoder().decode(
            SkillSummary.self,
            from: Data(
                """
                {
                    "name": "git",
                    "description": "Built-in git tools",
                    "tools": ["git_status"],
                    "capabilities": [],
                    "version": "1.0.0",
                    "source": "builtin",
                    "revision_hash": "abc123",
                    "activated_at_ms": 42,
                    "signature_status": "unsigned"
                }
                """.utf8
            )
        )

        XCTAssertEqual(skill.displayName, "Git")
        XCTAssertTrue(skill.isBuiltin)
        XCTAssertFalse(skill.isInstallableSkill)
        XCTAssertEqual(skill.loadedStatusLabel, "Built-in")
        XCTAssertEqual(skill.revisionHash, "abc123")
        XCTAssertEqual(skill.activatedAtMs, 42)
    }

    func testSkillSummaryTreatsStaleSourceAsOpaqueUpdateAvailableSignal() {
        let skill = SkillSummary(
            name: "github",
            description: "GitHub tools",
            tools: ["view_pr"],
            capabilities: ["network"],
            version: "1.0.0",
            source: "installed",
            revisionHash: "abc123",
            activatedAtMs: 42,
            signatureStatus: "unsigned",
            staleSource: "source manifest drift (source=hash-a, active=hash-b)"
        )

        XCTAssertTrue(skill.hasStaleSource)
        XCTAssertEqual(skill.loadedStatusLabel, "Update available")
        XCTAssertEqual(
            skill.staleSourceMessage,
            "Installed source changed since this revision was loaded. Restart the server to activate the latest skill version."
        )
    }

    func testSkillSummaryUsesInstalledLabelForNormalInstallableSkills() {
        let skill = SkillSummary(
            name: "github",
            description: "GitHub tools",
            tools: ["view_pr"],
            capabilities: ["network"]
        )

        XCTAssertEqual(skill.loadedStatusLabel, "Installed")
    }

    func testSkillSettingsFieldValidateRequiresValueWhenMarkedRequired() {
        let field = SkillSettingsField(
            key: "api_key",
            label: "API Key",
            fieldType: .secret,
            placeholder: nil,
            helpText: nil,
            required: true,
            minLength: nil,
            maxLength: nil,
            pattern: nil
        )

        XCTAssertEqual(field.validate(nil), "API Key is required.")
        XCTAssertEqual(field.validate(""), "API Key is required.")
    }

    func testSkillSettingsFieldValidateChecksBooleanStrings() {
        let field = SkillSettingsField(
            key: "safesearch",
            label: "Safe Search",
            fieldType: .boolean,
            placeholder: nil,
            helpText: nil,
            required: false,
            minLength: nil,
            maxLength: nil,
            pattern: nil
        )

        XCTAssertNil(field.validate("true"))
        XCTAssertNil(field.validate("false"))
        XCTAssertEqual(
            field.validate("yes"),
            "Safe Search must be either true or false."
        )
    }

    func testSkillSettingsFieldDecodesLegacyManifestTypeKey() throws {
        let field = try JSONDecoder().decode(
            SkillSettingsField.self,
            from: Data(
                """
                {
                    "key": "github_token",
                    "label": "GitHub Personal Access Token",
                    "type": "secret"
                }
                """.utf8
            )
        )

        XCTAssertEqual(field.fieldType, .secret)
        XCTAssertFalse(field.required)
    }

    func testSkillSettingsFieldDecodesFieldTypeKey() throws {
        let field = try JSONDecoder().decode(
            SkillSettingsField.self,
            from: Data(
                """
                {
                    "key": "github_token",
                    "label": "GitHub Personal Access Token",
                    "field_type": "secret"
                }
                """.utf8
            )
        )

        XCTAssertEqual(field.fieldType, .secret)
    }

    func testSkillSettingsFieldPrefersFieldTypeWhenBothKeysExist() throws {
        let field = try JSONDecoder().decode(
            SkillSettingsField.self,
            from: Data(
                """
                {
                    "key": "github_token",
                    "label": "GitHub Personal Access Token",
                    "field_type": "boolean",
                    "type": "secret"
                }
                """.utf8
            )
        )

        XCTAssertEqual(field.fieldType, .boolean)
    }

    func testSkillSettingsFieldDecodesMissingTypeKeyAsUnknown() throws {
        let field = try JSONDecoder().decode(
            SkillSettingsField.self,
            from: Data(
                """
                {
                    "key": "github_token",
                    "label": "GitHub Personal Access Token"
                }
                """.utf8
            )
        )

        switch field.fieldType {
        case .unknown(let rawType):
            XCTAssertEqual(rawType, "missing")
        default:
            XCTFail("expected unknown field type for missing type key")
        }
    }

    func testSkillSettingsFieldEncodesApiFieldTypeKey() throws {
        let field = SkillSettingsField(
            key: "github_token",
            label: "GitHub Personal Access Token",
            fieldType: .secret,
            placeholder: nil,
            helpText: nil,
            required: true,
            minLength: nil,
            maxLength: nil,
            pattern: nil
        )

        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(field)) as? [String: Any]
        XCTAssertEqual(json?["field_type"] as? String, "secret")
        XCTAssertNil(json?["type"])
    }

    func testBeginEditingSettingsLoadsDraftAndRedactsSecrets() async throws {
        let appState = try await makeConfiguredAppState { request in
            XCTAssertEqual(request.url?.path, "/v1/skills/brave-search/settings")

            return .json(
                """
                {
                    "skill_name": "brave-search",
                    "schema": {
                        "version": 1,
                        "fields": [
                            {
                                "key": "api_key",
                                "label": "API Key",
                                "field_type": "secret",
                                "required": true
                            },
                            {
                                "key": "region",
                                "label": "Region",
                                "field_type": "text",
                                "required": false
                            },
                            {
                                "key": "safesearch",
                                "label": "Safe Search",
                                "field_type": "boolean",
                                "required": false
                            }
                        ]
                    },
                    "values": [
                        {
                            "key": "api_key",
                            "value": null,
                            "is_secret": true,
                            "is_configured": true
                        },
                        {
                            "key": "region",
                            "value": "us-en",
                            "is_secret": false,
                            "is_configured": true
                        },
                        {
                            "key": "safesearch",
                            "value": "true",
                            "is_secret": false,
                            "is_configured": true
                        }
                    ]
                }
                """
            )
        }
        let sut = SkillsViewModel(appState: appState)

        await sut.beginEditingSettings(
            for: SkillSummary(
                name: "brave-search",
                description: "Search the web",
                tools: [],
                capabilities: []
            )
        )

        XCTAssertEqual(sut.editingSkillSettings?.skillName, "brave-search")
        XCTAssertEqual(sut.skillSettingsDraft["api_key"], "")
        XCTAssertEqual(sut.skillSettingsDraft["region"], "us-en")
        XCTAssertEqual(sut.skillSettingsDraft["safesearch"], "true")
        XCTAssertTrue(sut.clearedSkillSecretKeys.isEmpty)
        XCTAssertNil(sut.skillSettingsErrorMessage)
    }

    func testCancelEditingSettingsResetsEditorState() {
        let appState = AppState(startLoadingPersistedState: false)
        let sut = SkillsViewModel(appState: appState)

        sut.editingSkillSettings = makeSkillSettingsResponse(secretConfigured: true)
        sut.skillSettingsDraft = ["api_key": "", "region": "us-en"]
        sut.clearedSkillSecretKeys = ["api_key"]
        sut.skillSettingsErrorMessage = "Bad value"
        sut.savingSkillSettingsName = "brave-search"

        sut.cancelEditingSettings()

        XCTAssertNil(sut.editingSkillSettings)
        XCTAssertTrue(sut.skillSettingsDraft.isEmpty)
        XCTAssertTrue(sut.clearedSkillSecretKeys.isEmpty)
        XCTAssertNil(sut.skillSettingsErrorMessage)
        XCTAssertNil(sut.savingSkillSettingsName)
    }

    func testSaveEditingSettingsRejectsMissingRequiredSecretWhenUnconfigured() async {
        let appState = AppState(startLoadingPersistedState: false)
        let sut = SkillsViewModel(appState: appState)
        sut.editingSkillSettings = makeSkillSettingsResponse(secretConfigured: false)
        sut.skillSettingsDraft = ["api_key": "", "region": "us-en"]

        await sut.saveEditingSettings()

        XCTAssertEqual(sut.skillSettingsErrorMessage, "API Key is required.")
        XCTAssertEqual(sut.editingSkillSettings?.skillName, "brave-search")
    }

    func testSaveEditingSettingsSendsClearForSecretsAndClosesEditor() async throws {
        let capturedRequestBody = CapturedRequestBody()
        let appState = try await makeConfiguredAppState { request in
            XCTAssertEqual(request.httpMethod, "PUT")
            XCTAssertEqual(request.url?.path, "/v1/skills/brave-search/settings")
            capturedRequestBody.set(try request.bodyDataForTesting())

            return .json(
                """
                {
                    "updated": true,
                    "settings": {
                        "skill_name": "brave-search",
                        "schema": {
                            "version": 1,
                            "fields": [
                                {
                                    "key": "api_key",
                                    "label": "API Key",
                                    "field_type": "secret",
                                    "required": true
                                },
                                {
                                    "key": "region",
                                    "label": "Region",
                                    "field_type": "text",
                                    "required": false
                                }
                            ]
                        },
                        "values": [
                            {
                                "key": "api_key",
                                "value": null,
                                "is_secret": true,
                                "is_configured": false
                            },
                            {
                                "key": "region",
                                "value": "global",
                                "is_secret": false,
                                "is_configured": true
                            }
                        ]
                    }
                }
                """
            )
        }
        let sut = SkillsViewModel(appState: appState)
        sut.editingSkillSettings = makeSkillSettingsResponse(
            secretConfigured: true,
            secretRequired: false
        )
        sut.skillSettingsDraft = ["api_key": "", "region": "global"]
        sut.clearSecretSetting("api_key")

        await sut.saveEditingSettings()

        let capturedBody = capturedRequestBody.value()
        let body = try XCTUnwrap(capturedBody)
        let json = try XCTUnwrap(
            JSONSerialization.jsonObject(with: body) as? [String: Any]
        )
        let values = try XCTUnwrap(json["values"] as? [[String: Any]])
        let apiKeyUpdate = try XCTUnwrap(values.first(where: { ($0["key"] as? String) == "api_key" }))
        let regionUpdate = try XCTUnwrap(values.first(where: { ($0["key"] as? String) == "region" }))

        XCTAssertTrue(apiKeyUpdate["value"] is NSNull)
        XCTAssertEqual(regionUpdate["value"] as? String, "global")
        XCTAssertNil(sut.editingSkillSettings)
        XCTAssertTrue(sut.skillSettingsDraft.isEmpty)
        XCTAssertTrue(sut.clearedSkillSecretKeys.isEmpty)
    }

    func testSkillSettingsFieldDecodesUnknownFieldType() throws {
        let field = try JSONDecoder().decode(
            SkillSettingsField.self,
            from: Data(
                """
                {
                    "key": "threshold",
                    "label": "Threshold",
                    "field_type": "number",
                    "required": false
                }
                """.utf8
            )
        )

        switch field.fieldType {
        case .unknown(let rawType):
            XCTAssertEqual(rawType, "number")
        default:
            XCTFail("expected unknown field type")
        }
    }

    private func makeConfiguredAppState(
        responder: @escaping MockSkillsURLProtocolStore.Responder
    ) async throws -> AppState {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockSkillsURLProtocol.self]
        let session = URLSession(configuration: configuration)
        let client = FawxClient(
            restSession: session,
            streamSession: session
        )
        let persistence = AppStatePersistence(
            defaultsSuiteName: "SkillsViewModelTests.\(UUID().uuidString)",
            keychainService: "ai.fawx.app.skills-tests.\(UUID().uuidString)",
            localInstallLoader: { nil }
        )
        let appState = AppState(
            persistence: persistence,
            client: client,
            startLoadingPersistedState: false
        )

        MockSkillsURLProtocol.setResponder(responder)
        try await appState.savePairing(
            serverURLString: "https://skills.example.com:8400",
            token: "skills-token",
            deviceName: "Skill Test Device",
            connectionMode: .remote
        )

        return appState
    }

    private func makeSkillSettingsResponse(
        secretConfigured: Bool,
        secretRequired: Bool = true
    ) -> SkillSettingsResponse {
        SkillSettingsResponse(
            skillName: "brave-search",
            schema: SkillSettingsSchema(
                version: 1,
                fields: [
                    SkillSettingsField(
                        key: "api_key",
                        label: "API Key",
                        fieldType: .secret,
                        placeholder: nil,
                        helpText: nil,
                        required: secretRequired,
                        minLength: nil,
                        maxLength: nil,
                        pattern: nil
                    ),
                    SkillSettingsField(
                        key: "region",
                        label: "Region",
                        fieldType: .text,
                        placeholder: nil,
                        helpText: nil,
                        required: false,
                        minLength: nil,
                        maxLength: nil,
                        pattern: nil
                    ),
                ]
            ),
            values: [
                SkillSettingValue(
                    key: "api_key",
                    value: nil,
                    isSecret: true,
                    isConfigured: secretConfigured
                ),
                SkillSettingValue(
                    key: "region",
                    value: "us-en",
                    isSecret: false,
                    isConfigured: true
                ),
            ]
        )
    }
}

private final class CapturedRequestBody: @unchecked Sendable {
    private let lock = NSLock()
    private var body: Data?

    func set(_ body: Data?) {
        lock.lock()
        defer { lock.unlock() }
        self.body = body
    }

    func value() -> Data? {
        lock.lock()
        defer { lock.unlock() }
        return body
    }
}

private final class MockSkillsURLProtocol: URLProtocol, @unchecked Sendable {
    private static let store = MockSkillsURLProtocolStore()

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        do {
            let (response, data) = try Self.store.response(for: request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}

    static func setResponder(_ responder: @escaping MockSkillsURLProtocolStore.Responder) {
        store.setResponder(responder)
    }

    static func recordedRequests() -> [URLRequest] {
        store.recordedRequests()
    }
}

private final class MockSkillsURLProtocolStore: @unchecked Sendable {
    typealias Responder = @Sendable (URLRequest) throws -> MockSkillsResponse

    private let lock = NSLock()

    private var responder: Responder?
    private var requests: [URLRequest] = []

    func setResponder(_ responder: @escaping Responder) {
        lock.lock()
        defer { lock.unlock() }
        self.responder = responder
        requests = []
    }

    func response(for request: URLRequest) throws -> (HTTPURLResponse, Data) {
        lock.lock()
        defer { lock.unlock() }
        requests.append(request)

        guard let responder else {
            throw MockSkillsProtocolError.missingResponder
        }

        let response = try responder(request)
        guard let url = request.url else {
            throw MockSkillsProtocolError.missingURL
        }
        guard let httpResponse = HTTPURLResponse(
            url: url,
            statusCode: response.statusCode,
            httpVersion: nil,
            headerFields: ["Content-Type": "application/json"]
        ) else {
            throw MockSkillsProtocolError.invalidResponse
        }

        return (httpResponse, response.body)
    }

    func recordedRequests() -> [URLRequest] {
        lock.lock()
        defer { lock.unlock() }
        return requests
    }
}

private struct MockSkillsResponse: Sendable {
    let statusCode: Int
    let body: Data

    init(statusCode: Int, body: Data = Data("{}".utf8)) {
        self.statusCode = statusCode
        self.body = body
    }

    static func json(_ body: String, statusCode: Int = 200) -> Self {
        Self(statusCode: statusCode, body: Data(body.utf8))
    }
}

private enum MockSkillsProtocolError: Error {
    case invalidResponse
    case missingResponder
    case missingURL
    case unreadableRequestBody
}

private extension URLRequest {
    func bodyDataForTesting() throws -> Data? {
        if let httpBody {
            return httpBody
        }

        guard let httpBodyStream else {
            return nil
        }

        return try Data(readingRequestBodyFrom: httpBodyStream)
    }
}

private extension Data {
    init(readingRequestBodyFrom stream: InputStream) throws {
        stream.open()
        defer { stream.close() }

        self.init()

        let bufferSize = 4096
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { buffer.deallocate() }

        while stream.hasBytesAvailable {
            let bytesRead = stream.read(buffer, maxLength: bufferSize)
            if bytesRead < 0 {
                throw stream.streamError ?? MockSkillsProtocolError.unreadableRequestBody
            }
            if bytesRead == 0 {
                break
            }

            append(buffer, count: bytesRead)
        }
    }
}
