import AuthenticationServices
import Foundation
import Network

#if os(macOS)
import AppKit
#else
import UIKit
#endif

@MainActor
final class OAuthSessionCoordinator: NSObject {
    private var session: ASWebAuthenticationSession?
    private var localhostBridge: LocalhostOAuthCallbackBridge?
    private let presentationProvider = OAuthPresentationContextProvider()

    func authenticate(
        authorizeURL: URL,
        providerRedirectURL: URL,
        callbackURL: URL
    ) async throws -> URL {
        guard let callbackScheme = callbackURL.scheme else {
            throw APIError.invalidURL(callbackURL.absoluteString)
        }

        if LocalhostOAuthCallbackBridge.shouldHandle(providerRedirectURL: providerRedirectURL, callbackURL: callbackURL) {
            let bridge = try await LocalhostOAuthCallbackBridge(
                providerRedirectURL: providerRedirectURL,
                callbackURL: callbackURL
            )
            localhostBridge = bridge
        } else {
            localhostBridge = nil
        }

        defer {
            localhostBridge?.stop()
            localhostBridge = nil
        }

        do {
            let completedCallbackURL = try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<URL, Error>) in
                let session = ASWebAuthenticationSession(
                    url: authorizeURL,
                    callbackURLScheme: callbackScheme,
                    completionHandler: makeAuthenticationSessionCompletion(continuation: continuation)
                )

                session.prefersEphemeralWebBrowserSession = true
                session.presentationContextProvider = presentationProvider
                self.session = session

                if !session.start() {
                    continuation.resume(throwing: APIError.invalidResponse)
                    self.session = nil
                }
            }

            session = nil
            return completedCallbackURL
        } catch {
            session = nil
            throw error
        }
    }
}

private func makeAuthenticationSessionCompletion(
    continuation: CheckedContinuation<URL, Error>
) -> @Sendable (URL?, Error?) -> Void {
    { callbackURL, error in
        if let error {
            continuation.resume(throwing: error)
            return
        }

        guard let callbackURL else {
            continuation.resume(throwing: APIError.invalidResponse)
            return
        }

        continuation.resume(returning: callbackURL)
    }
}

private final class LocalhostOAuthCallbackBridge: @unchecked Sendable {
    private let providerRedirectURL: URL
    private let callbackURL: URL
    private let queue = DispatchQueue(label: "ai.fawx.oauth.localhost-bridge")
    private var listener: NWListener?
    private var hasHandledCallback = false

