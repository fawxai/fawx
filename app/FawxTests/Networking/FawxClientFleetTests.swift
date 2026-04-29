import Foundation
import XCTest
@testable import Fawx

final class FawxClientFleetTests: XCTestCase {
    func testRemoveFleetNodeBuildsDeleteRequestWithEncodedNodeID() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let request = try await client.removeFleetNodeRequestForTesting(id: "node/a b")
        let components = try XCTUnwrap(request.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(request.httpMethod, "DELETE")
        XCTAssertEqual(components.percentEncodedPath, "/v1/fleet/nodes/node%2Fa%20b")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
    }

    func testWorkspaceCatalogRequestsBuildExpectedPaths() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )
        let workspaceScope = WorkspaceScope(explicitPath: "/Users/fawx/fawx-opened")

        let workspacesRequest = try await client.listWorkspacesRequestForTesting()
        let openWorkspaceRequest = try await client.openWorkspaceRequestForTesting(path: "/tmp/repo-a")
        let threadsRequest = try await client.workspaceThreadsRequestForTesting(
            id: "repo/main",
            workspaceScope: workspaceScope
        )
        let worktreesRequest = try await client.workspaceWorktreesRequestForTesting(
            id: "repo/main",
            workspaceScope: workspaceScope
        )
        let createThreadRequest = try await client.createThreadRequestForTesting(
            workspaceID: "ws-repo",
            workspaceScope: workspaceScope,
            title: "Thread title",
            model: "gpt-5.4",
            worktreeID: "wt-1"
        )
        let createWorktreeRequest = try await client.createWorktreeRequestForTesting(
            workspaceID: "ws-repo",
            workspaceScope: workspaceScope,
            branch: "feature/thread-state",
            baseRef: "origin/dev"
        )
        let archiveWorktreeRequest = try await client.archiveWorktreeRequestForTesting(
            id: "wt/1",
            workspaceScope: workspaceScope
        )
        let deleteWorktreeRequest = try await client.deleteWorktreeRequestForTesting(
            id: "wt/1",
            workspaceScope: workspaceScope
        )
        let threadsComponents = try XCTUnwrap(threadsRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let worktreesComponents = try XCTUnwrap(worktreesRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let createThreadComponents = try XCTUnwrap(createThreadRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let createWorktreeComponents = try XCTUnwrap(createWorktreeRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let archiveWorktreeComponents = try XCTUnwrap(archiveWorktreeRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let deleteWorktreeComponents = try XCTUnwrap(deleteWorktreeRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(workspacesRequest.httpMethod, "GET")
        XCTAssertEqual(workspacesRequest.url?.path, "/v1/workspaces")
        XCTAssertEqual(openWorkspaceRequest.httpMethod, "POST")
        XCTAssertEqual(openWorkspaceRequest.url?.path, "/v1/workspaces/open")
        XCTAssertEqual(workspacesRequest.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
        XCTAssertEqual(threadsComponents.percentEncodedPath, "/v1/workspaces/repo%2Fmain/threads")
        XCTAssertEqual(worktreesComponents.percentEncodedPath, "/v1/workspaces/repo%2Fmain/worktrees")
        XCTAssertEqual(
            threadsComponents.queryItems?.first(where: { $0.name == "workspace_path" })?.value,
            "/Users/fawx/fawx-opened"
        )
        XCTAssertEqual(
            worktreesComponents.queryItems?.first(where: { $0.name == "workspace_path" })?.value,
            "/Users/fawx/fawx-opened"
        )
        XCTAssertEqual(createThreadRequest.httpMethod, "POST")
        XCTAssertEqual(createThreadComponents.percentEncodedPath, "/v1/threads")
        let createThreadJSON = try XCTUnwrap(
            JSONSerialization.jsonObject(with: try XCTUnwrap(createThreadRequest.httpBody))
                as? [String: String]
        )
        XCTAssertEqual(
            createThreadJSON,
            [
                "workspace_id": "ws-repo",
                "workspace_path": "/Users/fawx/fawx-opened",
                "title": "Thread title",
                "model": "gpt-5.4",
                "worktree_id": "wt-1",
            ]
        )
        XCTAssertEqual(createWorktreeRequest.httpMethod, "POST")
        XCTAssertEqual(createWorktreeComponents.percentEncodedPath, "/v1/worktrees")
        let createWorktreeJSON = try XCTUnwrap(
            JSONSerialization.jsonObject(with: try XCTUnwrap(createWorktreeRequest.httpBody))
                as? [String: String]
        )
        XCTAssertEqual(
            createWorktreeJSON,
            [
                "workspace_id": "ws-repo",
                "workspace_path": "/Users/fawx/fawx-opened",
                "branch": "feature/thread-state",
                "base_ref": "origin/dev",
            ]
        )
        XCTAssertEqual(archiveWorktreeRequest.httpMethod, "POST")
        XCTAssertEqual(archiveWorktreeComponents.percentEncodedPath, "/v1/worktrees/wt%2F1/archive")
        XCTAssertEqual(
            archiveWorktreeComponents.queryItems?.first(where: { $0.name == "workspace_path" })?.value,
            "/Users/fawx/fawx-opened"
        )
        XCTAssertEqual(deleteWorktreeRequest.httpMethod, "DELETE")
        XCTAssertEqual(deleteWorktreeComponents.percentEncodedPath, "/v1/worktrees/wt%2F1")
        XCTAssertEqual(
            deleteWorktreeComponents.queryItems?.first(where: { $0.name == "workspace_path" })?.value,
            "/Users/fawx/fawx-opened"
        )
    }

    func testGitTargetRequestsCarryExplicitWorkspaceAndWorktreeScope() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )
        let target = GitRepositoryTarget(
            kind: .worktree,
            id: "worktree:wt-1",
            title: "feature/git-picker",
            subtitle: "/Users/fawx/fawx/.worktrees/git-picker",
            sessionID: nil,
            workspaceID: "ws-repo",
            workspacePath: "/Users/fawx/fawx",
            worktreeID: "wt-1",
            branchName: "feature/git-picker"
        )

        let request = try await client.gitStatusRequestForTesting(target: target)
        let components = try XCTUnwrap(request.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(request.httpMethod, "GET")
        XCTAssertEqual(components.percentEncodedPath, "/v1/git/status")
        XCTAssertEqual(
            components.queryItems?.first(where: { $0.name == "workspace_id" })?.value,
            "ws-repo"
        )
        XCTAssertEqual(
            components.queryItems?.first(where: { $0.name == "workspace_path" })?.value,
            "/Users/fawx/fawx"
        )
        XCTAssertEqual(
            components.queryItems?.first(where: { $0.name == "worktree_id" })?.value,
            "wt-1"
        )
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
    }

    func testWorkspaceCatalogEndpointsDecodeResponsesAndHitExpectedPaths() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockWorkspaceURLProtocol.self]
        let session = URLSession(configuration: configuration)
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token",
            restSession: session,
            streamSession: session
        )
        let workspaceScope = WorkspaceScope(explicitPath: "/Users/fawx/fawx-opened")

        MockWorkspaceURLProtocol.setResponder { request in
            switch request.url?.path {
            case "/v1/workspaces":
                return .json(
                    """
                    {
                      "workspaces": [
                        {
                          "id": "ws-repo",
                          "name": "Repository",
                          "path": "/Users/fawx/fawx",
                          "kind": "repository",
                          "repo": {
                            "root": "/Users/fawx/fawx",
                            "vcs": "git",
                            "current_branch": "dev",
                            "default_branch": "main",
                            "origin": null,
                            "clean": true
                          },
                          "last_opened_at": 1710000000
                        }
                      ],
                      "total": 1
                    }
                    """
                )
            case "/v1/workspaces/ws-repo/threads":
                return .json(
                    """
                    {
                      "threads": [
                        {
                          "id": "thread-1",
                          "title": "Fix workspace navigation",
                          "kind": "coding",
                          "workspace_id": "ws-repo",
                          "worktree_id": "wt-1",
                          "active_session_id": "session-1",
                          "status": "active",
                          "preview": "Refactoring selection state",
                          "model": "gpt-5.4",
                          "created_at": 1710000000,
                          "updated_at": 1710000100
                        }
                      ],
                      "total": 1
                    }
                    """
                )
            case "/v1/workspaces/ws-repo/worktrees":
                return .json(
                    """
                    {
                      "worktrees": [
                        {
                          "id": "wt-1",
                          "workspace_id": "ws-repo",
                          "label": "feature/thread-state",
                          "path": "/Users/fawx/fawx/.worktrees/thread-state",
                          "branch": "feature/thread-state",
                          "base_ref": "origin/dev",
                          "status": "active",
                          "clean": true,
                          "ahead_count": 0,
                          "behind_count": 0
                        }
                      ],
                      "total": 1
                    }
                    """
                )
            case "/v1/workspaces/open":
                return .json(
                    """
                    {
                      "id": "ws-opened",
                      "name": "Opened Workspace",
                      "path": "/Users/fawx/fawx-opened",
                      "kind": "repository",
                      "repo": null,
                      "last_opened_at": 1710000200
                    }
                    """
                )
            case "/v1/threads":
                return .json(
                    """
                    {
                      "id": "thread-created",
                      "title": "Created thread",
                      "kind": "coding",
                      "workspace_id": "ws-repo",
                      "worktree_id": "wt-1",
                      "active_session_id": "session-created",
                      "status": "idle",
                      "preview": null,
                      "model": "gpt-5.4",
                      "created_at": 1710000200,
                      "updated_at": 1710000200
                    }
                    """
                )
            case "/v1/worktrees":
                return .json(
                    """
                    {
                      "id": "wt-created",
                      "workspace_id": "ws-repo",
                      "label": "feature/new-lane",
                      "path": "/Users/fawx/fawx/.worktrees/new-lane",
                      "branch": "feature/new-lane",
                      "base_ref": "origin/dev",
                      "status": "available",
                      "clean": true,
                      "ahead_count": 0,
                      "behind_count": 0
                    }
                    """
                )
            case "/v1/worktrees/wt-1/archive":
                return .json(
                    """
                    {
                      "worktree_id": "wt-1",
                      "archived_thread_count": 1
                    }
                    """
                )
            case "/v1/worktrees/wt-1":
                return .json(
                    """
                    {
                      "deleted": true,
                      "worktree_id": "wt-1"
                    }
                    """
                )
            default:
                return .json("{}", statusCode: 404)
            }
        }

        let workspaces = try await client.listWorkspaces()
        let openedWorkspace = try await client.openWorkspace(path: "/Users/fawx/fawx-opened")
        let threads = try await client.workspaceThreads(
            id: "ws-repo",
            workspaceScope: workspaceScope
        )
        let worktrees = try await client.workspaceWorktrees(
            id: "ws-repo",
            workspaceScope: workspaceScope
        )
        let createdThread = try await client.createThread(
            workspaceID: "ws-repo",
            workspaceScope: workspaceScope,
            title: "Created thread",
            model: "gpt-5.4",
            worktreeID: "wt-1"
        )
        let createdWorktree = try await client.createWorktree(
            workspaceID: "ws-repo",
            workspaceScope: workspaceScope,
            branch: "feature/new-lane",
            baseRef: "origin/dev"
        )
        let archivedWorktree = try await client.archiveWorktree(
            id: "wt-1",
            workspaceScope: workspaceScope
        )
        let deletedWorktree = try await client.deleteWorktree(
            id: "wt-1",
            workspaceScope: workspaceScope
        )
        let requests = MockWorkspaceURLProtocol.recordedRequests()
        MockWorkspaceURLProtocol.reset()

        XCTAssertEqual(workspaces.workspaces.map(\.id), ["ws-repo"])
        XCTAssertEqual(openedWorkspace.id, "ws-opened")
        XCTAssertEqual(threads.threads.map(\.activeSessionID), ["session-1"])
        XCTAssertEqual(worktrees.worktrees.map(\.id), ["wt-1"])
        XCTAssertEqual(createdThread.activeSessionID, "session-created")
        XCTAssertEqual(createdWorktree.id, "wt-created")
        XCTAssertEqual(archivedWorktree.archivedThreadCount, 1)
        XCTAssertTrue(deletedWorktree.deleted)
        XCTAssertEqual(
            requests.map(\.url?.path),
            [
                "/v1/workspaces",
                "/v1/workspaces/open",
                "/v1/workspaces/ws-repo/threads",
                "/v1/workspaces/ws-repo/worktrees",
                "/v1/threads",
                "/v1/worktrees",
                "/v1/worktrees/wt-1/archive",
                "/v1/worktrees/wt-1",
            ]
        )
    }

    func testGitLogRequestUsesSharedSessionQueryItems() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let request = try await client.gitLogRequestForTesting(limit: 25, sessionID: "session-123")
        let components = try XCTUnwrap(request.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(request.httpMethod, "GET")
        XCTAssertEqual(components.percentEncodedPath, "/v1/git/log")
        XCTAssertEqual(
            components.queryItems?.first(where: { $0.name == "limit" })?.value,
            "25"
        )
        XCTAssertEqual(
            components.queryItems?.first(where: { $0.name == "session_id" })?.value,
            "session-123"
        )
    }

    func testSessionArchiveRequestsBuildExpectedPathsAndArchivedQuery() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let listRequest = try await client.listSessionsRequestForTesting(archived: .archivedOnly)
        let archiveRequest = try await client.archiveSessionRequestForTesting(id: "session/a b")
        let unarchiveRequest = try await client.archiveSessionRequestForTesting(
            id: "session/a b",
            method: "DELETE"
        )
        let listComponents = try XCTUnwrap(listRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let archiveComponents = try XCTUnwrap(archiveRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })
        let unarchiveComponents = try XCTUnwrap(unarchiveRequest.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(listRequest.httpMethod, "GET")
        XCTAssertEqual(
            listComponents.queryItems?.first(where: { $0.name == "archived" })?.value,
            "only"
        )
        XCTAssertEqual(archiveRequest.httpMethod, "POST")
        XCTAssertEqual(unarchiveRequest.httpMethod, "DELETE")
        XCTAssertEqual(
            archiveComponents.percentEncodedPath,
            "/v1/sessions/session%2Fa%20b/archive"
        )
        XCTAssertEqual(
            unarchiveComponents.percentEncodedPath,
            "/v1/sessions/session%2Fa%20b/archive"
        )
    }
}

private final class MockWorkspaceURLProtocol: URLProtocol, @unchecked Sendable {
    private static let store = MockWorkspaceURLProtocolStore()

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

    static func setResponder(_ responder: @escaping MockWorkspaceURLProtocolStore.Responder) {
        store.setResponder(responder)
    }

    static func recordedRequests() -> [URLRequest] {
        store.recordedRequests()
    }

    static func reset() {
        store.reset()
    }
}

private final class MockWorkspaceURLProtocolStore: @unchecked Sendable {
    typealias Responder = @Sendable (URLRequest) throws -> MockWorkspaceResponse

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
            throw MockWorkspaceProtocolError.missingResponder
        }

        let response = try responder(request)
        guard let url = request.url else {
            throw MockWorkspaceProtocolError.missingURL
        }
        guard let httpResponse = HTTPURLResponse(
            url: url,
            statusCode: response.statusCode,
            httpVersion: nil,
            headerFields: ["Content-Type": "application/json"]
        ) else {
            throw MockWorkspaceProtocolError.invalidResponse
        }

        return (httpResponse, response.body)
    }

    func recordedRequests() -> [URLRequest] {
        lock.lock()
        defer { lock.unlock() }
        return requests
    }

    func reset() {
        lock.lock()
        defer { lock.unlock() }
        responder = nil
        requests = []
    }
}

private struct MockWorkspaceResponse: Sendable {
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

private enum MockWorkspaceProtocolError: Error {
    case invalidResponse
    case missingResponder
    case missingURL
}
