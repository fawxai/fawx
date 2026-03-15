import Foundation
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

final class IOSViewSourceRegressionTests: XCTestCase {
    func testSessionListViewDoesNotDuplicateSessionRowForSplitLayout() throws {
        let source = try sourceFile(at: "app/Fawx/Views/iOS/SessionListView.swift")
        let sessionRowSource = try snippet(
            in: source,
            startingAt: "    @ViewBuilder\n    private func sessionRow(for session: Session) -> some View {",
            endingBefore: "\n\n    private var usesSplitLayout: Bool {"
        )

        XCTAssertFalse(
            sessionRowSource.contains("if usesSplitLayout"),
            "sessionRow(for:) should not duplicate its button content behind split-layout branches."
        )
    }

    func testIOSSettingsViewDoesNotContainAlwaysTrueSectionFilters() throws {
        let source = try sourceFile(at: "app/Fawx/Views/iOS/iOSSettingsView.swift")

        for token in [
            "showsConnectionSection",
            "showsServerSection",
            "showsAppearanceSection",
            "showsStatusSection",
            "matchesSettingsSearch("
        ] {
            XCTAssertFalse(
                source.contains(token),
                "Expected iOSSettingsView.swift to remove the always-true settings filter stub: \(token)"
            )
        }

        XCTAssertTrue(source.contains("Section(\"Connection\")"))
        XCTAssertTrue(source.contains("Section(\"Server\")"))
        XCTAssertTrue(source.contains("Section(\"Appearance\")"))
    }

    private func sourceFile(at relativePath: String) throws -> String {
        try String(contentsOf: repositoryRoot().appendingPathComponent(relativePath), encoding: .utf8)
    }

    private func repositoryRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    private func snippet(
        in source: String,
        startingAt startMarker: String,
        endingBefore endMarker: String
    ) throws -> Substring {
        let startRange = try XCTUnwrap(
            source.range(of: startMarker),
            "Missing start marker in source file."
        )
        let endRange = try XCTUnwrap(
            source.range(of: endMarker, options: [], range: startRange.upperBound..<source.endIndex),
            "Missing end marker in source file."
        )

        return source[startRange.lowerBound..<endRange.lowerBound]
    }
}
