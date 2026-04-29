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

    func testParseLineParsesPhaseBoundaryEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: phase_boundary"), [])
        XCTAssertEqual(try parser.parseLine(#"data: {"phase":"finalizing"}"#), [])

        let events = try parser.parseLine("")

        XCTAssertEqual(events, [.transcriptPhaseBoundary("finalizing")])
    }

    func testParseLineParsesCompletedSummaryEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: completed_summary"), [])
        XCTAssertEqual(
            try parser.parseLine(#"data: {"text":"Worked this turn: 2 searches."}"#),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(events, [.completedSummary("Worked this turn: 2 searches.")])
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
            try parser.parseLine(
                #"data: {"kind":"implementing","message":"Implementing the committed plan."}"#),
            []
        )

        let events = try parser.parseLine("")

        XCTAssertEqual(
            events,
            [.progress(kind: "implementing", message: "Implementing the committed plan.")]
        )
    }

    func testParseLineParsesPreviewTextAndResetEvents() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: text_preview_delta"), [])
        XCTAssertEqual(try parser.parseLine(#"data: {"text":"partial answer"}"#), [])
        XCTAssertEqual(try parser.parseLine(""), [.textPreviewDelta("partial answer")])

        XCTAssertEqual(try parser.parseLine("event: text_reset"), [])
        XCTAssertEqual(try parser.parseLine("data: {}"), [])
        XCTAssertEqual(try parser.parseLine(""), [.textReset])
    }

    func testParseLineParsesTypedActivityAndFinalAnswerEvents() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: working_narration_delta"), [])
        XCTAssertEqual(try parser.parseLine(#"data: {"text":"I’ll inspect the diff."}"#), [])
        XCTAssertEqual(try parser.parseLine(""), [.workingNarrationDelta("I’ll inspect the diff.")])

        XCTAssertEqual(try parser.parseLine("event: working_narration_delta"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"text":"I'm reading app.","voiceover_suppressed":true}"#),
            []
        )
        XCTAssertEqual(
            try parser.parseLine(""),
            [.workingNarrationDelta("I'm reading app.", voiceoverSuppressed: true)]
        )

        XCTAssertEqual(try parser.parseLine("event: activity_start"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"id":"round-1","title":"Ran 1 tool","kind":"tool_round"}"#),
            []
        )
        XCTAssertEqual(
            try parser.parseLine(""),
            [.activityStart(id: "round-1", title: "Ran 1 tool", kind: "tool_round")]
        )

        XCTAssertEqual(try parser.parseLine("event: activity_tool_call_start"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":"round-1","id":"call-1","name":"run_command"}"#),
            []
        )
        XCTAssertEqual(
            try parser.parseLine(""),
            [.activityToolCallStart(activityID: "round-1", id: "call-1", name: "run_command")]
        )

        XCTAssertEqual(try parser.parseLine("event: activity_tool_result"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":"round-1","id":"call-1","tool_name":"run_command","output":"ok","is_error":false}"#
            ),
            []
        )
        XCTAssertEqual(
            try parser.parseLine(""),
            [
                .activityToolResult(
                    activityID: "round-1",
                    id: "call-1",
                    toolName: "run_command",
                    output: "ok",
                    isError: false
                )
            ]
        )

        XCTAssertEqual(try parser.parseLine("event: tool_progress"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":"round-1","id":"call-1","tool_name":"run_command","class":"mutation","target":"git diff","advances_slot":"mutation:git-diff","outcome":"advanced"}"#
            ),
            []
        )
        XCTAssertEqual(
            try parser.parseLine(""),
            [
                .toolProgress(
                    activityID: "round-1",
                    id: "call-1",
                    toolName: "run_command",
                    category: "mutation",
                    target: "git diff",
                    advancesSlot: "mutation:git-diff",
                    outcome: "advanced"
                )
            ]
        )

        XCTAssertEqual(try parser.parseLine("event: final_answer_delta"), [])
        XCTAssertEqual(try parser.parseLine(#"data: {"text":"Done."}"#), [])
        XCTAssertEqual(try parser.parseLine(""), [.finalAnswerDelta("Done.")])
    }

    func testParseLineSerializesToolCallCompleteArgumentsAsJSON() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"id":"call-1","name":"read_file","arguments":{"path":"README.md"}}"#
            ),
            []
        )

        let events = try parser.parseLine("")
        guard let event = events.first,
            case .toolCallComplete(let id, let name, let arguments) = event,
            id == "call-1",
            name == "read_file"
        else {
            return XCTFail("Expected a completed tool call event with serialized arguments")
        }
        let decodedArguments = try JSONDecoder().decode(JSONValue.self, from: Data(arguments.utf8))
        XCTAssertEqual(decodedArguments, .object(["path": .string("README.md")]))
    }

    func testParseLinePreservesStringEncodedToolCallCompleteArguments() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"id":"call-1","name":"run_command","arguments":"{\"command\":\"git diff --stat\"}"}"#
            ),
            []
        )

        let events = try parser.parseLine("")
        guard let event = events.first,
            case .toolCallComplete(let id, let name, let arguments) = event,
            id == "call-1",
            name == "run_command"
        else {
            return XCTFail("Expected a completed tool call event with raw string arguments")
        }
        let decodedArguments = try JSONDecoder().decode(JSONValue.self, from: Data(arguments.utf8))
        XCTAssertEqual(decodedArguments, .object(["command": .string("git diff --stat")]))
    }

    func testParseLineSerializesActivityToolCallCompleteArgumentsAsJSON() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: activity_tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":"round-1","id":"call-1","name":"run_command","arguments":{"command":"git diff --stat"}}"#
            ),
            []
        )

        let events = try parser.parseLine("")
        guard let event = events.first,
            case .activityToolCallComplete(let activityID, let id, let name, let arguments) = event,
            activityID == "round-1",
            id == "call-1",
            name == "run_command"
        else {
            return XCTFail(
                "Expected a completed activity tool call event with serialized arguments")
        }
        let decodedArguments = try JSONDecoder().decode(JSONValue.self, from: Data(arguments.utf8))
        XCTAssertEqual(decodedArguments, .object(["command": .string("git diff --stat")]))
    }

    func testParseLinePreservesStringEncodedActivityToolCallCompleteArguments() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: activity_tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":"round-1","id":"call-1","name":"run_command","arguments":"{\"command\":\"git diff --stat\"}"}"#
            ),
            []
        )

        let events = try parser.parseLine("")
        guard let event = events.first,
            case .activityToolCallComplete(let activityID, let id, let name, let arguments) = event,
            activityID == "round-1",
            id == "call-1",
            name == "run_command"
        else {
            return XCTFail(
                "Expected a completed activity tool call event with raw string arguments")
        }
        let decodedArguments = try JSONDecoder().decode(JSONValue.self, from: Data(arguments.utf8))
        XCTAssertEqual(decodedArguments, .object(["command": .string("git diff --stat")]))
    }

    func testParseLineRejectsMalformedToolCallCompletePayloads() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(#"data: {"id":"call-1","arguments":{"path":"README.md"}"#),
            []
        )

        XCTAssertThrowsError(try parser.parseLine(""))
    }

    func testParseLineRejectsMissingRequiredTypedPayloadFields() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: activity_start"), [])
        XCTAssertEqual(try parser.parseLine(#"data: {"title":"Missing required id"}"#), [])

        XCTAssertThrowsError(try parser.parseLine(""))
    }

    func testParseLineRejectsNullRequiredTypedPayloadFields() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: activity_tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"activity_id":null,"id":"call-1","name":"run_command","arguments":null}"#
            ),
            []
        )

        XCTAssertThrowsError(try parser.parseLine(""))
    }

    func testParseLineAcceptsNullOptionalTypedPayloadFields() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: tool_call_complete"), [])
        XCTAssertEqual(
            try parser.parseLine(
                #"data: {"id":null,"name":null,"arguments":null}"#
            ),
            []
        )

        XCTAssertEqual(
            try parser.parseLine(""),
            [.toolCallComplete(id: nil, name: nil, arguments: "")]
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
            try parser.parseLine(
                #"data: {"id":"prompt-1","action":"write","path":"/tmp/report.md","tier":2}"#),
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
                        expiresAt: 1_742_000_000
                    )
                )
            ]
        )
    }
}
