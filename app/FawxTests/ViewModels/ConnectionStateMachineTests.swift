import XCTest
@testable import Fawx

final class ConnectionStateMachineTests: XCTestCase {
    func testIssueKindReturnsAuthenticationForUnauthorizedResponse() {
        let kind = ConnectionStateMachine.issueKind(for: APIError.httpStatus(401, nil))

        XCTAssertEqual(kind, .authentication)
    }

    func testIssueKindReturnsConnectivityForTimeout() {
        let kind = ConnectionStateMachine.issueKind(for: URLError(.timedOut))

        XCTAssertEqual(kind, .connectivity)
    }

    func testShouldHandleAsConnectionIssueReturnsFalseForNonConnectionError() {
        let shouldHandle = ConnectionStateMachine.shouldHandleAsConnectionIssue(
            APIError.decoding("bad payload")
        )

        XCTAssertFalse(shouldHandle)
    }

    func testIssueKindReturnsOtherForServerErrorResponse() {
        let kind = ConnectionStateMachine.issueKind(for: APIError.httpStatus(500, nil))

        XCTAssertEqual(kind, .other)
    }

    func testShouldHandleAsConnectionIssueReturnsFalseForServerErrorResponse() {
        let shouldHandle = ConnectionStateMachine.shouldHandleAsConnectionIssue(
            APIError.httpStatus(500, "tool call failed")
        )

        XCTAssertFalse(shouldHandle)
    }

    func testFailureStatusReconnectsOnConnectivityFailureWhenAllowed() {
        let status = ConnectionStateMachine.failureStatus(
            for: URLError(.cannotConnectToHost),
            allowReconnect: true
        )

        XCTAssertEqual(status, .reconnecting)
    }

    func testFailureStatusDisconnectsOnAuthenticationFailure() {
        let status = ConnectionStateMachine.failureStatus(
            for: APIError.httpStatus(401, nil),
            allowReconnect: true
        )

        XCTAssertEqual(status, .disconnected)
    }

    func testRetryFailureStatusDisconnectsAfterMaximumAttempts() {
        let status = ConnectionStateMachine.retryFailureStatus(
            for: URLError(.cannotConnectToHost),
            reconnectAttempt: 5
        )

        XCTAssertEqual(status, .disconnected)
    }
}
