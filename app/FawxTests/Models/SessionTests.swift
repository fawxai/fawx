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

    func testSessionMemorySanitizedForSavingTrimsBlankValues() {
        let memory = SessionMemory(
            project: "  Compaction UX  ",
            currentState: "   ",
            keyDecisions: ["Keep the banner subtle", "   "],
            activeFiles: [" app/Fawx/Views/Shared/SessionMemoryPanel.swift "],
            customContext: ["Support older servers gracefully", ""],
            lastUpdated: 42
        )

        let sanitized = memory.sanitizedForSaving

        XCTAssertEqual(sanitized.project, "Compaction UX")
        XCTAssertNil(sanitized.currentState)
        XCTAssertEqual(sanitized.keyDecisions, ["Keep the banner subtle"])
        XCTAssertEqual(sanitized.activeFiles, ["app/Fawx/Views/Shared/SessionMemoryPanel.swift"])
        XCTAssertEqual(sanitized.customContext, ["Support older servers gracefully"])
        XCTAssertEqual(sanitized.lastUpdated, 42)
    }

    func testSessionMemoryEstimatedTokensIsZeroForEmptyMemory() {
        XCTAssertEqual(SessionMemory().estimatedTokens, 0)
    }

    func testSessionMemoryEstimatedTokensReflectRenderedMemory() {
        let memory = SessionMemory(
            project: "Compaction UX",
            currentState: "Add a memory editor",
            keyDecisions: ["Use a sheet"],
            activeFiles: ["app/Fawx/Views/Shared/SessionMemoryPanel.swift"],
            customContext: ["Keep the copy concise"]
        )

        XCTAssertGreaterThan(memory.estimatedTokens, 0)
        XCTAssertGreaterThan(memory.estimatedTokens, memory.keyDecisions.count)
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

    func testMacOSContentViewLetsChatSurfaceFlexBeforeCompressingSidePanes() throws {
        let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")
        let containerSource = try snippet(
            in: source,
            startingAt: "    @ViewBuilder\n    private var chatDetailContainer: some View {",
            endingBefore: "\n\n    private var statusBarView: some View {"
        )

        XCTAssertFalse(source.contains("WindowMinimumSizeConfigurator("))
        XCTAssertFalse(source.contains("chatDetailMinWidth"))
        XCTAssertTrue(containerSource.contains(".frame(maxWidth: .infinity, maxHeight: .infinity)"))
        XCTAssertTrue(containerSource.contains("minWidth: Layout.compactGitPanelMinWidth"))
    }

    func testStatusBarIncludesSessionMemoryButton() throws {
        let source = try sourceFile(at: "app/Fawx/Views/Shared/StatusBar.swift")

        XCTAssertTrue(source.contains("accessibilityIdentifier(\"sessionMemoryButton\")"))
        XCTAssertTrue(source.contains("accessibilityLabel(\"Open session memory\")"))
        XCTAssertTrue(source.contains("Text(\"Memory\")"))
    }

    func testChatDetailViewPresentsSessionMemoryPanel() throws {
        let source = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")

        XCTAssertTrue(source.contains("SessionMemoryPanel(appState: appState, session: session)"))
        XCTAssertTrue(source.contains("presentedSessionMemory"))
    }

    func testSessionMemoryPanelValidatesAndCountsActiveFiles() throws {
        let source = try sourceFile(at: "app/Fawx/Views/Shared/SessionMemoryPanel.swift")

        XCTAssertTrue(source.contains("\\(sanitizedDraft.activeFiles.count) / \\(SessionMemory.maxItems) active files"))
        XCTAssertTrue(source.contains("Keep active files to \\(SessionMemory.maxItems) items or fewer."))
        XCTAssertTrue(source.contains(".disabled(isDisabled || isAtItemLimit)"))
    }

    func testChatDetailViewStylesEmergencyCompactionBanner() throws {
        let source = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")

        XCTAssertTrue(source.contains("isEmergency"))
        XCTAssertTrue(source.contains("Color.fawxWarning.opacity(0.12)"))
        XCTAssertTrue(source.contains("Color.fawxWarning.opacity(0.45)"))
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
