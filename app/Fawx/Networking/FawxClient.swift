import Foundation

actor FawxClient {
    private static let streamIdleTimeout: TimeInterval = 120
    private static let streamResourceTimeout: TimeInterval = 600

    private var baseURL: URL?
    private var bearerToken: String?

    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()
    private let restSession: URLSession
    private let streamSession: URLSession

    init(baseURL: URL? = nil, bearerToken: String? = nil) {
        self.baseURL = baseURL
        self.bearerToken = bearerToken

        let restConfiguration = URLSessionConfiguration.default
        restConfiguration.timeoutIntervalForRequest = 15
        restConfiguration.timeoutIntervalForResource = 30
        self.restSession = URLSession(configuration: restConfiguration)

        let streamConfiguration = URLSessionConfiguration.default
        streamConfiguration.timeoutIntervalForRequest = Self.streamIdleTimeout
        streamConfiguration.timeoutIntervalForResource = Self.streamResourceTimeout
        self.streamSession = URLSession(configuration: streamConfiguration)
    }

    func updateConfiguration(baseURL: URL?, bearerToken: String?) {
        self.baseURL = baseURL
        self.bearerToken = bearerToken
    }

    func health() async throws -> HealthResponse {
        try await performRequest(path: "/health", authRequired: false, decodeAs: HealthResponse.self)
    }

    func pair(code: String, deviceName: String) async throws -> PairingExchangeResponse {
        let body = try encoder.encode(PairingExchangeBody(code: code, deviceName: deviceName))
        return try await performRequest(
            path: "/v1/pair",
            method: "POST",
            authRequired: false,
            bodyData: body,
            decodeAs: PairingExchangeResponse.self
        )
    }

    func adoptLocalDevice(deviceName: String) async throws -> PairingExchangeResponse {
        let body = try encoder.encode(LocalAdoptBody(deviceName: deviceName))
        return try await performSetupRequest(
            path: "/v1/setup/adopt-local",
            method: "POST",
            bodyData: body,
            decodeAs: PairingExchangeResponse.self
        )
    }

    func setupStatus() async throws -> SetupStatusResponse {
        try await performSetupRequest(path: "/v1/setup/status", decodeAs: SetupStatusResponse.self)
    }

    func runtimeStatus() async throws -> LocalServerRuntimeStatus {
        try await performRequest(path: "/v1/server/status", decodeAs: LocalServerRuntimeStatus.self)
    }

    func restartServer() async throws -> ServerRestartControlResponse {
        try await performRequest(
            path: "/v1/server/restart",
            method: "POST",
            bodyData: Data(),
            decodeAs: ServerRestartControlResponse.self
        )
    }

    func stopServer() async throws -> ServerStopControlResponse {
        try await performRequest(
            path: "/v1/server/stop",
            method: "POST",
            bodyData: Data(),
            decodeAs: ServerStopControlResponse.self
        )
    }

    func launchAgentStatus() async throws -> LaunchAgentStatusResponse {
        try await performSetupRequest(
            path: "/v1/launchagent/status",
            decodeAs: LaunchAgentStatusResponse.self
        )
    }

    func installLaunchAgent(autoStart: Bool = true) async throws -> LaunchAgentInstallResponse {
        let body = try encoder.encode(LaunchAgentInstallBody(autoStart: autoStart))
        return try await performSetupRequest(
            path: "/v1/launchagent/install",
            method: "POST",
            bodyData: body,
            decodeAs: LaunchAgentInstallResponse.self
        )
    }

    func uninstallLaunchAgent() async throws -> LaunchAgentUninstallResponse {
        try await performSetupRequest(
            path: "/v1/launchagent/uninstall",
            method: "POST",
            bodyData: Data(),
            decodeAs: LaunchAgentUninstallResponse.self
        )
    }

    func qrPairing() async throws -> QrPairingResponse {
        try await performSetupRequest(path: "/v1/pair/qr", decodeAs: QrPairingResponse.self)
    }

    func tailscaleCert(hostname: String) async throws -> TailscaleCertResponse {
        let body = try encoder.encode(TailscaleCertRequest(hostname: hostname))
        return try await performSetupRequest(
            path: "/v1/tailscale/cert",
            method: "POST",
            bodyData: body,
            decodeAs: TailscaleCertResponse.self
        )
    }

    func exchangeAnthropicSetupToken(
        _ setupToken: String,
        label: String = "Personal Claude subscription"
    ) async throws -> ProviderAuthActionResponse {
        let body = try encoder.encode(AnthropicSetupTokenBody(setupToken: setupToken, label: label))
        return try await performSetupRequest(
            path: "/v1/auth/anthropic/setup-token",
            method: "POST",
            bodyData: body,
            decodeAs: ProviderAuthActionResponse.self
        )
    }

    func storeAPIKey(
        provider: String,
        apiKey: String,
        label: String = "Manual key"
    ) async throws -> ProviderAuthActionResponse {
        let body = try encoder.encode(APIKeyStoreBody(apiKey: apiKey, label: label))
        return try await performSetupRequest(
            path: "/v1/auth/\(provider)/api-key",
            method: "POST",
            bodyData: body,
            decodeAs: ProviderAuthActionResponse.self
        )
    }

    func verifyProvider(
        _ provider: String,
        timeoutSeconds: Int = 10
    ) async throws -> ProviderVerificationResponse {
        let body = try encoder.encode(VerifyProviderBody(timeoutSeconds: timeoutSeconds))
        return try await performSetupRequest(
            path: "/v1/auth/\(provider)/verify",
            method: "POST",
            bodyData: body,
            decodeAs: ProviderVerificationResponse.self
        )
    }

    func deleteProvider(_ provider: String) async throws -> DeleteProviderResponse {
        try await performRequest(
            path: "/v1/auth/\(provider)",
            method: "DELETE",
            decodeAs: DeleteProviderResponse.self
        )
    }

    func patchConfig(changes: JSONValue) async throws -> ConfigPatchResponse {
        let body = try encoder.encode(ConfigPatchBody(changes: changes))
        return try await performRequest(
            path: "/v1/config",
            method: "PATCH",
            bodyData: body,
            decodeAs: ConfigPatchResponse.self
        )
    }

    func getPermissions() async throws -> PermissionsResponse {
        try await performRequest(path: "/v1/permissions", decodeAs: PermissionsResponse.self)
    }

    func patchPermissions(_ request: PermissionsPatchRequest) async throws -> PermissionsPatchResponse {
        do {
            return try await performPermissionsPatch(request)
        } catch let error as APIError where error.statusCode == 422 {
            guard let legacyRequest = request.legacyCompatibleRequest else {
                throw error
            }
            return try await performPermissionsPatch(legacyRequest)
        }
    }

    func respondToPermissionPrompt(
        id: String,
        decision: PermissionPromptDecision
    ) async throws {
        do {
            let body = try encoder.encode(PermissionPromptRespondBody(decision: decision))
            let _: JSONValue = try await performRequest(
                path: "/v1/permissions/prompts/\(id)/respond",
                method: "POST",
                bodyData: body,
                decodeAs: JSONValue.self
            )
        } catch let error as APIError
            where decision == .allowSession && error.statusCode == 422
        {
            let legacyBody = try encoder.encode(
                LegacyPermissionPromptRespondBody(decision: "allow", scope: "session")
            )
            let _: JSONValue = try await performRequest(
                path: "/v1/permissions/prompts/\(id)/respond",
                method: "POST",
                bodyData: legacyBody,
                decodeAs: JSONValue.self
            )
        }
    }

    func getSynthesis() async throws -> SynthesisResponse {
        try await performRequest(path: "/v1/synthesis", decodeAs: SynthesisResponse.self)
    }

    func setSynthesis(_ text: String, version: Int? = nil) async throws -> SetSynthesisResponse {
        let body = try encoder.encode(SetSynthesisRequest(synthesis: text, version: version))
        return try await performRequest(
            path: "/v1/synthesis",
            method: "PUT",
            bodyData: body,
            decodeAs: SetSynthesisResponse.self
        )
    }

    func clearSynthesis() async throws -> ClearSynthesisResponse {
        try await performRequest(
            path: "/v1/synthesis",
            method: "DELETE",
            decodeAs: ClearSynthesisResponse.self
        )
    }

    func getUsage() async throws -> UsageResponse {
        try await performRequest(path: "/v1/usage", decodeAs: UsageResponse.self)
    }

    func getTelemetryConsent() async throws -> TelemetryConsentResponse {
        try await performRequest(path: "/v1/telemetry/consent", decodeAs: TelemetryConsentResponse.self)
    }

    func patchTelemetryConsent(_ request: TelemetryConsentPatchRequest) async throws -> TelemetryConsentResponse {
        let body = try encoder.encode(request)
        return try await performRequest(
            path: "/v1/telemetry/consent",
            method: "PATCH",
            bodyData: body,
            decodeAs: TelemetryConsentResponse.self
        )
    }

    func serverStatus() async throws -> ServerStatusResponse {
        try await performRequest(
            candidatePaths: ["/status", "/v1/status"],
            authRequired: true,
            decodeAs: ServerStatusResponse.self
        )
    }

    func serverConfig() async throws -> JSONValue {
        try await performRequest(
            candidatePaths: ["/config", "/v1/config"],
            authRequired: true,
            decodeAs: JSONValue.self
        )
    }

    func updateServerConfig(_ payload: JSONValue) async throws -> JSONValue {
        let body = try encoder.encode(payload)
        return try await performRequest(
            candidatePaths: ["/config", "/v1/config"],
            method: "POST",
            authRequired: true,
            bodyData: body,
            decodeAs: JSONValue.self
        )
    }

    func listSessions(kind: SessionKind? = nil, limit: Int? = nil) async throws -> SessionsResponse {
        var queryItems: [URLQueryItem] = []
        if let kind {
            queryItems.append(.init(name: "kind", value: kind.rawValue))
        }
        if let limit {
            queryItems.append(.init(name: "limit", value: String(limit)))
        }

        return try await performRequest(
            path: "/v1/sessions",
            queryItems: queryItems,
            decodeAs: SessionsResponse.self
        )
    }

    func createSession(label: String? = nil, model: String? = nil) async throws -> Session {
        let body = try encoder.encode(CreateSessionBody(label: label, model: model))
        return try await performRequest(
            path: "/v1/sessions",
            method: "POST",
            bodyData: body,
            decodeAs: Session.self
        )
    }

    func session(id: String) async throws -> Session {
        try await performRequest(path: "/v1/sessions/\(id)", decodeAs: Session.self)
    }

    func deleteSession(id: String) async throws -> DeleteSessionResponse {
        try await performRequest(
            path: "/v1/sessions/\(id)",
            method: "DELETE",
            decodeAs: DeleteSessionResponse.self
        )
    }

    func clearSession(id: String) async throws -> ClearSessionResponse {
        try await performRequest(
            path: "/v1/sessions/\(id)/clear",
            method: "POST",
            bodyData: Data(),
            decodeAs: ClearSessionResponse.self
        )
    }

    func sessionMessages(id: String, limit: Int = 200) async throws -> MessagesResponse {
        try await performRequest(
            path: "/v1/sessions/\(id)/messages",
            queryItems: [.init(name: "limit", value: String(limit))],
            decodeAs: MessagesResponse.self
        )
    }

    func sendMessage(
        sessionID: String,
        message: String,
        images: [ImagePayload] = []
    ) async throws -> MessageResponse {
        let body = try encoder.encode(SendMessageBody(message: message, images: images, sessionID: nil))
        return try await performRequest(
            path: "/v1/sessions/\(sessionID)/messages",
            method: "POST",
            bodyData: body,
            decodeAs: MessageResponse.self
        )
    }

    func sendMessageStream(
        sessionID: String,
        message: String,
        images: [ImagePayload] = []
    ) throws -> AsyncThrowingStream<SSEEvent, Error> {
        let body = try encoder.encode(SendMessageBody(message: message, images: images, sessionID: nil))
        let request = try makeRequest(
            path: "/v1/sessions/\(sessionID)/messages",
            method: "POST",
            authRequired: true,
            bodyData: body,
            acceptsEventStream: true
        )
        let session = streamSession

        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    let (bytes, response) = try await session.bytes(for: request)
                    guard let http = response as? HTTPURLResponse else {
                        throw APIError.invalidResponse
                    }

                    if !(200 ..< 300).contains(http.statusCode) {
                        var data = Data()
                        for try await byte in bytes {
                            data.append(byte)
                        }
                        throw Self.httpError(statusCode: http.statusCode, data: data)
                    }

                    var parser = SSEParser()
                    for try await line in bytes.lines {
                        let events = try parser.parseLine(line)
                        for event in events {
                            continuation.yield(event)
                        }
                    }

                    for event in try parser.finish() {
                        continuation.yield(event)
                    }
                    continuation.finish()
                } catch is CancellationError {
                    continuation.finish(throwing: CancellationError())
                } catch {
                    continuation.finish(throwing: error)
                }
            }

            continuation.onTermination = { _ in
                task.cancel()
            }
        }
    }

    func sessionContext(id: String) async throws -> ContextInfo {
        try await performRequest(path: "/v1/sessions/\(id)/context", decodeAs: ContextInfo.self)
    }

    func listModels() async throws -> ModelCatalogResponse {
        try await performRequest(path: "/v1/models", decodeAs: ModelCatalogResponse.self)
    }

    func setModel(_ modelID: String) async throws -> SetModelResponse {
        let body = try encoder.encode(SetModelBody(model: modelID))
        return try await performRequest(
            path: "/v1/model",
            method: "PUT",
            bodyData: body,
            decodeAs: SetModelResponse.self
        )
    }

    func thinking() async throws -> ThinkingConfig {
        try await performRequest(path: "/v1/thinking", decodeAs: ThinkingConfig.self)
    }

    func setThinking(_ level: ThinkingLevel) async throws -> SetThinkingResponse {
        let body = try encoder.encode(SetThinkingBody(level: level.rawValue))
        return try await performRequest(
            path: "/v1/thinking",
            method: "PUT",
            bodyData: body,
            decodeAs: SetThinkingResponse.self
        )
    }

    func skills() async throws -> SkillsResponse {
        try await performRequest(path: "/v1/skills", decodeAs: SkillsResponse.self)
    }

    func searchSkills(query: String) async throws -> SkillSearchResponse {
        try await performRequest(
            path: "/v1/skills/search",
            queryItems: [.init(name: "q", value: query)],
            decodeAs: SkillSearchResponse.self
        )
    }

    func installSkill(name: String) async throws {
        let body = try encoder.encode(InstallSkillRequest(name: name))
        let _: JSONValue = try await performRequest(
            path: "/v1/skills/install",
            method: "POST",
            bodyData: body,
            decodeAs: JSONValue.self
        )
    }

    func removeSkill(name: String) async throws {
        let _: JSONValue = try await performRequest(
            path: "/v1/skills/\(name)",
            method: "DELETE",
            decodeAs: JSONValue.self
        )
    }

    func updateSkillPermissions(
        name: String,
        capabilities: [String]
    ) async throws -> UpdateSkillPermissionsResponse {
        let body = try encoder.encode(UpdateSkillPermissionsRequest(capabilities: capabilities))
        return try await performRequest(
            path: "/v1/skills/\(name)",
            method: "PATCH",
            bodyData: body,
            decodeAs: UpdateSkillPermissionsResponse.self
        )
    }

    func fleetOverview() async throws -> FleetOverviewResponse {
        try await performRequest(path: "/v1/fleet/overview", decodeAs: FleetOverviewResponse.self)
    }

    func fleetNodes() async throws -> FleetNodesResponse {
        try await performRequest(path: "/v1/fleet/nodes", decodeAs: FleetNodesResponse.self)
    }

    func fleetNode(id: String) async throws -> FleetNodeDetailResponse {
        try await performRequest(path: "/v1/fleet/nodes/\(id)", decodeAs: FleetNodeDetailResponse.self)
    }

    func dispatchFleetTask(
        nodeID: String,
        task: String,
        priority: String = "normal"
    ) async throws -> FleetDispatchTaskResponse {
        let body = try encoder.encode(FleetDispatchTaskBody(task: task, priority: priority))
        return try await performRequest(
            path: "/v1/fleet/nodes/\(nodeID)/tasks",
            method: "POST",
            bodyData: body,
            decodeAs: FleetDispatchTaskResponse.self
        )
    }

    func experiments() async throws -> ExperimentsListResponse {
        try await performRequest(path: "/v1/experiments", decodeAs: ExperimentsListResponse.self)
    }

    func experiment(id: String) async throws -> ExperimentDetail {
        try await performRequest(path: "/v1/experiments/\(id)", decodeAs: ExperimentDetail.self)
    }

    func experimentResults(id: String) async throws -> ExperimentResultsResponse {
        try await performRequest(
            path: "/v1/experiments/\(id)/results",
            decodeAs: ExperimentResultsResponse.self
        )
    }

    func stopExperiment(id: String) async throws -> StopExperimentResponse {
        try await performRequest(
            path: "/v1/experiments/\(id)/stop",
            method: "POST",
            bodyData: Data(),
            decodeAs: StopExperimentResponse.self
        )
    }

    func gitStatus() async throws -> GitStatusResponse {
        try await performRequest(path: "/v1/git/status", decodeAs: GitStatusResponse.self)
    }

    func gitLog(limit: Int = 10) async throws -> GitLogResponse {
        try await performRequest(
            path: "/v1/git/log",
            queryItems: [.init(name: "limit", value: String(limit))],
            decodeAs: GitLogResponse.self
        )
    }

    func gitDiff() async throws -> GitDiffResponse {
        try await performRequest(path: "/v1/git/diff", decodeAs: GitDiffResponse.self)
    }

    func gitStage(paths: [String]) async throws -> GitStageResponse {
        let body = try encoder.encode(GitPathsRequest(paths: paths))
        return try await performRequest(
            path: "/v1/git/stage",
            method: "POST",
            bodyData: body,
            decodeAs: GitStageResponse.self
        )
    }

    func gitStageAll() async throws -> GitStageResponse {
        let body = try encoder.encode(EmptyJSONRequest())
        return try await performRequest(
            path: "/v1/git/stage",
            method: "POST",
            bodyData: body,
            decodeAs: GitStageResponse.self
        )
    }

    func gitUnstage(paths: [String]) async throws -> GitUnstageResponse {
        let body = try encoder.encode(GitPathsRequest(paths: paths))
        return try await performRequest(
            path: "/v1/git/unstage",
            method: "POST",
            bodyData: body,
            decodeAs: GitUnstageResponse.self
        )
    }

    func gitUnstageAll() async throws -> GitUnstageResponse {
        let body = try encoder.encode(EmptyJSONRequest())
        return try await performRequest(
            path: "/v1/git/unstage",
            method: "POST",
            bodyData: body,
            decodeAs: GitUnstageResponse.self
        )
    }

    func gitCommit(message: String) async throws -> GitCommitResponse {
        let body = try encoder.encode(GitCommitRequestBody(message: message))
        return try await performRequest(
            path: "/v1/git/commit",
            method: "POST",
            bodyData: body,
            decodeAs: GitCommitResponse.self
        )
    }

    func gitPush() async throws -> GitPushResponse {
        try await performRequest(
            path: "/v1/git/push",
            method: "POST",
            bodyData: Data(),
            decodeAs: GitPushResponse.self
        )
    }

    func gitPull() async throws -> GitPullResponse {
        try await performRequest(
            path: "/v1/git/pull",
            method: "POST",
            bodyData: Data(),
            decodeAs: GitPullResponse.self
        )
    }

    func gitFetch() async throws -> GitFetchResponse {
        try await performRequest(
            path: "/v1/git/fetch",
            method: "POST",
            bodyData: Data(),
            decodeAs: GitFetchResponse.self
        )
    }

    func authProviders() async throws -> AuthProvidersResponse {
        try await performRequest(
            candidatePaths: ["/v1/auth/status", "/v1/auth"],
            decodeAs: AuthProvidersResponse.self
        )
    }

    func oauthStart(provider: String) async throws -> OAuthStartResponse {
        try await performSetupRequest(
            path: "/v1/auth/\(provider)/oauth-start",
            decodeAs: OAuthStartResponse.self
        )
    }

    func oauthCallback(
        provider: String,
        code: String,
        flowToken: String
    ) async throws -> OAuthCallbackResponse {
        let body = try encoder.encode(OAuthCallbackRequest(code: code, flowToken: flowToken))
        return try await performSetupRequest(
            path: "/v1/auth/\(provider)/oauth-callback",
            method: "POST",
            bodyData: body,
            decodeAs: OAuthCallbackResponse.self
        )
    }

    func sendTopLevelMessage(
        _ message: String,
        images: [ImagePayload] = [],
        sessionID: String? = nil
    ) async throws -> MessageResponse {
        let body = try encoder.encode(SendMessageBody(message: message, images: images, sessionID: sessionID))
        return try await performRequest(
            path: "/message",
            method: "POST",
            bodyData: body,
            decodeAs: MessageResponse.self
        )
    }

    private func performRequest<Response: Decodable>(
        candidatePaths: [String],
        method: String = "GET",
        queryItems: [URLQueryItem] = [],
        authRequired: Bool = true,
        bodyData: Data? = nil,
        decodeAs: Response.Type
    ) async throws -> Response {
        var lastError: Error?

        for path in candidatePaths {
            do {
                return try await performRequest(
                    path: path,
                    method: method,
                    queryItems: queryItems,
                    authRequired: authRequired,
                    bodyData: bodyData,
                    decodeAs: Response.self
                )
            } catch APIError.httpStatus(let code, _) where code == 404 || code == 405 {
                lastError = APIError.httpStatus(code, nil)
                continue
            } catch {
                throw error
            }
        }

        throw lastError ?? APIError.invalidResponse
    }

    private func performRequest<Response: Decodable>(
        path: String,
        method: String = "GET",
        queryItems: [URLQueryItem] = [],
        authRequired: Bool = true,
        bodyData: Data? = nil,
        decodeAs: Response.Type
    ) async throws -> Response {
        let request = try makeRequest(
            path: path,
            method: method,
            queryItems: queryItems,
            authRequired: authRequired,
            bodyData: bodyData
        )
        let (data, response) = try await restSession.data(for: request)
        try Self.validate(response: response, data: data)

        do {
            return try decoder.decode(Response.self, from: data)
        } catch {
            throw APIError.decoding(error.localizedDescription)
        }
    }

    private func performSetupRequest<Response: Decodable>(
        path: String,
        method: String = "GET",
        queryItems: [URLQueryItem] = [],
        bodyData: Data? = nil,
        decodeAs: Response.Type
    ) async throws -> Response {
        if let bearerToken, !bearerToken.isEmpty {
            do {
                return try await performRequest(
                    path: path,
                    method: method,
                    queryItems: queryItems,
                    authRequired: true,
                    bodyData: bodyData,
                    decodeAs: Response.self
                )
            } catch APIError.httpStatus(let code, _) where code == 401 || code == 403 || code == 404 {
                // Fall back to the setup-mode public endpoint shape below.
            } catch APIError.authenticationRequired {
                // Fall through to the public setup endpoint shape below.
            }
        }

        return try await performRequest(
            path: path,
            method: method,
            queryItems: queryItems,
            authRequired: false,
            bodyData: bodyData,
            decodeAs: Response.self
        )
    }

    private func performPermissionsPatch(
        _ request: PermissionsPatchRequest
    ) async throws -> PermissionsPatchResponse {
        let body = try encoder.encode(request)
        return try await performRequest(
            path: "/v1/permissions",
            method: "PATCH",
            bodyData: body,
            decodeAs: PermissionsPatchResponse.self
        )
    }

    private func makeRequest(
        path: String,
        method: String,
        queryItems: [URLQueryItem] = [],
        authRequired: Bool,
        bodyData: Data? = nil,
        acceptsEventStream: Bool = false
    ) throws -> URLRequest {
        guard let url = try url(path: path, queryItems: queryItems) else {
            throw APIError.invalidURL(path)
        }

        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue(acceptsEventStream ? "text/event-stream" : "application/json", forHTTPHeaderField: "Accept")
        if let bodyData {
            request.httpBody = bodyData
        }

        if authRequired {
            guard let bearerToken, !bearerToken.isEmpty else {
                throw APIError.authenticationRequired
            }
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        }

        return request
    }

    private func url(path: String, queryItems: [URLQueryItem]) throws -> URL? {
        guard let baseURL else {
            throw APIError.notConfigured
        }
        guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
            throw APIError.invalidURL(baseURL.absoluteString)
        }

        let basePath = components.path == "/" ? "" : components.path
        components.path = basePath + (path.hasPrefix("/") ? path : "/" + path)
        components.queryItems = queryItems.isEmpty ? nil : queryItems
        return components.url
    }

    private static func validate(response: URLResponse, data: Data) throws {
        guard let http = response as? HTTPURLResponse else {
            throw APIError.invalidResponse
        }

        guard (200 ..< 300).contains(http.statusCode) else {
            throw httpError(statusCode: http.statusCode, data: data)
        }
    }

    private static func httpError(statusCode: Int, data: Data) -> APIError {
        if
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let error = object["error"] as? String
        {
            return .httpStatus(statusCode, error)
        }

        if
            let body = String(data: data, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines),
            !body.isEmpty
        {
            return .httpStatus(statusCode, body)
        }

        return .httpStatus(statusCode, nil)
    }
}

