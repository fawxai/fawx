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

    func testFinishFlushesTrailingDoneEvent() throws {
        var parser = SSEParser()

        XCTAssertEqual(try parser.parseLine("event: done"), [])
        XCTAssertEqual(try parser.parseLine("data: {\"response\":\"All set\"}"), [])

        let events = try parser.finish()

        XCTAssertEqual(events, [.done(response: "All set")])
    }
}
