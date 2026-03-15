import Foundation

enum ConnectionIssueKind: Equatable {
    case authentication
    case connectivity
    case other
}

enum ConnectionStateMachine {
    static func issueKind(for error: Error) -> ConnectionIssueKind {
        if case APIError.httpStatus(let code, _) = error, code == 401 {
            return .authentication
        }

        if let urlError = error as? URLError {
            switch urlError.code {
            case .timedOut,
                    .cannotFindHost,
                    .cannotConnectToHost,
                    .networkConnectionLost,
                    .dnsLookupFailed,
                    .notConnectedToInternet:
                return .connectivity
            default:
                break
            }
        }

        if case APIError.invalidResponse = error {
            return .connectivity
        }

        if case APIError.httpStatus(let code, _) = error, code == 408 || (500 ... 599).contains(code) {
            return .connectivity
        }

        return .other
    }

    static func shouldHandleAsConnectionIssue(_ error: Error) -> Bool {
        issueKind(for: error) != .other
    }

    static func failureStatus(for error: Error, allowReconnect: Bool) -> ConnectionStatus {
        switch issueKind(for: error) {
        case .authentication:
            return .disconnected
        case .connectivity:
            return allowReconnect ? .reconnecting : .disconnected
        case .other:
            return .disconnected
        }
    }

    static func retryFailureStatus(
        for error: Error,
        reconnectAttempt: Int,
        maximumAttempts: Int = 5
    ) -> ConnectionStatus {
        switch issueKind(for: error) {
        case .connectivity where reconnectAttempt < maximumAttempts:
            return .reconnecting
        case .authentication, .connectivity, .other:
            return .disconnected
        }
    }
}