    init(providerRedirectURL: URL, callbackURL: URL) async throws {
        self.providerRedirectURL = providerRedirectURL
        self.callbackURL = callbackURL

        guard let port = Self.port(for: providerRedirectURL) else {
            throw APIError.invalidURL(providerRedirectURL.absoluteString)
        }

        let listener: NWListener
        do {
            listener = try NWListener(using: .tcp, on: port)
        } catch {
            throw APIError.streamError("Couldn't start local OAuth callback bridge: \(error.localizedDescription)")
        }

        self.listener = listener
        listener.newConnectionHandler = { [weak self] connection in
            self?.handle(connection)
        }

        let resumeState = ContinuationResumeState()

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let resumeIfNeeded: @Sendable (Result<Void, Error>) -> Void = { result in
                guard resumeState.markResumedIfNeeded() else {
                    return
                }

                switch result {
                case .success:
                    continuation.resume(returning: ())
                case .failure(let error):
                    continuation.resume(throwing: error)
                }
            }

            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    resumeIfNeeded(.success(()))
                case .failed(let error):
                    resumeIfNeeded(.failure(APIError.streamError(
                        "Couldn't start local OAuth callback bridge: \(error.localizedDescription)"
                    )))
                case .cancelled:
                    resumeIfNeeded(.failure(APIError.streamError(
                        "Local OAuth callback bridge was cancelled before it became ready."
                    )))
                default:
                    break
                }
            }

            listener.start(queue: queue)
        }
    }

    func stop() {
        listener?.cancel()
        listener = nil
    }

    static func shouldHandle(providerRedirectURL: URL, callbackURL: URL) -> Bool {
        guard let scheme = providerRedirectURL.scheme?.lowercased() else {
            return false
        }

        guard scheme == "http" || scheme == "https" else {
            return false
        }

        guard let host = providerRedirectURL.host?.lowercased() else {
            return false
        }

        let isLoopbackHost = host == "localhost"
            || host == "127.0.0.1"
            || host == "::1"
            || host == "[::1]"
        guard isLoopbackHost else {
            return false
        }

        return providerRedirectURL.scheme?.lowercased() != callbackURL.scheme?.lowercased()
    }

    private static func port(for url: URL) -> NWEndpoint.Port? {
        let resolvedPort: Int
        if let explicitPort = url.port {
            resolvedPort = explicitPort
        } else if url.scheme?.lowercased() == "https" {
            resolvedPort = 443
        } else {
            resolvedPort = 80
        }

        return NWEndpoint.Port(rawValue: UInt16(resolvedPort))
    }

    private func handle(_ connection: NWConnection) {
        connection.start(queue: queue)
        receiveRequest(on: connection, accumulated: Data())
    }

    private func receiveRequest(on connection: NWConnection, accumulated: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { [weak self] data, _, isComplete, error in
            guard let self else {
                connection.cancel()
                return
            }

            if error != nil {
                connection.cancel()
                return
            }

            let nextData = accumulated + (data ?? Data())
            if nextData.containsHTTPHeaderTerminator || isComplete {
                self.respond(to: nextData, on: connection)
            } else {
                self.receiveRequest(on: connection, accumulated: nextData)
            }
        }
    }

    private func respond(to requestData: Data, on connection: NWConnection) {
        guard let request = String(data: requestData, encoding: .utf8) else {
            send(status: "400 Bad Request", body: "Invalid OAuth callback request.", on: connection)
            return
        }

        let requestLine = request.components(separatedBy: "\r\n").first
        let parts = requestLine?.split(separator: " ", omittingEmptySubsequences: true) ?? []
        guard parts.count >= 2, parts[0] == "GET" else {
            send(status: "405 Method Not Allowed", body: "Unsupported OAuth callback method.", on: connection)
            return
        }

        guard let incomingURL = incomingRequestURL(pathAndQuery: String(parts[1])) else {
            send(status: "400 Bad Request", body: "Couldn't parse OAuth callback URL.", on: connection)
            return
        }

        guard incomingURL.path == providerRedirectURL.path else {
            send(status: "404 Not Found", body: "OAuth callback path not found.", on: connection)
            return
        }

        guard !hasHandledCallback else {
            send(status: "409 Conflict", body: "OAuth callback already received.", on: connection)
            return
        }
        hasHandledCallback = true

        guard let bridgedCallbackURL = bridgedCallbackURL(for: incomingURL) else {
            send(status: "500 Internal Server Error", body: "Couldn't forward OAuth callback to Fawx.", on: connection)
            return
        }

        sendRedirect(to: bridgedCallbackURL, on: connection)
    }

    private func incomingRequestURL(pathAndQuery: String) -> URL? {
        if let absoluteURL = URL(string: pathAndQuery), absoluteURL.scheme != nil {
            return absoluteURL
        }

        guard var components = URLComponents(url: providerRedirectURL, resolvingAgainstBaseURL: false) else {
            return nil
        }

        guard let targetComponents = URLComponents(string: pathAndQuery) else {
            return nil
        }

        components.percentEncodedPath = targetComponents.percentEncodedPath
        components.percentEncodedQuery = targetComponents.percentEncodedQuery
        components.fragment = nil
        return components.url
    }

    private func bridgedCallbackURL(for incomingURL: URL) -> URL? {
        guard
            var outgoingComponents = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false),
            let incomingComponents = URLComponents(url: incomingURL, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        outgoingComponents.queryItems = incomingComponents.queryItems
        return outgoingComponents.url
    }

    private func sendRedirect(to url: URL, on connection: NWConnection) {
        let escapedURL = Self.escapeHTML(url.absoluteString)
        let body = """
        <html>
        <head>
        <meta http-equiv="refresh" content="0;url=\(escapedURL)">
        <script>window.location.replace(\(Self.javaScriptStringLiteral(url.absoluteString)));</script>
        </head>
        <body>
        <p>Returning to Fawx...</p>
        <p><a href="\(escapedURL)">Tap here if nothing happens.</a></p>
        </body>
        </html>
        """

        send(
            status: "302 Found",
            headers: [
                "Location: \(url.absoluteString)",
                "Cache-Control: no-store",
                "Pragma: no-cache"
            ],
            body: body,
            on: connection
        )

        stop()
    }

    private func send(
        status: String,
        headers: [String] = [],
        body: String,
        on connection: NWConnection
    ) {
        var headerLines = [
            "HTTP/1.1 \(status)",
            "Content-Type: text/html; charset=utf-8",
            "Content-Length: \(body.utf8.count)",
            "Connection: close"
        ]
        headerLines.append(contentsOf: headers)

        let response = headerLines.joined(separator: "\r\n")
            + "\r\n\r\n"
            + body

        connection.send(content: Data(response.utf8), completion: .contentProcessed { _ in
            connection.cancel()
        })
    }

    private static func escapeHTML(_ value: String) -> String {
        value
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }

    private static func javaScriptStringLiteral(_ value: String) -> String {
        let escaped = value
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        return "\"\(escaped)\""
    }
}

private final class ContinuationResumeState: @unchecked Sendable {
    private let lock = NSLock()
    private var hasResumed = false

    func markResumedIfNeeded() -> Bool {
        lock.lock()
        defer { lock.unlock() }

        guard !hasResumed else {
            return false
        }

        hasResumed = true
        return true
    }
}

private extension Data {
    var containsHTTPHeaderTerminator: Bool {
        range(of: Data("\r\n\r\n".utf8)) != nil
    }
}

private final class OAuthPresentationContextProvider: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
#if os(macOS)
        NSApp.keyWindow ?? NSApp.windows.first ?? ASPresentationAnchor()
#else
        if let scene = UIApplication.shared.connectedScenes
            .compactMap({ $0 as? UIWindowScene })
            .first(where: { $0.activationState == .foregroundActive }),
           let window = scene.windows.first(where: \.isKeyWindow) ?? scene.windows.first {
            return window
        }

        return ASPresentationAnchor()
#endif
    }
}
