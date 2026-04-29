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

    func testStructuredToolMessagesPreserveReadableContentWithoutDumpingToolPayloads() throws {
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
                      "content": "hello from the tool",
                      "is_error": false
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
        XCTAssertEqual(response.messages[1].content, "Tool output available.")
        XCTAssertFalse(response.messages[1].content.contains("hello from the tool"))
    }

    func testStructuredToolMessagesRetainBlocksForTranscriptReconstruction() throws {
        let data = Data(
            """
            {
              "messages": [
                {
                  "role": "assistant",
                  "content": [
                    {
                      "type": "text",
                      "text": "Let me check."
                    },
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
                      "content": "hello from the tool",
                      "is_error": true
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

        XCTAssertEqual(response.messages[0].transcriptDisplayText, "Let me check.")
        XCTAssertEqual(response.messages[0].contentBlocks.count, 2)
        XCTAssertEqual(response.messages[1].contentBlocks.count, 1)

        guard case .toolUse(let id, let name, let input) = response.messages[0].contentBlocks[1] else {
            return XCTFail("Expected tool_use block")
        }

        XCTAssertEqual(id, "call_1")
        XCTAssertEqual(name, "read_file")
        XCTAssertEqual(input, .object(["path": .string("README.md")]))

        guard case .toolResult(let toolUseID, let content, let isError) = response.messages[1].contentBlocks[0] else {
            return XCTFail("Expected tool_result block")
        }

        XCTAssertEqual(toolUseID, "call_1")
        XCTAssertEqual(content, .string("hello from the tool"))
        XCTAssertEqual(isError, true)
    }

    func testStructuredToolMessagesDecodeLegacyToolResultWithoutIsError() throws {
        let data = Data(
            """
            {
              "messages": [
                {
                  "role": "tool",
                  "content": [
                    {
                      "type": "tool_result",
                      "tool_use_id": "call_1",
                      "content": "legacy output"
                    }
                  ],
                  "timestamp": 124
                }
              ],
              "total": 1
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(MessagesResponse.self, from: data)

        guard case .toolResult(let toolUseID, let content, let isError) = response.messages[0].contentBlocks[0] else {
            return XCTFail("Expected tool_result block")
        }

        XCTAssertEqual(toolUseID, "call_1")
        XCTAssertEqual(content, .string("legacy output"))
        XCTAssertNil(isError)
    }

    func testStructuredAttachmentMessagesDecodeBlocksAndTranscriptMarkers() throws {
        let data = Data(
            """
            {
              "messages": [
                {
                  "role": "user",
                  "content": [
                    {
                      "type": "image",
                      "media_type": "image/png",
                      "data": "aW1hZ2U="
                    },
                    {
                      "type": "document",
                      "media_type": "application/pdf",
                      "data": "cGRm",
                      "filename": "brief.pdf"
                    },
                    {
                      "type": "text",
                      "text": "Please review"
                    }
                  ],
                  "timestamp": 123
                }
              ],
              "total": 1
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(MessagesResponse.self, from: data)
        let message = try XCTUnwrap(response.messages.first)

        XCTAssertEqual(
            message.transcriptDisplayText,
            "[image]\n\n[document: brief.pdf]\n\nPlease review"
        )
        XCTAssertEqual(message.content, "[image]\n\n[document: brief.pdf]\n\nPlease review")

        guard case .image(let imageMediaType, let imageData) = message.contentBlocks[0] else {
            return XCTFail("Expected image block")
        }

        XCTAssertEqual(imageMediaType, "image/png")
        XCTAssertEqual(imageData, "aW1hZ2U=")

        guard case .document(let documentMediaType, let documentData, let filename) = message.contentBlocks[1] else {
            return XCTFail("Expected document block")
        }

        XCTAssertEqual(documentMediaType, "application/pdf")
        XCTAssertEqual(documentData, "cGRm")
        XCTAssertEqual(filename, "brief.pdf")
    }

    func testAttachmentBlocksEncodeMediaTypeDataAndFilename() throws {
        let message = SessionMessage(
            role: .user,
            contentBlocks: [
                .image(mediaType: "image/png", data: "aW1hZ2U="),
                .document(mediaType: "application/pdf", data: "cGRm", filename: "brief.pdf"),
            ],
            timestamp: 123
        )

        let data = try JSONEncoder().encode(message)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        let content = try XCTUnwrap(json["content"] as? [[String: Any]])

        XCTAssertEqual(content.count, 2)
        XCTAssertEqual(content[0]["type"] as? String, "image")
        XCTAssertEqual(content[0]["media_type"] as? String, "image/png")
        XCTAssertEqual(content[0]["data"] as? String, "aW1hZ2U=")

        XCTAssertEqual(content[1]["type"] as? String, "document")
        XCTAssertEqual(content[1]["media_type"] as? String, "application/pdf")
        XCTAssertEqual(content[1]["data"] as? String, "cGRm")
        XCTAssertEqual(content[1]["filename"] as? String, "brief.pdf")
    }
}
