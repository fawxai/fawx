import XCTest
@testable import Fawx

final class SSEParserTests: XCTestCase {
    func testParseLineParsesMultilinePhaseEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: phase"), [])
        XCTAssertEqual(try parser.parseLine("data: {\"phase\":"), [])
        XCTAssertEqual(try parser.parseLine("data: \"thinking\"}"), [])

        let events = try parser.parseLine("")

        XCTAssertEqual(events, [.phase("thinking")])
    }

    func testParseLineIgnoresCommentLines() throws {
        var parser = SSEParser()

        let events = try parser.parseLine(": keep-alive")

        XCTAssertEqual(events, [])
    }

    func testParseLineIgnoresUnknownEvents() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: mystery"), [])
        XCTAssertEqual(try parser.parseLine("data: {\"value\":1}"), [])

        let events = try parser.parseLine("")

        XCTAssertEqual(events, [])
    }

    func testParseLineIgnoresEmptyDataPayload() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: text_delta"), [])
        XCTAssertEqual(try parser.parseLine("data:"), [])

        let events = try parser.parseLine("")

        XCTAssertEqual(events, [])
    }

    func testParseLineParsesProgressEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: progress"), [])
        XCTAssertEqual(
            try parser.parseLine(#"data: {"kind":"implementing","message":"Implementing the committed plan."}"#),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(
            events,
            [.progress(kind: "implementing", message: "Implementing the committed plan.")]
        )
    }

    func testFinishFlushesTrailingDoneEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: done"), [])
        XCTAssertEqual(try parser.parseLine("data: {\"response\":\"All set\"}"), [])

        let events = try parser.finish()

        XCTAssertEqual(events, [.done(response: "All set")])
    }

    func testParseLineParsesPermissionPromptEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: permission_prompt"), [])
        XCTAssertEqual(
            try parser.parseLine(#"data: {"id":"prompt-1","action":"write","path":"/tmp/report.md","tier":2}"#),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(
            events,
            [
                .permissionPrompt(
                    PermissionPrompt(
                        id: "prompt-1",
                        action: "write",
                        path: "/tmp/report.md",
                        tier: 2
                    )
                )
            ]
        )
    }

    func testParseLineParsesContextCompactedEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: context_compacted"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"tier":"slide","messages_removed":12,"tokens_before":5100,"tokens_after":2900,"usage_ratio":0.42}"#
            ),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(
            events,
            [
                .contextCompacted(
                    tier: "slide",
                    messagesRemoved: 12,
                    tokensBefore: 5100,
                    tokensAfter: 2900,
                    usageRatio: 0.42
                )
            ]
        )
    }

    func testParseLineParsesLegacyPermissionPromptEventShape() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: permission_prompt"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"id":"prompt-1","tool":"shell","title":"Allow shell command","reason":"Needed to inspect the repo","request_summary":"git status --short --branch","session_scoped_allow_available":true,"expires_at":1742000000}"#
            ),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(
            events,
            [
                .permissionPrompt(
                    PermissionPrompt(
                        id: "prompt-1",
                        action: "shell command",
                        path: "git status --short --branch",
                        tier: nil,
                        sessionScopedAllowAvailable: true,
                        expiresAt: 1742000000
                    )
                )
            ]
        )
    }
}
