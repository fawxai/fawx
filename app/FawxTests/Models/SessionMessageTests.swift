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
}
