import XCTest
@testable import Fawx

final class SessionMessageTests: XCTestCase {
    func testDecodedMessagesReceiveUniqueIDsWhenPayloadsMatch() throws {
        let data = Data(
            """
            {
              "messages": [
                { "role": "assistant", "content": "Same", "timestamp": 123 },
                { "role": "assistant", "content": "Same", "timestamp": 123 }
              ],
              "total": 2
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(MessagesResponse.self, from: data)

        XCTAssertEqual(response.messages.count, 2)
        XCTAssertNotEqual(response.messages[0].id, response.messages[1].id)
    }

    func testStructuredToolMessagesPreserveReadableContent() throws {
        let data = Data(
            """
            {
              "messages": [
                {
                  "role": "assistant",
                  "content": [
                    {
                      "type": "tool_use",
                      "id": "call_1",
                      "name": "read_file",
                      "input": { "path": "README.md" }
                    }
                  ],
                  "timestamp": 123
                },
                {
                  "role": "tool",
                  "content": [
                    {
                      "type": "tool_result",
                      "tool_use_id": "call_1",
                      "content": "hello from the tool"
                    }
                  ],
                  "timestamp": 124
                }
              ],
              "total": 2
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(MessagesResponse.self, from: data)

        XCTAssertEqual(response.messages.map(\.role), [.assistant, .tool])
        XCTAssertTrue(response.messages[0].content.contains("[read_file]"))
        XCTAssertTrue(response.messages[0].content.contains("README.md"))
        XCTAssertEqual(response.messages[1].content, "hello from the tool")
    }
}
