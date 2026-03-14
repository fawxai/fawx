import Foundation

actor FawxClient {
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
        streamConfiguration.timeoutIntervalForRequest = 0
        streamConfiguration.timeoutIntervalForResource = 0
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

    func authProviders() async throws -> AuthProvidersResponse {
        try await performRequest(
            candidatePaths: ["/v1/auth/status", "/v1/auth"],
            decodeAs: AuthProvidersResponse.self
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
            } catch APIError.httpStatus(let code, _) where code == 404 {
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

struct PairingExchangeResponse: Decodable, Sendable {
    let token: String
    let deviceName: String?

    enum CodingKeys: String, CodingKey {
        case token
        case deviceName = "device_name"
    }
}
