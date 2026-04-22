import Foundation

actor FawxClient {
    private static let streamIdleTimeout: TimeInterval = 120
    private static let streamResourceTimeout: TimeInterval = 600
    private static let workspacesPath = "/v1/workspaces"

    private var baseURL: URL?
    private var bearerToken: String?

    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()
    private let restSession: URLSession
    private let streamSession: URLSession

    init(baseURL: URL? = nil, bearerToken: String? = nil) {
        self.init(
            baseURL: baseURL,
            bearerToken: bearerToken,
            restSession: Self.makeRestSession(),
            streamSession: Self.makeStreamSession()
        )
    }

    init(
        baseURL: URL? = nil,
        bearerToken: String? = nil,
        restSession: URLSession,
        streamSession: URLSession
    ) {
        self.baseURL = baseURL
        self.bearerToken = bearerToken
        self.restSession = restSession
        self.streamSession = streamSession
    }

    func updateConfiguration(baseURL: URL?, bearerToken: String?) {
        debugLogConfigurationUpdate(from: self.baseURL, to: baseURL)
        self.baseURL = baseURL
        self.bearerToken = bearerToken
    }

    private func gitTargetQueryItems(
        sessionID: String? = nil,
        target: GitRepositoryTarget? = nil
    ) -> [URLQueryItem] {
        var queryItems: [URLQueryItem] = []
        let resolvedSessionID = sessionID?.nonEmpty ?? target?.sessionID?.nonEmpty
        if let resolvedSessionID {
            queryItems.append(URLQueryItem(name: "session_id", value: resolvedSessionID))
        }
        if let workspaceID = target?.workspaceID?.nonEmpty {
            queryItems.append(URLQueryItem(name: "workspace_id", value: workspaceID))
        }
        if let workspacePath = target?.workspacePath?.nonEmpty {
            queryItems.append(URLQueryItem(name: "workspace_path", value: workspacePath))
        }
        if let worktreeID = target?.worktreeID?.nonEmpty {
            queryItems.append(URLQueryItem(name: "worktree_id", value: worktreeID))
        }
        return queryItems
    }

    private func gitLogQueryItems(limit: Int, sessionID: String?) -> [URLQueryItem] {
        var queryItems = [URLQueryItem(name: "limit", value: String(limit))]
        queryItems.append(contentsOf: gitTargetQueryItems(sessionID: sessionID))
        return queryItems
    }

    private func gitLogQueryItems(limit: Int, target: GitRepositoryTarget?) -> [URLQueryItem] {
        var queryItems = [URLQueryItem(name: "limit", value: String(limit))]
        queryItems.append(contentsOf: gitTargetQueryItems(target: target))
        return queryItems
    }

    private func workspaceScopeQueryItems(workspaceScope: WorkspaceScope?) -> [URLQueryItem] {
        guard let requestedPath = workspaceScope?.requestedPath else {
            return []
        }
        return [URLQueryItem(name: "workspace_path", value: requestedPath)]
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

    func generatePairingCode() async throws -> PairingCodeResponse {
        try await performRequest(
            path: "/v1/pair/generate",
            method: "POST",
            bodyData: Data("{}".utf8),
            decodeAs: PairingCodeResponse.self
        )
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
        } catch let error as APIError where shouldRetryLegacyPermissionsPatch(for: request, after: error) {
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

    func ripcordStatus() async throws -> RipcordStatusResponse {
        try await performRequest(path: "/v1/ripcord/status", decodeAs: RipcordStatusResponse.self)
    }

    func ripcordJournal() async throws -> RipcordJournalResponse {
        try await performRequest(path: "/v1/ripcord/journal", decodeAs: RipcordJournalResponse.self)
    }

    func pullRipcord() async throws -> RipcordReport {
        try await performRequest(
            path: "/v1/ripcord/pull",
            method: "POST",
            bodyData: Data(),
            decodeAs: RipcordReport.self
        )
    }

    func approveRipcord() async throws {
        let _: RipcordApproveResponse = try await performRequest(
            path: "/v1/ripcord/approve",
            method: "POST",
            bodyData: Data(),
            decodeAs: RipcordApproveResponse.self
        )
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

    func listWorkspaces() async throws -> WorkspacesResponse {
        try await performRequest(path: Self.workspacesPath, decodeAs: WorkspacesResponse.self)
    }

    func workspaceThreads(id: String, workspaceScope: WorkspaceScope? = nil) async throws -> ThreadsResponse {
        try await performRequest(
            path: Self.workspaceThreadsPath(id: id),
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            decodeAs: ThreadsResponse.self
        )
    }

    func workspaceWorktrees(id: String, workspaceScope: WorkspaceScope? = nil) async throws -> WorktreesResponse {
        try await performRequest(
            path: Self.workspaceWorktreesPath(id: id),
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            decodeAs: WorktreesResponse.self
        )
    }

    func openWorkspace(path: String) async throws -> WorkspaceSummary {
        let body = try encoder.encode(OpenWorkspaceBody(path: path))
        return try await performRequest(
            path: "\(Self.workspacesPath)/open",
            method: "POST",
            bodyData: body,
            decodeAs: WorkspaceSummary.self
        )
    }

    func createThread(
        workspaceID: String,
        workspaceScope: WorkspaceScope? = nil,
        title: String? = nil,
        model: String? = nil,
        thinking: ThinkingLevel? = nil,
        worktreeID: String? = nil
    ) async throws -> ThreadSummary {
        let body = try encoder.encode(
            CreateThreadBody(
                workspaceID: workspaceID,
                workspaceScope: workspaceScope,
                title: title,
                model: model,
                thinking: thinking?.rawValue,
                worktreeID: worktreeID
            )
        )
        return try await performRequest(
            path: "/v1/threads",
            method: "POST",
            bodyData: body,
            decodeAs: ThreadSummary.self
        )
    }

    func createWorktree(
        workspaceID: String,
        workspaceScope: WorkspaceScope? = nil,
        branch: String,
        baseRef: String? = nil
    ) async throws -> WorktreeSummary {
        let body = try encoder.encode(
            CreateWorktreeBody(
                workspaceID: workspaceID,
                workspaceScope: workspaceScope,
                branch: branch,
                baseRef: baseRef
            )
        )
        return try await performRequest(
            path: "/v1/worktrees",
            method: "POST",
            bodyData: body,
            decodeAs: WorktreeSummary.self
        )
    }

    func attachWorktreeThread(
        worktreeID: String,
        threadID: String,
        workspaceScope: WorkspaceScope? = nil
    ) async throws -> AttachWorktreeThreadResponse {
        let body = try encoder.encode(AttachWorktreeThreadBody(threadID: threadID))
        return try await performRequest(
            path: Self.worktreePath(id: worktreeID, suffix: "attach-thread"),
            method: "POST",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            bodyData: body,
            decodeAs: AttachWorktreeThreadResponse.self
        )
    }

    func archiveWorktree(id: String, workspaceScope: WorkspaceScope? = nil) async throws -> ArchiveWorktreeResponse {
        try await performRequest(
            path: Self.worktreePath(id: id, suffix: "archive"),
            method: "POST",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            bodyData: Data(),
            decodeAs: ArchiveWorktreeResponse.self
        )
    }

    func deleteWorktree(id: String, workspaceScope: WorkspaceScope? = nil) async throws -> DeleteWorktreeResponse {
        try await performRequest(
            path: Self.worktreePath(id: id),
            method: "DELETE",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            decodeAs: DeleteWorktreeResponse.self
        )
    }

    func listSessions(
        kind: SessionKind? = nil,
        limit: Int? = nil,
        archived: SessionArchiveFilter = .active
    ) async throws -> SessionsResponse {
        var queryItems: [URLQueryItem] = []
        if let kind {
            queryItems.append(.init(name: "kind", value: kind.rawValue))
        }
        if let limit {
            queryItems.append(.init(name: "limit", value: String(limit)))
        }
        queryItems.append(.init(name: "archived", value: archived.rawValue))

        return try await performRequest(
            path: "/v1/sessions",
            queryItems: queryItems,
            decodeAs: SessionsResponse.self
        )
    }

    func createSession(
        label: String? = nil,
        model: String? = nil,
        thinking: ThinkingLevel? = nil
    ) async throws -> Session {
        let body = try encoder.encode(
            CreateSessionBody(label: label, model: model, thinking: thinking?.rawValue)
        )
        return try await performRequest(
            path: "/v1/sessions",
            method: "POST",
            bodyData: body,
            decodeAs: Session.self
        )
    }

    func session(id: String) async throws -> Session {
        try await performRequest(path: Self.sessionPath(id: id), decodeAs: Session.self)
    }

    func updateSessionModel(id: String, model: String) async throws -> Session {
        let body = try encoder.encode(SetModelBody(model: model))
        return try await performRequest(
            path: Self.sessionPath(id: id, suffix: "model"),
            method: "PUT",
            bodyData: body,
            decodeAs: Session.self
        )
    }

    func updateSessionThinking(id: String, level: ThinkingLevel) async throws -> Session {
        let body = try encoder.encode(SetThinkingBody(level: level.rawValue))
        return try await performRequest(
            path: Self.sessionPath(id: id, suffix: "thinking"),
            method: "PUT",
            bodyData: body,
            decodeAs: Session.self
        )
    }

    func archiveSession(id: String) async throws -> Session {
        try await performRequest(
            path: Self.sessionPath(id: id, suffix: "archive"),
            method: "POST",
            bodyData: Data(),
            decodeAs: Session.self
        )
    }

    func unarchiveSession(id: String) async throws -> Session {
        try await performRequest(
            path: Self.sessionPath(id: id, suffix: "archive"),
            method: "DELETE",
            decodeAs: Session.self
        )
    }

    func deleteSession(id: String) async throws -> DeleteSessionResponse {
        try await performRequest(
            path: Self.sessionPath(id: id),
            method: "DELETE",
            decodeAs: DeleteSessionResponse.self
        )
    }

    func clearSession(id: String) async throws -> ClearSessionResponse {
        try await performRequest(
            path: Self.sessionPath(id: id, suffix: "clear"),
            method: "POST",
            bodyData: Data(),
            decodeAs: ClearSessionResponse.self
        )
    }

    func stopSession(id: String) async throws -> StopSessionResponse {
        try await performRequest(
            path: Self.sessionPath(id: id, suffix: "stop"),
            method: "POST",
            bodyData: Data(),
            decodeAs: StopSessionResponse.self
        )
    }

    func steerSession(id: String, text: String) async throws -> SteerSessionResponse {
        let body = try encoder.encode(SteerSessionBody(text: text))
        return try await performRequest(
            path: Self.sessionPath(id: id, suffix: "steer"),
            method: "POST",
            bodyData: body,
            decodeAs: SteerSessionResponse.self
        )
    }

    func sessionMessages(id: String, limit: Int = 200) async throws -> MessagesResponse {
        try await performRequest(
            path: Self.sessionPath(id: id, suffix: "messages"),
            queryItems: [.init(name: "limit", value: String(limit))],
            decodeAs: MessagesResponse.self
        )
    }

    func sendMessage(
        sessionID: String,
        message: String,
        images: [ImagePayload] = [],
        documents: [DocumentPayload] = [],
        steering: String? = nil
    ) async throws -> MessageResponse {
        let body = try encoder.encode(
            SendMessageBody(
                message: message,
                images: images,
                documents: documents,
                steering: steering,
                sessionID: nil
            )
        )
        return try await performRequest(
            path: Self.sessionPath(id: sessionID, suffix: "messages"),
            method: "POST",
            bodyData: body,
            decodeAs: MessageResponse.self
        )
    }

    func sendMessageStream(
        sessionID: String,
        message: String,
        images: [ImagePayload] = [],
        documents: [DocumentPayload] = [],
        steering: String? = nil
    ) throws -> AsyncThrowingStream<SSEEvent, Error> {
        let body = try encoder.encode(
            SendMessageBody(
                message: message,
                images: images,
                documents: documents,
                steering: steering,
                sessionID: nil
            )
        )
        let request = try makeRequest(
            path: Self.sessionPath(id: sessionID, suffix: "messages"),
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
        try await performRequest(path: Self.sessionPath(id: id, suffix: "context"), decodeAs: ContextInfo.self)
    }

    func sessionMemory(id: String) async throws -> SessionMemory {
        try await performRequest(path: Self.sessionPath(id: id, suffix: "memory"), decodeAs: SessionMemory.self)
    }

    func updateSessionMemory(id: String, memory: SessionMemory) async throws -> SessionMemory {
        let body = try encoder.encode(memory)
        return try await performRequest(
            path: Self.sessionPath(id: id, suffix: "memory"),
            method: "PUT",
            bodyData: body,
            decodeAs: SessionMemory.self
        )
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
        do {
            return try await performRequest(
                path: "/v1/thinking",
                method: "POST",
                bodyData: body,
                decodeAs: SetThinkingResponse.self
            )
        } catch APIError.httpStatus(let code, _) where code == 404 || code == 405 {
            return try await performRequest(
                path: "/v1/thinking",
                method: "PUT",
                bodyData: body,
                decodeAs: SetThinkingResponse.self
            )
        }
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

    func skillSettings(name: String) async throws -> SkillSettingsResponse {
        try await performRequest(
            path: "/v1/skills/\(Self.encodedPathComponent(name))/settings",
            decodeAs: SkillSettingsResponse.self
        )
    }

    func updateSkillSettings(
        name: String,
        values: [SkillSettingInput]
    ) async throws -> UpdateSkillSettingsResponse {
        let body = try encoder.encode(UpdateSkillSettingsRequest(values: values))
        return try await performRequest(
            path: "/v1/skills/\(Self.encodedPathComponent(name))/settings",
            method: "PUT",
            bodyData: body,
            decodeAs: UpdateSkillSettingsResponse.self
        )
    }

    func fleetOverview() async throws -> FleetOverviewResponse {
        try await performRequest(path: "/v1/fleet/overview", decodeAs: FleetOverviewResponse.self)
    }

    func fleetNodes() async throws -> FleetNodesResponse {
        try await performRequest(path: "/v1/fleet/nodes", decodeAs: FleetNodesResponse.self)
    }

    func fleetNode(id: String) async throws -> FleetNodeDetailResponse {
        try await performRequest(
            path: "/v1/fleet/nodes/\(Self.encodedPathComponent(id))",
            decodeAs: FleetNodeDetailResponse.self
        )
    }

    func removeFleetNode(id: String) async throws -> FleetRemoveNodeResponse {
        try await performRequest(
            path: "/v1/fleet/nodes/\(Self.encodedPathComponent(id))",
            method: "DELETE",
            decodeAs: FleetRemoveNodeResponse.self
        )
    }

    func dispatchFleetTask(
        nodeID: String,
        task: String,
        priority: String = "normal"
    ) async throws -> FleetDispatchTaskResponse {
        let body = try encoder.encode(FleetDispatchTaskBody(task: task, priority: priority))
        return try await performRequest(
            path: "/v1/fleet/nodes/\(Self.encodedPathComponent(nodeID))/tasks",
            method: "POST",
            bodyData: body,
            decodeAs: FleetDispatchTaskResponse.self
        )
    }

#if DEBUG
    func removeFleetNodeRequestForTesting(id: String) throws -> URLRequest {
        try makeRequest(
            path: "/v1/fleet/nodes/\(Self.encodedPathComponent(id))",
            method: "DELETE",
            authRequired: true
        )
    }

    func listWorkspacesRequestForTesting() throws -> URLRequest {
        try makeRequest(
            path: Self.workspacesPath,
            method: "GET",
            authRequired: true
        )
    }

    func openWorkspaceRequestForTesting(path: String) throws -> URLRequest {
        let body = try encoder.encode(OpenWorkspaceBody(path: path))
        return try makeRequest(
            path: "\(Self.workspacesPath)/open",
            method: "POST",
            authRequired: true,
            bodyData: body
        )
    }

    func listSessionsRequestForTesting(
        kind: SessionKind? = nil,
        limit: Int? = nil,
        archived: SessionArchiveFilter = .active
    ) throws -> URLRequest {
        var queryItems: [URLQueryItem] = [.init(name: "archived", value: archived.rawValue)]
        if let kind {
            queryItems.append(.init(name: "kind", value: kind.rawValue))
        }
        if let limit {
            queryItems.append(.init(name: "limit", value: String(limit)))
        }

        return try makeRequest(
            path: "/v1/sessions",
            method: "GET",
            queryItems: queryItems,
            authRequired: true
        )
    }

    func workspaceThreadsRequestForTesting(id: String, workspaceScope: WorkspaceScope? = nil) throws -> URLRequest {
        try makeRequest(
            path: Self.workspaceThreadsPath(id: id),
            method: "GET",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            authRequired: true
        )
    }

    func workspaceWorktreesRequestForTesting(id: String, workspaceScope: WorkspaceScope? = nil) throws -> URLRequest {
        try makeRequest(
            path: Self.workspaceWorktreesPath(id: id),
            method: "GET",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            authRequired: true
        )
    }

    func createThreadRequestForTesting(
        workspaceID: String,
        workspaceScope: WorkspaceScope? = nil,
        title: String? = nil,
        model: String? = nil,
        thinking: ThinkingLevel? = nil,
        worktreeID: String? = nil
    ) throws -> URLRequest {
        let body = try encoder.encode(
            CreateThreadBody(
                workspaceID: workspaceID,
                workspaceScope: workspaceScope,
                title: title,
                model: model,
                thinking: thinking?.rawValue,
                worktreeID: worktreeID
            )
        )
        return try makeRequest(
            path: "/v1/threads",
            method: "POST",
            authRequired: true,
            bodyData: body
        )
    }

    func createWorktreeRequestForTesting(
        workspaceID: String,
        workspaceScope: WorkspaceScope? = nil,
        branch: String,
        baseRef: String? = nil
    ) throws -> URLRequest {
        let body = try encoder.encode(
            CreateWorktreeBody(
                workspaceID: workspaceID,
                workspaceScope: workspaceScope,
                branch: branch,
                baseRef: baseRef
            )
        )
        return try makeRequest(
            path: "/v1/worktrees",
            method: "POST",
            authRequired: true,
            bodyData: body
        )
    }

    func archiveWorktreeRequestForTesting(
        id: String,
        workspaceScope: WorkspaceScope? = nil,
        method: String = "POST"
    ) throws -> URLRequest {
        try makeRequest(
            path: Self.worktreePath(id: id, suffix: "archive"),
            method: method,
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            authRequired: true,
            bodyData: method == "POST" ? Data() : nil
        )
    }

    func deleteWorktreeRequestForTesting(id: String, workspaceScope: WorkspaceScope? = nil) throws -> URLRequest {
        try makeRequest(
            path: Self.worktreePath(id: id),
            method: "DELETE",
            queryItems: workspaceScopeQueryItems(workspaceScope: workspaceScope),
            authRequired: true
        )
    }

    func gitLogRequestForTesting(limit: Int = 10, sessionID: String? = nil) throws -> URLRequest {
        return try makeRequest(
            path: "/v1/git/log",
            method: "GET",
            queryItems: gitLogQueryItems(limit: limit, sessionID: sessionID),
            authRequired: true
        )
    }

    func gitStatusRequestForTesting(target: GitRepositoryTarget) throws -> URLRequest {
        try makeRequest(
            path: "/v1/git/status",
            method: "GET",
            queryItems: gitTargetQueryItems(target: target),
            authRequired: true
        )
    }

    func setThinkingRequestForTesting(
        _ level: ThinkingLevel,
        method: String = "POST"
    ) throws -> URLRequest {
        let body = try encoder.encode(SetThinkingBody(level: level.rawValue))
        return try makeRequest(
            path: "/v1/thinking",
            method: method,
            authRequired: true,
            bodyData: body
        )
    }

    func sessionMemoryRequestForTesting(id: String) throws -> URLRequest {
        try makeRequest(
            path: Self.sessionPath(id: id, suffix: "memory"),
            method: "GET",
            authRequired: true
        )
    }

    func archiveSessionRequestForTesting(id: String, method: String = "POST") throws -> URLRequest {
        try makeRequest(
            path: Self.sessionPath(id: id, suffix: "archive"),
            method: method,
            authRequired: true,
            bodyData: method == "POST" ? Data() : nil
        )
    }

    func updateSessionMemoryRequestForTesting(
        id: String,
        memory: SessionMemory
    ) throws -> URLRequest {
        let body = try encoder.encode(memory)
        return try makeRequest(
            path: Self.sessionPath(id: id, suffix: "memory"),
            method: "PUT",
            authRequired: true,
            bodyData: body
        )
    }

    func updateSkillSettingsRequestForTesting(
        name: String,
        values: [SkillSettingInput]
    ) throws -> URLRequest {
        let body = try encoder.encode(UpdateSkillSettingsRequest(values: values))
        return try makeRequest(
            path: "/v1/skills/\(Self.encodedPathComponent(name))/settings",
            method: "PUT",
            authRequired: true,
            bodyData: body
        )
    }
#endif

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

    func gitStatus(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitStatusResponse {
        try await performRequest(
            path: "/v1/git/status",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            decodeAs: GitStatusResponse.self
        )
    }

    func gitLog(
        limit: Int = 10,
        sessionID: String? = nil,
        target: GitRepositoryTarget? = nil
    ) async throws -> GitLogResponse {
        return try await performRequest(
            path: "/v1/git/log",
            queryItems: target == nil
                ? gitLogQueryItems(limit: limit, sessionID: sessionID)
                : gitLogQueryItems(limit: limit, target: target),
            decodeAs: GitLogResponse.self
        )
    }

    func gitDiff(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitDiffResponse {
        try await performRequest(
            path: "/v1/git/diff",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            decodeAs: GitDiffResponse.self
        )
    }

    func gitStage(
        paths: [String],
        sessionID: String? = nil,
        target: GitRepositoryTarget? = nil
    ) async throws -> GitStageResponse {
        let body = try encoder.encode(GitPathsRequest(paths: paths))
        return try await performRequest(
            path: "/v1/git/stage",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: body,
            decodeAs: GitStageResponse.self
        )
    }

    func gitStageAll(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitStageResponse {
        let body = try encoder.encode(EmptyJSONRequest())
        return try await performRequest(
            path: "/v1/git/stage",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: body,
            decodeAs: GitStageResponse.self
        )
    }

    func gitUnstage(
        paths: [String],
        sessionID: String? = nil,
        target: GitRepositoryTarget? = nil
    ) async throws -> GitUnstageResponse {
        let body = try encoder.encode(GitPathsRequest(paths: paths))
        return try await performRequest(
            path: "/v1/git/unstage",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: body,
            decodeAs: GitUnstageResponse.self
        )
    }

    func gitUnstageAll(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitUnstageResponse {
        let body = try encoder.encode(EmptyJSONRequest())
        return try await performRequest(
            path: "/v1/git/unstage",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: body,
            decodeAs: GitUnstageResponse.self
        )
    }

    func gitCommit(
        message: String,
        sessionID: String? = nil,
        target: GitRepositoryTarget? = nil
    ) async throws -> GitCommitResponse {
        let body = try encoder.encode(GitCommitRequestBody(message: message))
        return try await performRequest(
            path: "/v1/git/commit",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: body,
            decodeAs: GitCommitResponse.self
        )
    }

    func gitPush(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitPushResponse {
        try await performRequest(
            path: "/v1/git/push",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: Data(),
            decodeAs: GitPushResponse.self
        )
    }

    func gitPull(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitPullResponse {
        try await performRequest(
            path: "/v1/git/pull",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
            bodyData: Data(),
            decodeAs: GitPullResponse.self
        )
    }

    func gitFetch(sessionID: String? = nil, target: GitRepositoryTarget? = nil) async throws -> GitFetchResponse {
        try await performRequest(
            path: "/v1/git/fetch",
            method: "POST",
            queryItems: gitTargetQueryItems(sessionID: sessionID, target: target),
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
        documents: [DocumentPayload] = [],
        steering: String? = nil,
        sessionID: String? = nil
    ) async throws -> MessageResponse {
        let body = try encoder.encode(
            SendMessageBody(
                message: message,
                images: images,
                documents: documents,
                steering: steering,
                sessionID: sessionID
            )
        )
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

    private func shouldRetryLegacyPermissionsPatch(
        for request: PermissionsPatchRequest,
        after error: APIError
    ) -> Bool {
        guard request.legacyCompatibleRequest != nil else {
            return false
        }

        guard let statusCode = error.statusCode else {
            return false
        }

        guard (400 ..< 500).contains(statusCode) else {
            return false
        }

        return statusCode != 401 && statusCode != 403
    }

    private static func makeRestSession() -> URLSession {
        let restConfiguration = URLSessionConfiguration.default
        restConfiguration.timeoutIntervalForRequest = 15
        restConfiguration.timeoutIntervalForResource = 30
        return URLSession(configuration: restConfiguration)
    }

    private static func makeStreamSession() -> URLSession {
        let streamConfiguration = URLSessionConfiguration.default
        streamConfiguration.timeoutIntervalForRequest = Self.streamIdleTimeout
        streamConfiguration.timeoutIntervalForResource = Self.streamResourceTimeout
        return URLSession(configuration: streamConfiguration)
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

        debugLogRequest(path: path, resolvedURL: url)

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
        let resolvedPath = basePath + (path.hasPrefix("/") ? path : "/" + path)
        components.percentEncodedPath = Self.percentEncodedPath(resolvedPath)
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

    private static func encodedPathComponent(_ value: String) -> String {
        let allowed = CharacterSet.urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }

    private static func workspaceThreadsPath(id: String) -> String {
        "\(workspacesPath)/\(encodedPathComponent(id))/threads"
    }

    private static func workspaceWorktreesPath(id: String) -> String {
        "\(workspacesPath)/\(encodedPathComponent(id))/worktrees"
    }

    private static func worktreePath(id: String, suffix: String? = nil) -> String {
        let basePath = "/v1/worktrees/\(encodedPathComponent(id))"
        guard let suffix, suffix.isEmpty == false else {
            return basePath
        }

        return "\(basePath)/\(suffix)"
    }

    private static func sessionPath(id: String, suffix: String? = nil) -> String {
        let basePath = "/v1/sessions/\(encodedPathComponent(id))"
        guard let suffix, suffix.isEmpty == false else {
            return basePath
        }

        return "\(basePath)/\(suffix)"
    }

    private static func percentEncodedPath(_ value: String) -> String {
        let allowed = CharacterSet.urlPathAllowed.union(CharacterSet(charactersIn: "%"))
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }

#if DEBUG
    private func debugLogConfigurationUpdate(from oldValue: URL?, to newValue: URL?) {
        let oldString = oldValue?.absoluteString ?? "<nil>"
        let newString = newValue?.absoluteString ?? "<nil>"
        guard oldString != newString || newString.contains(":18400") else {
            return
        }

        NSLog(
            """
            [FawxDebug][FawxClient] updateConfiguration old=%@ new=%@ stack=%@
            """,
            oldString,
            newString,
            Thread.callStackSymbols.prefix(8).joined(separator: " | ")
        )
    }

    private func debugLogRequest(path: String, resolvedURL: URL) {
        let urlString = resolvedURL.absoluteString
        let shouldLog = urlString.contains(":18400")
            || path.contains("/messages")
            || path == "/health"
        guard shouldLog else {
            return
        }

        NSLog(
            "[FawxDebug][FawxClient] request path=%@ url=%@",
            path,
            urlString
        )
    }
#else
    private func debugLogConfigurationUpdate(from oldValue: URL?, to newValue: URL?) {}

    private func debugLogRequest(path: String, resolvedURL: URL) {}
#endif
}

private struct CreateSessionBody: Encodable {
    let label: String?
    let model: String?
    let thinking: String?
}

private struct OpenWorkspaceBody: Encodable {
    let path: String
}

private struct CreateThreadBody: Encodable {
    let workspaceID: String
    let workspaceScope: WorkspaceScope?
    let title: String?
    let model: String?
    let thinking: String?
    let worktreeID: String?

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case workspaceScope = "workspace_path"
        case title
        case model
        case thinking
        case worktreeID = "worktree_id"
    }
}

private struct CreateWorktreeBody: Encodable {
    let workspaceID: String
    let workspaceScope: WorkspaceScope?
    let branch: String
    let baseRef: String?

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case workspaceScope = "workspace_path"
        case branch
        case baseRef = "base_ref"
    }
}

private struct AttachWorktreeThreadBody: Encodable {
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
    }
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

private struct UpdateSkillSettingsRequest: Encodable {
    let values: [SkillSettingInput]
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
    let documents: [DocumentPayload]
    let steering: String?
    let sessionID: String?

    enum CodingKeys: String, CodingKey {
        case message
        case images
        case documents
        case steering
        case sessionID = "session_id"
    }
}

private struct SteerSessionBody: Encodable {
    let text: String
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