private struct CreateSessionBody: Encodable {
    let label: String?
    let model: String?
}

private struct LaunchAgentInstallBody: Encodable {
    let autoStart: Bool

    enum CodingKeys: String, CodingKey {
        case autoStart = "auto_start"
    }
}

private struct TailscaleCertRequest: Encodable {
    let hostname: String
}

private struct AnthropicSetupTokenBody: Encodable {
    let setupToken: String
    let label: String

    enum CodingKeys: String, CodingKey {
        case setupToken = "setup_token"
        case label
    }
}

private struct APIKeyStoreBody: Encodable {
    let apiKey: String
    let label: String

    enum CodingKeys: String, CodingKey {
        case apiKey = "api_key"
        case label
    }
}

private struct VerifyProviderBody: Encodable {
    let timeoutSeconds: Int

    enum CodingKeys: String, CodingKey {
        case timeoutSeconds = "timeout_seconds"
    }
}

private struct ConfigPatchBody: Encodable {
    let changes: JSONValue
}

private struct PermissionPromptRespondBody: Encodable {
    let decision: PermissionPromptDecision
}

private struct LegacyPermissionPromptRespondBody: Encodable {
    let decision: String
    let scope: String
}

private struct InstallSkillRequest: Encodable {
    let name: String
}

private struct UpdateSkillPermissionsRequest: Encodable {
    let capabilities: [String]
}

private struct FleetDispatchTaskBody: Encodable {
    let task: String
    let priority: String
}

private struct GitPathsRequest: Encodable {
    let paths: [String]
}

private struct EmptyJSONRequest: Encodable {}

private struct GitCommitRequestBody: Encodable {
    let message: String
}

private struct SendMessageBody: Encodable {
    let message: String
    let images: [ImagePayload]
    let sessionID: String?

    enum CodingKeys: String, CodingKey {
        case message
        case images
        case sessionID = "session_id"
    }
}

private struct SetModelBody: Encodable {
    let model: String
}

private struct SetThinkingBody: Encodable {
    let level: String
}

private struct PairingExchangeBody: Encodable {
    let code: String
    let deviceName: String

    enum CodingKeys: String, CodingKey {
        case code
        case deviceName = "device_name"
    }
}

private struct LocalAdoptBody: Encodable {
    let deviceName: String

    enum CodingKeys: String, CodingKey {
        case deviceName = "device_name"
    }
}

struct PairingExchangeResponse: Decodable, Sendable {
    let token: String
    let deviceName: String?

    enum CodingKeys: String, CodingKey {
        case token
        case deviceName = "device_name"
    }
}
