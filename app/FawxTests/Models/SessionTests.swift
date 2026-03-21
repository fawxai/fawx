import Foundation
import XCTest
@testable import Fawx

final class ContentViewLayoutTests: XCTestCase {
    func testContentViewLayoutWidensMinimumWindowWhenGitPanelIsVisible() {
        let defaultMinimumWidth = ContentView.Layout.resolvedMinimumWindowWidth(showingGitPanel: false)
        let gitPanelMinimumWidth = ContentView.Layout.resolvedMinimumWindowWidth(showingGitPanel: true)

        XCTAssertEqual(defaultMinimumWidth, ContentView.Layout.minimumWindowWidth)
        XCTAssertEqual(gitPanelMinimumWidth, ContentView.Layout.minimumWindowWidthWithGitPanel)
        XCTAssertGreaterThan(gitPanelMinimumWidth, defaultMinimumWidth)
        XCTAssertEqual(
            gitPanelMinimumWidth,
            ContentView.Layout.sidebarMinWidth
                + ContentView.Layout.chatDetailMinWidth
                + ContentView.Layout.compactGitPanelMinWidth
                + ContentView.Layout.splitDividerWidthAllowance
        )
    }
}

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

    func testFilterSessionSectionsMatchesTitlePreviewModelAndKey() {
        let sections = [
            SessionSection(
                title: "Today",
                sessions: [
                    makeSession(
                        key: "sess-alpha",
                        label: "Debug streaming issue",
                        preview: "The SSE connection drops after 30 seconds",
                        model: "gpt-5.4"
                    ),
                    makeSession(
                        key: "sess-beta",
                        label: "Git pane polish",
                        preview: "Need a cleaner diff viewer",
                        model: "claude-sonnet"
                    ),
                ]
            )
        ]

        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: "streaming").first?.sessions.map(\.id), ["sess-alpha"])
        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: "diff viewer").first?.sessions.map(\.id), ["sess-beta"])
        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: "gpt-5.4").first?.sessions.map(\.id), ["sess-alpha"])
        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: "sess-beta").first?.sessions.map(\.id), ["sess-beta"])
    }

    func testFilterSessionSectionsRemovesEmptyGroupsAndReturnsOriginalSectionsForBlankQuery() {
        let today = SessionSection(
            title: "Today",
            sessions: [makeSession(key: "sess-today", label: "Session browser polish")]
        )
        let older = SessionSection(
            title: "Older",
            sessions: [makeSession(key: "sess-older", label: "Fleet panel")]
        )
        let sections = [today, older]

        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: " ").map(\.title), ["Today", "Older"])
        XCTAssertEqual(SessionViewModel.filterSessionSections(sections, query: "browser").map(\.title), ["Today"])
    }

    func testSessionRowSubtitleTextUsesPreviewWhenAvailable() {
        let session = makeSession(
            key: "sess-preview",
            preview: "Most recent assistant reply",
            messageCount: 3
        )

        XCTAssertEqual(SessionRowView.subtitleText(for: session), "Most recent assistant reply")
    }

    func testSessionRowSubtitleTextShowsNoMessagesFallback() {
        let session = makeSession(key: "sess-empty", preview: nil, messageCount: 0)

        XCTAssertEqual(SessionRowView.subtitleText(for: session), "No messages yet")
    }

    func testSessionRowSubtitleTextShowsPluralizedMessageCounts() {
        let singleMessageSession = makeSession(key: "sess-one", preview: nil, messageCount: 1)
        let multiMessageSession = makeSession(key: "sess-many", preview: nil, messageCount: 4)

        XCTAssertEqual(SessionRowView.subtitleText(for: singleMessageSession), "1 message")
        XCTAssertEqual(SessionRowView.subtitleText(for: multiMessageSession), "4 messages")
    }

    private func makeSession(
        key: String,
        label: String? = nil,
        title: String? = nil,
        preview: String? = nil,
        model: String = "test-model",
        updatedAt: Int = 1,
        messageCount: Int = 0
    ) -> Session {
        Session(
            key: key,
            kind: .main,
            status: .idle,
            label: label,
            title: title,
            preview: preview,
            model: model,
            createdAt: 0,
            updatedAt: updatedAt,
            messageCount: messageCount
        )
    }
}

final class ViewSourceRegressionTests: XCTestCase {
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
        XCTAssertTrue(source.contains("Section(\"Manage\")"))
        XCTAssertTrue(source.contains("Section(\"Appearance\")"))
        XCTAssertTrue(source.contains("NavigationLink(value: SettingsRoute.server)"))
        XCTAssertTrue(source.contains("NavigationLink(value: SettingsRoute.permissions)"))
        XCTAssertTrue(source.contains("NavigationLink(value: SettingsRoute.synthesis)"))
        XCTAssertTrue(source.contains("NavigationLink(value: SettingsRoute.usage)"))
    }

    func testMacOSContentViewPinsSidebarColumnWidthForChatLayout() throws {
        let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")

        XCTAssertTrue(source.contains(".navigationSplitViewColumnWidth("))
        XCTAssertTrue(source.contains("min: Layout.sidebarMinWidth"))
        XCTAssertTrue(source.contains("ideal: Layout.sidebarIdealWidth"))
        XCTAssertTrue(source.contains("max: Layout.sidebarMaxWidth"))
    }

    func testMacOSContentViewSynchronizesWindowMinimumSizeWithGitPanelLayout() throws {
        let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")

        XCTAssertTrue(source.contains("WindowMinimumSizeConfigurator("))
        XCTAssertTrue(source.contains("Layout.resolvedMinimumWindowWidth(showingGitPanel: shouldShowGitPanel)"))
        XCTAssertTrue(source.contains("window.contentMinSize = targetContentMinSize"))
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
