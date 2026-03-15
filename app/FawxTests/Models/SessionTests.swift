import XCTest
@testable import Fawx

final class SessionTests: XCTestCase {
    func testSummarizedSessionTitleStripsCommonPromptPrefix() {
        let title = summarizedSessionTitle(from: "Hey Fawx, please help me with the streaming retry bug")

        XCTAssertEqual(title, "Streaming retry bug")
    }

    func testStrippedSessionPromptPrefixRemovesArticleAfterPrefix() {
        let stripped = strippedSessionPromptPrefix(from: "Please help me with the build issue")

        XCTAssertEqual(stripped, "build issue")
    }

    func testTruncateSessionTitleStopsAtWordBoundary() {
        let title = truncateSessionTitle("This session title should stop before the next word", maxLength: 26)

        XCTAssertEqual(title, "This session title should...")
    }
}
