import Foundation
import XCTest
@testable import Fawx

final class FawxClientThinkingTests: XCTestCase {
    func testSetThinkingBuildsPostRequest() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let request = try await client.setThinkingRequestForTesting(.high)
        let body = try XCTUnwrap(request.httpBody)
        let payload = try JSONSerialization.jsonObject(with: body) as? [String: String]

        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.url?.path, "/v1/thinking")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
        XCTAssertEqual(payload?["level"], "high")
    }

    func testSetThinkingRetriesWithPutAfterLegacyMethodResponse() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockThinkingURLProtocol.self]
        let session = URLSession(configuration: configuration)
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token",
            restSession: session,
            streamSession: session
        )

        await MockThinkingURLProtocol.stubResponses([
            .init(statusCode: 405),
            .json(
                """
                {
                    "previous_level": "low",
                    "level": "high",
                    "valid_levels": ["off", "low", "high"]
                }
                """
            ),
        ])

        let response = try await client.setThinking(.high)
        let requests = await MockThinkingURLProtocol.recordedRequests()
        await MockThinkingURLProtocol.reset()

        XCTAssertEqual(requests.map(\.httpMethod), ["POST", "PUT"])
        XCTAssertEqual(requests.map(\.url?.path), ["/v1/thinking", "/v1/thinking"])
        XCTAssertEqual(response.previousLevel, .low)
        XCTAssertEqual(response.level, .high)
        XCTAssertEqual(response.validLevels.map(\.rawValue), ["off", "low", "high"])
    }
}

private final class MockThinkingURLProtocol: URLProtocol, @unchecked Sendable {
    private static let store = MockThinkingURLProtocolStore()
    private var requestTask: Task<Void, Never>?

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        let request = self.request
        let client = client

        requestTask = Task {
            do {
                let (response, data) = try await Self.store.nextResponse(for: request)
                guard !Task.isCancelled else {
                    return
                }

                client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
                client?.urlProtocol(self, didLoad: data)
                client?.urlProtocolDidFinishLoading(self)
            } catch {
                guard !Task.isCancelled else {
                    return
                }

                client?.urlProtocol(self, didFailWithError: error)
            }
        }
    }

    override func stopLoading() {
        requestTask?.cancel()
        requestTask = nil
    }

    static func stubResponses(_ responses: [MockThinkingResponse]) async {
        await store.stubResponses(responses)
    }

    static func recordedRequests() async -> [URLRequest] {
        await store.recordedRequests()
    }

    static func reset() async {
        await store.reset()
    }
}

private actor MockThinkingURLProtocolStore {
    private var responses: [MockThinkingResponse] = []
    private var requests: [URLRequest] = []

    func stubResponses(_ responses: [MockThinkingResponse]) {
        self.responses = responses
        requests = []
    }

    func nextResponse(for request: URLRequest) throws -> (HTTPURLResponse, Data) {
        requests.append(request)

        guard !responses.isEmpty else {
            throw MockThinkingProtocolError.missingStubResponse
        }

        let response = responses.removeFirst()
        guard let url = request.url else {
            throw MockThinkingProtocolError.missingURL
        }

        guard let httpResponse = HTTPURLResponse(
            url: url,
            statusCode: response.statusCode,
            httpVersion: nil,
            headerFields: ["Content-Type": "application/json"]
        ) else {
            throw MockThinkingProtocolError.invalidResponse
        }

        return (httpResponse, response.body)
    }

    func recordedRequests() -> [URLRequest] {
        requests
    }

    func reset() {
        responses = []
        requests = []
    }
}

private struct MockThinkingResponse: Sendable {
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

private enum MockThinkingProtocolError: Error {
    case invalidResponse
    case missingStubResponse
    case missingURL
}
