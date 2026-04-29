import Foundation
import XCTest
#if os(macOS)
import AppKit
#endif

@testable import Fawx

final class SessionTests: XCTestCase {
#if os(macOS)
  func testTranscriptMarkdownRendererPreservesNewlinesAndRendersATXHeadings() {
    let rendered = TranscriptMarkdownRenderer.displayText(
      for: """
      ### Summary
      Line one
      Line two

      ## Verdict
      Done
      """)

    XCTAssertEqual(
      rendered,
      """
      Summary
      Line one
      Line two

      Verdict
      Done
      """)
  }

  func testTranscriptMarkdownRendererSupportsSetextHeadingsAndPipeTables() {
    let rendered = TranscriptMarkdownRenderer.displayText(
      for: """
      Summary
      =======
      | Name | Value |
      | --- | --- |
      | Files | 3 |
      After table
      """)

    XCTAssertTrue(rendered.hasPrefix("Summary\n"))
    XCTAssertTrue(rendered.contains("Name   Value\n-----  -----\nFiles  3"))
    XCTAssertTrue(rendered.hasSuffix("\nAfter table"))
    XCTAssertFalse(rendered.contains("| --- | --- |"))
  }

  func testTranscriptMarkdownRendererSupportsAlignedPipeTablesWithoutOuterPipes() {
    let rendered = TranscriptMarkdownRenderer.displayText(
      for: """
      Name | Result
      :--- | ---:
      Search | 12
      Read files | 3
      """)

    XCTAssertEqual(
      rendered,
      """
      Name        Result
      ----------  ------
      Search      12
      Read files  3
      """)
  }

  func testTranscriptMarkdownRendererWrapsWidePipeTablesWithoutDroppingTableShape() {
    let rendered = TranscriptMarkdownRenderer.displayText(
      for: """
      | Concern | Current State |
      | --- | --- |
      | **Live working narration** | Separated at model level with `narration` property on timeline snapshots |
      | **Final answer** | Rendered as generic `.text` blocks; no clean separation from working text |
      """)

    XCTAssertTrue(rendered.hasPrefix("Concern"))
    XCTAssertTrue(rendered.contains("Current State"))
    XCTAssertTrue(rendered.contains("Live working narration"))
    XCTAssertTrue(rendered.contains("Final answer"))
    XCTAssertTrue(rendered.contains("-----------------------"))
    XCTAssertTrue(rendered.contains("timeline snapshots"))
    XCTAssertFalse(rendered.contains("Separated at model level with narration property on timeline snapshots"))
    XCTAssertFalse(rendered.contains("Rendered as generic .text blocks; no clean separation from working text"))
    XCTAssertFalse(rendered.contains("Concern: Live working narration"))
    XCTAssertFalse(rendered.contains("Current State:"))
    XCTAssertFalse(rendered.contains("| --- | --- |"))
    XCTAssertFalse(rendered.contains("**"))
    XCTAssertFalse(rendered.contains("`"))
  }

  func testTranscriptMarkdownRendererExposesNativeTableBlocks() {
    let blocks = TranscriptMarkdownRenderer.blocks(
      for: """
      Before table
      | Concern | Current State |
      | --- | --- |
      | Live narration | Separated |
      | Final answer | Distinct |
      After table
      """)

    XCTAssertEqual(blocks.count, 3)
    guard case .text(let before) = blocks[0] else {
      return XCTFail("Expected leading text block")
    }
    XCTAssertEqual(before, "Before table\n")

    guard case .table(let table) = blocks[1] else {
      return XCTFail("Expected native table block")
    }
    XCTAssertEqual(
      table.rows,
      [
        ["Concern", "Current State"],
        ["Live narration", "Separated"],
        ["Final answer", "Distinct"],
      ])
    XCTAssertEqual(table.columnCount, 2)

    guard case .text(let after) = blocks[2] else {
      return XCTFail("Expected trailing text block")
    }
    XCTAssertEqual(after, "\nAfter table")
  }

  func testTranscriptMarkdownRendererStylesMarkdownAndBareLinks() {
    let rendered = TranscriptMarkdownRenderer.attributedString(
      for: "See [docs](https://example.com/docs) and https://fawx.ai.",
      alignment: .left,
      baseFont: .systemFont(ofSize: 14)
    )
    let fullRange = NSRange(location: 0, length: rendered.length)
    var linkURLs: [String] = []
    var linkedText: [String] = []
    var underlineCount = 0

    rendered.enumerateAttribute(.link, in: fullRange) { value, range, _ in
      guard let value else {
        return
      }

      if let url = value as? URL {
        linkURLs.append(url.absoluteString)
      } else if let url = value as? NSURL {
        linkURLs.append(url.absoluteString ?? "")
      } else {
        linkURLs.append(String(describing: value))
      }
      linkedText.append((rendered.string as NSString).substring(with: range))

      if rendered.attribute(.underlineStyle, at: range.location, effectiveRange: nil) != nil {
        underlineCount += 1
      }
    }

    XCTAssertTrue(linkURLs.contains("https://example.com/docs"))
    XCTAssertTrue(linkURLs.contains("https://fawx.ai"))
    XCTAssertTrue(linkedText.contains { $0.contains("docs") })
    XCTAssertTrue(linkedText.contains { $0.contains("fawx.ai") })
    XCTAssertEqual(underlineCount, 2)
  }

  func testTranscriptMarkdownRendererPromotesInlineMarkdownStyling() throws {
    let rendered = TranscriptMarkdownRenderer.attributedString(
      for: "The UI has **partial separation** and `FinalAnswerBlock` support.",
      alignment: .left,
      baseFont: .systemFont(ofSize: 14)
    )
    let text = rendered.string as NSString
    let strongRange = text.range(of: "partial separation")
    let codeRange = text.range(of: "FinalAnswerBlock")

    XCTAssertFalse(rendered.string.contains("**"))
    XCTAssertFalse(rendered.string.contains("`"))
    XCTAssertNotEqual(strongRange.location, NSNotFound)
    XCTAssertNotEqual(codeRange.location, NSNotFound)

    let strongFont = try XCTUnwrap(
      rendered.attribute(.font, at: strongRange.location, effectiveRange: nil) as? NSFont
    )
    let codeFont = try XCTUnwrap(
      rendered.attribute(.font, at: codeRange.location, effectiveRange: nil) as? NSFont
    )

    XCTAssertTrue(strongFont.fontDescriptor.symbolicTraits.contains(.bold))
    XCTAssertTrue(codeFont.fontDescriptor.symbolicTraits.contains(.monoSpace))
    XCTAssertNotNil(rendered.attribute(.backgroundColor, at: codeRange.location, effectiveRange: nil))
  }

  func testTranscriptMarkdownRendererExposesFencedCodeBlocks() {
    let blocks = TranscriptMarkdownRenderer.blocks(
      for: """
      Before
      ```swift
      enum SessionContentBlock {
        case finalAnswer(String)
      }
      ```
      After
      """)

    XCTAssertEqual(blocks.count, 3)
    guard case .codeBlock(let language, let content) = blocks[1] else {
      return XCTFail("Expected fenced code block")
    }
    XCTAssertEqual(language, "swift")
    XCTAssertEqual(
      content,
      """
      enum SessionContentBlock {
        case finalAnswer(String)
      }
      """)
  }

  func testTranscriptMarkdownTableColumnsSizeFromContent() {
    let blocks = TranscriptMarkdownRenderer.blocks(
      for: """
      | Tiny | Longer Column |
      | --- | --- |
      | A | This cell needs materially more room than the tiny column |
      """)

    guard case .table(let table)? = blocks.first else {
      return XCTFail("Expected table block")
    }

    XCTAssertGreaterThan(table.columnWidth(at: 1), table.columnWidth(at: 0))
    XCTAssertGreaterThan(table.columnWidth(at: 1), 300)
  }
#endif

  func testSummarizedSessionTitleStripsCommonPromptPrefix() {
    let title = summarizedSessionTitle(
      from: "Hey Fawx, please help me with the streaming retry bug")

    XCTAssertEqual(title, "Streaming retry bug")
  }

  func testStrippedSessionPromptPrefixRemovesArticleAfterPrefix() {
    let stripped = strippedSessionPromptPrefix(from: "Please help me with the build issue")

    XCTAssertEqual(stripped, "build issue")
  }

  func testTruncateSessionTitleStopsAtWordBoundary() {
    let title = truncateSessionTitle(
      "This session title should stop before the next word", maxLength: 26)

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

    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: "streaming").first?.sessions.map(
        \.id), ["sess-alpha"])
    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: "diff viewer").first?.sessions.map(
        \.id), ["sess-beta"])
    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: "gpt-5.4").first?.sessions.map(\.id),
      ["sess-alpha"])
    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: "sess-beta").first?.sessions.map(
        \.id), ["sess-beta"])
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

    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: " ").map(\.title), ["Today", "Older"])
    XCTAssertEqual(
      SessionViewModel.filterSessionSections(sections, query: "browser").map(\.title), ["Today"])
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

  func testSessionMemoryDecodesMinimalPayloadUsingEmptyCollectionDefaults() throws {
    let payload = """
      {
        "project": "Compaction UX",
        "last_updated": 42
      }
      """

    let decoded = try JSONDecoder().decode(SessionMemory.self, from: Data(payload.utf8))

    XCTAssertEqual(decoded.project, "Compaction UX")
    XCTAssertNil(decoded.currentState)
    XCTAssertEqual(decoded.keyDecisions, [])
    XCTAssertEqual(decoded.activeFiles, [])
    XCTAssertEqual(decoded.customContext, [])
    XCTAssertEqual(decoded.lastUpdated, 42)
  }

  func testSessionDecodesArchivedFieldsFromBackendPayload() throws {
    let payload = """
      {
        "key": "session-archived",
        "kind": "main",
        "status": "idle",
        "label": null,
        "title": "Archived thread",
        "preview": "Stored for later",
        "model": "gpt-5.4",
        "created_at": 1710000000,
        "updated_at": 1710000100,
        "message_count": 3,
        "archived": true,
        "archived_at": 1710000200
      }
      """

    let decoded = try JSONDecoder().decode(Session.self, from: Data(payload.utf8))

    XCTAssertTrue(decoded.archived)
    XCTAssertEqual(decoded.archivedAt, 1_710_000_200)
    XCTAssertEqual(decoded.displayTitle, "Archived thread")
  }

  func testSessionDecodesMissingArchiveFieldsUsingSafeDefaults() throws {
    let payload = """
      {
        "key": "session-active",
        "kind": "main",
        "status": "idle",
        "label": null,
        "title": "Active thread",
        "preview": null,
        "model": "gpt-5.4",
        "created_at": 1710000000,
        "updated_at": 1710000100,
        "message_count": 0
      }
      """

    let decoded = try JSONDecoder().decode(Session.self, from: Data(payload.utf8))

    XCTAssertFalse(decoded.archived)
    XCTAssertNil(decoded.archivedAt)
  }

  func testSessionDecodesThinkingOverrideFromBackendPayload() throws {
    let payload = """
      {
        "key": "session-thinking",
        "kind": "main",
        "status": "idle",
        "label": null,
        "title": "Thread thinking",
        "preview": null,
        "model": "claude-opus-4-6",
        "thinking": "medium",
        "created_at": 1710000000,
        "updated_at": 1710000100,
        "message_count": 4
      }
      """

    let decoded = try JSONDecoder().decode(Session.self, from: Data(payload.utf8))

    XCTAssertEqual(decoded.thinking, .medium)
  }

  func testWorkspaceSummaryDecodesBackendPayload() throws {
    let payload = """
      {
        "workspaces": [
          {
            "id": "ws-repo",
            "name": "Repository",
            "path": "/Users/fawx/fawx",
            "kind": "repository",
            "repo": {
              "root": "/Users/fawx/fawx",
              "vcs": "git",
              "current_branch": "dev",
              "default_branch": "main",
              "origin": "git@github.com:example/fawx.git",
              "clean": true
            },
            "last_opened_at": 1710000000
          }
        ],
        "total": 1
      }
      """

    let decoded = try JSONDecoder().decode(WorkspacesResponse.self, from: Data(payload.utf8))
    let workspace = try XCTUnwrap(decoded.workspaces.first)
    let repo = try XCTUnwrap(workspace.repo)

    XCTAssertEqual(decoded.total, 1)
    XCTAssertEqual(workspace.id, "ws-repo")
    XCTAssertEqual(workspace.kind, .repository)
    XCTAssertEqual(workspace.lastOpenedAt, 1_710_000_000)
    XCTAssertEqual(repo.currentBranch, "dev")
    XCTAssertEqual(repo.defaultBranch, "main")
    XCTAssertEqual(repo.origin, "git@github.com:example/fawx.git")
    XCTAssertTrue(repo.clean)
  }

  func testWorkspaceScopeEncodesExplicitPathAsSingleValue() throws {
    let encoded = try JSONEncoder().encode(WorkspaceScope(explicitPath: "/Users/fawx/fawx"))
    let decoded = try JSONDecoder().decode(WorkspaceScope.self, from: encoded)
    let json = try JSONDecoder().decode(String.self, from: encoded)

    XCTAssertEqual(json, "/Users/fawx/fawx")
    XCTAssertEqual(decoded.requestedPath, "/Users/fawx/fawx")
  }

  func testWorkspaceScopeCanRepresentNoScope() throws {
    let encoded = try JSONEncoder().encode(WorkspaceScope())
    let decoded = try JSONDecoder().decode(WorkspaceScope.self, from: Data("null".utf8))

    XCTAssertEqual(String(decoding: encoded, as: UTF8.self), "null")
    XCTAssertNil(decoded.requestedPath)
  }

  func testThreadSummaryDecodesBackendPayload() throws {
    let payload = """
      {
        "id": "thread-1",
        "title": "Fix sidebar state",
        "kind": "coding",
        "workspace_id": "ws-repo",
        "worktree_id": "wt-1",
        "active_session_id": "session-123",
        "status": "active",
        "preview": "Wiring the new thread selection state",
        "model": "gpt-5.4",
        "created_at": 1710000000,
        "updated_at": 1710000100
      }
      """

    let decoded = try JSONDecoder().decode(ThreadSummary.self, from: Data(payload.utf8))

    XCTAssertEqual(decoded.id, "thread-1")
    XCTAssertEqual(decoded.kind, .coding)
    XCTAssertEqual(decoded.workspaceID, "ws-repo")
    XCTAssertEqual(decoded.worktreeID, "wt-1")
    XCTAssertEqual(decoded.activeSessionID, "session-123")
    XCTAssertEqual(decoded.status, .active)
    XCTAssertEqual(decoded.preview, "Wiring the new thread selection state")
    XCTAssertEqual(decoded.model, "gpt-5.4")
    XCTAssertEqual(decoded.createdAt, 1_710_000_000)
    XCTAssertEqual(decoded.updatedAt, 1_710_000_100)
  }

  func testWorktreeSummaryDecodesBackendPayload() throws {
    let payload = """
      {
        "worktrees": [
          {
            "id": "wt-1",
            "workspace_id": "ws-repo",
            "label": "feature/thread-nav",
            "path": "/Users/fawx/fawx/.worktrees/thread-nav",
            "branch": "feature/thread-nav",
            "base_ref": "origin/dev",
            "status": "active",
            "clean": false,
            "ahead_count": 2,
            "behind_count": 1
          }
        ],
        "total": 1
      }
      """

    let decoded = try JSONDecoder().decode(WorktreesResponse.self, from: Data(payload.utf8))
    let worktree = try XCTUnwrap(decoded.worktrees.first)

    XCTAssertEqual(decoded.total, 1)
    XCTAssertEqual(worktree.id, "wt-1")
    XCTAssertEqual(worktree.workspaceID, "ws-repo")
    XCTAssertEqual(worktree.baseRef, "origin/dev")
    XCTAssertEqual(worktree.status, .active)
    XCTAssertFalse(worktree.clean)
    XCTAssertEqual(worktree.aheadCount, 2)
    XCTAssertEqual(worktree.behindCount, 1)
  }

  func testSessionCompatibilityAdapterUsesThreadActiveSessionID() {
    let thread = ThreadSummary(
      id: "thread-automation",
      title: "Nightly cleanup",
      kind: .automation,
      workspaceID: "ws-general",
      worktreeID: nil,
      activeSessionID: "session-automation",
      status: .paused,
      preview: "Waiting for the next run window",
      model: "gpt-5.4-mini",
      createdAt: 1_710_000_000,
      updatedAt: 1_710_000_120
    )

    let session = Session(threadSummary: thread)

    XCTAssertEqual(session.id, "session-automation")
    XCTAssertEqual(session.kind, .cron)
    XCTAssertEqual(session.status, .paused)
    XCTAssertEqual(session.title, "Nightly cleanup")
    XCTAssertEqual(session.preview, "Waiting for the next run window")
    XCTAssertEqual(session.model, "gpt-5.4-mini")
  }

  func testSidebarSelectionMigratesLegacySessionRawValue() {
    let selection = try? XCTUnwrap(SidebarSelection(rawValue: "session:session-123"))

    XCTAssertEqual(selection, .thread(.activeSessionID("session-123")))
    XCTAssertEqual(selection?.rawValue, "session:session-123")
    XCTAssertTrue(selection?.isChatSelection == true)
  }

  func testSidebarSelectionRoundTripsWorkspaceAndThreadIdentifiers() {
    let workspaceSelection = SidebarSelection.workspace("ws-repo")
    let threadSelection = SidebarSelection.thread(.threadID("thread-123"))

    XCTAssertEqual(SidebarSelection(rawValue: workspaceSelection.rawValue), workspaceSelection)
    XCTAssertEqual(SidebarSelection(rawValue: threadSelection.rawValue), threadSelection)
  }

  func testThreadReferenceConvenienceAccessorsExposeStoredIdentifiers() {
    let threadReference = ThreadReference.threadID("thread-123")
    let sessionReference = ThreadReference.activeSessionID("session-123")

    XCTAssertEqual(threadReference.threadID, "thread-123")
    XCTAssertNil(threadReference.sessionID)
    XCTAssertNil(sessionReference.threadID)
    XCTAssertEqual(sessionReference.sessionID, "session-123")
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
      startingAt: "@ViewBuilder\n  private func sessionRow(for session: Session) -> some View {",
      endingBefore: "\n\n  private var usesSplitLayout: Bool {"
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
      "matchesSettingsSearch(",
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

  func testMacOSContentViewHidesVisibleChatWithoutStoppingStreamsBeforeUtilityNavigation() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")
    let helperSource = try snippet(
      in: source,
      startingAt: "  private func hideActiveChatForUtilityNavigation() {",
      endingBefore: "\n\n  private var isChatSectionSelected: Bool {"
    )

    XCTAssertFalse(helperSource.contains("stopStreaming"))
    XCTAssertTrue(helperSource.contains("chatViewModel.cancelScheduledLoad()"))
    XCTAssertTrue(helperSource.contains("sessionViewModel.select(nil)"))
    XCTAssertTrue(helperSource.contains("chatViewModel.showEmptyState()"))
  }

  func testMacOSContentViewLetsChatSurfaceFlexBeforeCompressingSidePanes() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")
    let containerSource = try snippet(
      in: source,
      startingAt: "  @ViewBuilder\n  private var chatDetailContainer: some View {",
      endingBefore: "\n\n  private var chatShellContainer: some View {"
    )

    XCTAssertFalse(source.contains("WindowMinimumSizeConfigurator("))
    XCTAssertFalse(source.contains("chatDetailMinWidth"))
    XCTAssertTrue(containerSource.contains(".frame(maxWidth: .infinity, maxHeight: .infinity)"))
    XCTAssertTrue(containerSource.contains("minWidth: Layout.compactGitPanelMinWidth"))
  }

  func testMacOSSidebarUsesWorkspaceFirstThreadShellCopy() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/Sidebar.swift")

    XCTAssertTrue(source.contains("Text(\"Threads\")"))
    XCTAssertTrue(source.contains("title: \"By project\""))
    XCTAssertTrue(source.contains("title: \"Chronological list\""))
    XCTAssertTrue(source.contains("Text(\"Start a thread\")"))
    XCTAssertTrue(source.contains("Button(\"Archive Thread\")"))
    XCTAssertTrue(source.contains("Button(\"Rename Thread\")"))
    XCTAssertTrue(source.contains("Button(\"Remove\", role: .destructive)"))
  }

  func testMacOSSidebarKeepsGitInPrimaryFooterNavigation() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/Sidebar.swift")
    let footerSource = try snippet(
      in: source,
      startingAt: "  private var footer: some View {",
      endingBefore: "\n\n  private var visibleWorkspaceGroups: [WorkspaceThreadGroup] {"
    )

    XCTAssertTrue(footerSource.contains("title: \"Skills\""))
    XCTAssertTrue(footerSource.contains("title: \"Git\""))
    XCTAssertTrue(footerSource.contains("action: actions.showGit"))
    XCTAssertTrue(footerSource.contains("title: \"Fleet\""))
    XCTAssertTrue(footerSource.contains("title: \"Experiments\""))
    XCTAssertTrue(footerSource.contains("title: \"Settings\""))
  }

  func testMacOSFullGitViewKeepsExplicitRepositoryTargetSelection() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")
    let gitDetailSource = try snippet(
      in: source,
      startingAt: "    case .git:",
      endingBefore: "\n    case .settings:"
    )
    let showGitSource = try snippet(
      in: source,
      startingAt: "  private func showGit() {",
      endingBefore: "\n\n  private func showFleet() {"
    )
    let syncSource = try snippet(
      in: source,
      startingAt: "  private func syncThreadInspectorContext() {",
      endingBefore: "\n\n  private func bindGitRepositoryTarget"
    )

    XCTAssertTrue(gitDetailSource.contains("repositoryTargets: sessionViewModel.gitRepositoryTargets"))
    XCTAssertTrue(
      gitDetailSource.contains(
        "defaultRepositoryTarget: sessionViewModel.defaultGitRepositoryTarget"))
    XCTAssertTrue(gitDetailSource.contains("selectRepositoryTarget: bindGitRepositoryTarget"))
    XCTAssertTrue(showGitSource.contains("bindDefaultGitRepositoryTargetIfNeeded()"))
    XCTAssertFalse(showGitSource.contains("switchToNonChatSection(.git)"))
    XCTAssertTrue(syncSource.contains("guard sidebarSelection != .git else"))
  }

  func testSharedGitViewIncludesTargetPickerAndPinnedDiffWidth() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/GitView.swift")

    XCTAssertTrue(source.contains("let repositoryTargets: [GitRepositoryTarget]"))
    XCTAssertTrue(source.contains("private var targetPicker: some View"))
    XCTAssertTrue(source.contains("FawxDropdownMenu"))
    XCTAssertTrue(source.contains("Choose Target"))
    XCTAssertTrue(source.contains("Pick a workspace, worktree, or thread"))
    XCTAssertTrue(source.contains("GeometryReader { proxy in"))
    XCTAssertTrue(source.contains("minWidth: max(0, proxy.size.width"))
    XCTAssertTrue(source.contains("sectionPadding"))
    XCTAssertTrue(source.contains(".fawxSurface(.section)"))
    XCTAssertTrue(source.contains(".fawxSurface(.code)"))
    XCTAssertTrue(source.contains(".fawxRowChrome(isSelected: isSelected"))
    XCTAssertFalse(source.contains("cardPadding"))
    XCTAssertFalse(source.contains(".background(Color.fawxSurface)"))
    XCTAssertFalse(source.contains(".background(Color.fawxCode)"))
    XCTAssertFalse(source.contains(".stroke(Color.fawxBorder, lineWidth: 1)"))
  }

  func testFooterDestinationViewsUseSemanticSurfacePrimitives() throws {
    let skillsSource = try sourceFile(at: "app/Fawx/Views/Shared/SkillsView.swift")
    let marketplaceSource = try sourceFile(at: "app/Fawx/Views/Shared/MarketplaceView.swift")
    let fleetSource = try sourceFile(at: "app/Fawx/Views/Shared/FleetView.swift")
    let experimentsSource = try sourceFile(at: "app/Fawx/Views/Shared/ExperimentsView.swift")
    let footerSurfaceSections = [
      try snippet(
        in: skillsSource,
        startingAt: "    private var searchField: some View {",
        endingBefore: "\n\n    @ViewBuilder"
      ),
      try snippet(
        in: skillsSource,
        startingAt: "private struct SkillCardView: View {",
        endingBefore: "\n\nstruct LoadedSkillsCopy"
      ),
      try snippet(
        in: skillsSource,
        startingAt: "    private func settingRow(for field: SkillSettingsField) -> some View {",
        endingBefore: "\n\n    private func isExistingSecretConfigured"
      ),
      try snippet(
        in: marketplaceSource,
        startingAt: "private struct MarketplaceSkillCard: View {",
        endingBefore: "\n\nprivate struct MarketplaceBadge"
      ),
      try snippet(
        in: fleetSource,
        startingAt: "    private var summaryCard: some View {",
        endingBefore: "\n\n    @ViewBuilder"
      ),
      try snippet(
        in: fleetSource,
        startingAt: "private struct FleetNodeCard: View {",
        endingBefore: "\n\nprivate struct FleetNodeDetailSheet"
      ),
      try snippet(
        in: fleetSource,
        startingAt: "    private func detailCard(_ detail: FleetNodeDetailResponse) -> some View {",
        endingBefore: "\n\n    private var dispatchCard"
      ),
      try snippet(
        in: fleetSource,
        startingAt: "private struct FleetPlaceholderView: View {",
        endingBefore: "\n\nprivate extension FleetNodeDisplayStatus"
      ),
      try snippet(
        in: experimentsSource,
        startingAt: "private struct ExperimentSummaryCard: View {",
        endingBefore: "\n\nprivate struct ExperimentDetailSheet"
      ),
      try snippet(
        in: experimentsSource,
        startingAt: "    private func overviewCard(_ detail: ExperimentDetail) -> some View {",
        endingBefore: "\n\nprivate struct ExperimentLeaderRow"
      ),
      try snippet(
        in: experimentsSource,
        startingAt: "private struct ExperimentsPlaceholderView: View {",
        endingBefore: "\n\nprivate extension ExperimentStatus"
      ),
    ]

    for source in footerSurfaceSections {
      XCTAssertTrue(source.contains(".fawxSurface(.field)"))
      XCTAssertFalse(source.contains(".background(Color.fawxSurface)"))
      XCTAssertFalse(source.contains(".stroke(Color.fawxBorder, lineWidth: 1)"))
    }

    XCTAssertTrue(skillsSource.contains(".pickerStyle(.segmented)\n        .tint(.fawxAccent)"))
    XCTAssertTrue(skillsSource.contains("PermissionChip(label: humanizedCapability(capability), tone: .neutral)"))
    XCTAssertTrue(skillsSource.contains("PermissionChip(label: humanizedCapability(capability), tone: .warning)"))
  }

  func testSkillsSettingsSecretFieldsRemainMaskedInSource() throws {
    let skillsSource = try sourceFile(at: "app/Fawx/Views/Shared/SkillsView.swift")
    let secretSection = try snippet(
      in: skillsSource,
      startingAt: "            case .secret:",
      endingBefore: "\n            case .boolean:"
    )

    XCTAssertTrue(secretSection.contains("SecureField("))
    XCTAssertFalse(secretSection.contains("TextField("))
  }

  func testMacOSTransientPresentationsUseSharedSurfaceContract() throws {
    let visualStyleSource = try sourceFile(at: "app/Fawx/Theme/VisualStyle.swift")
    let inputBarSource = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let experimentsSource = try sourceFile(at: "app/Fawx/Views/Shared/ExperimentsView.swift")
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let sessionMemorySource = try sourceFile(at: "app/Fawx/Views/Shared/SessionMemoryPanel.swift")
    let ripcordNotificationSource = try sourceFile(at: "app/Fawx/Views/Ripcord/RipcordNotification.swift")
    let ripcordBannerSource = try sourceFile(at: "app/Fawx/Views/Ripcord/RipcordBanner.swift")
    let ripcordReportSource = try sourceFile(at: "app/Fawx/Views/Ripcord/RipcordReportView.swift")
    let ripcordJournalSource = try sourceFile(at: "app/Fawx/Views/Ripcord/RipcordJournalPanel.swift")

    XCTAssertTrue(visualStyleSource.contains("case transient"))
    XCTAssertTrue(visualStyleSource.contains("func fawxTransientSurface("))

    let inputBarMacPresentation = try snippet(
      in: inputBarSource,
      startingAt: "#if os(macOS)\n            if isPresentingModelSelector",
      endingBefore: "#endif\n\n            VStack(alignment: .leading"
    )
    let inlineModelSelector = try snippet(
      in: inputBarSource,
      startingAt: "    private var inlineModelSelector: some View {",
      endingBefore: "#endif\n\n    private var effectivePlaceholder"
    )

    XCTAssertTrue(inputBarMacPresentation.contains("inlineModelSelector"))
    XCTAssertFalse(inputBarMacPresentation.contains(".sheet("))
    XCTAssertTrue(inlineModelSelector.contains("modelSelectorList("))
    XCTAssertTrue(inlineModelSelector.contains(".fawxTransientSurface(shadowStyle: nil)"))

    let experimentsMacPresentation = try snippet(
      in: experimentsSource,
      startingAt: "#if os(macOS)\n        experimentsContent",
      endingBefore: "#else\n        experimentsContent"
    )
    let experimentFloatingPanel = try snippet(
      in: experimentsSource,
      startingAt: "private struct ExperimentDetailFloatingPanel: View {",
      endingBefore: "#endif\n\nprivate struct ExperimentDetailContent"
    )
    let sessionMemoryMacPanel = try snippet(
      in: sessionMemorySource,
      startingAt: "#if os(macOS)\n        VStack(spacing: 0)",
      endingBefore: "#else\n        NavigationStack"
    )
    let sessionMemoryMacHeader = try snippet(
      in: sessionMemorySource,
      startingAt: "    private var macHeader: some View {",
      endingBefore: "\n\n    private var macDragHandle"
    )
    let sessionMemoryMacDragHandle = try snippet(
      in: sessionMemorySource,
      startingAt: "    private var macDragHandle: some View {",
      endingBefore: "\n\n    private var macCloseButton"
    )
    let sessionMemoryMacCloseButton = try snippet(
      in: sessionMemorySource,
      startingAt: "    private var macCloseButton: some View {",
      endingBefore: "\n\n    private var panelMoveGesture"
    )
    let sessionMemoryMacPresentation = try snippet(
      in: sessionMemorySource,
      startingAt: "#if os(macOS)\n        content",
      endingBefore: "#else\n        content"
    )

    XCTAssertTrue(experimentsMacPresentation.contains("selectedExperimentPanel"))
    XCTAssertTrue(experimentsMacPresentation.contains("GeometryReader"))
    XCTAssertTrue(experimentsMacPresentation.contains("proxy.size.height"))
    XCTAssertTrue(experimentsSource.contains("experimentPanelMaxHeight(availableHeight:"))
    XCTAssertFalse(experimentsSource.contains(".frame(maxHeight: 640)"))
    XCTAssertFalse(experimentsMacPresentation.contains(".sheet("))
    XCTAssertTrue(experimentFloatingPanel.contains(".fawxTransientSurface()"))
    XCTAssertTrue(sessionMemoryMacPanel.contains(".fawxTransientSurface()"))
    XCTAssertTrue(sessionMemoryMacPanel.contains(".offset(x: panelOffset.width, y: panelOffset.height)"))
    XCTAssertTrue(sessionMemorySource.contains("@State private var panelOffset = CGSize.zero"))
    XCTAssertTrue(sessionMemorySource.contains("@State private var panelDragOrigin: CGSize?"))
    XCTAssertFalse(sessionMemorySource.contains("@GestureState private var activePanelDragOffset"))
    XCTAssertTrue(sessionMemoryMacHeader.contains("macDragHandle"))
    XCTAssertFalse(sessionMemoryMacHeader.contains(".gesture(panelMoveGesture)"))
    XCTAssertTrue(sessionMemoryMacDragHandle.contains(".gesture(panelMoveGesture)"))
    XCTAssertTrue(sessionMemoryMacCloseButton.contains(".frame(width: 32, height: 32)"))
    XCTAssertTrue(sessionMemoryMacCloseButton.contains(".keyboardShortcut(.cancelAction)"))
    let sessionMemorySurfaceRange = try XCTUnwrap(
      sessionMemoryMacPanel.range(of: ".fawxTransientSurface()"))
    let sessionMemoryOffsetRange = try XCTUnwrap(
      sessionMemoryMacPanel.range(of: ".offset(x: panelOffset.width, y: panelOffset.height)"))
    XCTAssertLessThan(sessionMemorySurfaceRange.lowerBound, sessionMemoryOffsetRange.lowerBound)
    XCTAssertTrue(sessionMemoryMacPresentation.contains(".overlay"))
    XCTAssertFalse(sessionMemoryMacPresentation.contains(".sheet("))

    let transientSurfaceSections = [
      try snippet(
        in: chatSource,
        startingAt: "  private var loadingOverlay: some View {",
        endingBefore: "\n\n  private var cachedRefreshIndicator"
      ),
      try snippet(
        in: chatSource,
        startingAt: "  private var emptyState: some View {",
        endingBefore: "\n\n  private var composerArea"
      ),
      try snippet(
        in: ripcordNotificationSource,
        startingAt: "struct FawxSurfaceCard<Content: View>: View {",
        endingBefore: "\n\nprivate struct RipcordResolutionButton"
      ),
      try snippet(
        in: ripcordBannerSource,
        startingAt: "struct RipcordBanner: View {",
        endingBefore: "\n\n    private var actionButtons"
      ),
    ]

    for source in transientSurfaceSections {
      XCTAssertTrue(source.contains(".fawxTransientSurface"))
      XCTAssertFalse(source.contains(".background(Color.fawxSurface"))
      XCTAssertFalse(source.contains(".stroke(Color.fawxBorder, lineWidth: 1)"))
    }

    let modalFieldSections = [
      try snippet(
        in: chatSource,
        startingAt: "struct PermissionPromptSheetView: View {",
        endingBefore: "\n\n  private var promptAccentColor"
      ),
      try snippet(
        in: sessionMemorySource,
        startingAt: "    private var summaryCard: some View {",
        endingBefore: "\n\n    private var overviewCard"
      ),
      try snippet(
        in: sessionMemorySource,
        startingAt: "    private func memoryCard<Content: View>(",
        endingBefore: "\n\n    private func memoryField"
      ),
      try snippet(
        in: sessionMemorySource,
        startingAt: "    private func memoryField(",
        endingBefore: "\n\n    @ViewBuilder"
      ),
      try snippet(
        in: ripcordReportSource,
        startingAt: "    private var summaryCard: some View {",
        endingBefore: "\n\n    private func summaryRow"
      ),
      try snippet(
        in: ripcordReportSource,
        startingAt: "private struct RipcordReportRow: View {",
        endingBefore: "\n}\n"
      ),
      try snippet(
        in: ripcordJournalSource,
        startingAt: "    private var overviewCard: some View {",
        endingBefore: "\n\n    private var footer"
      ),
      try snippet(
        in: ripcordJournalSource,
        startingAt: "private struct RipcordJournalEntryCard: View {",
        endingBefore: "\n}\n\nprivate func makeRipcordJournalDateFormatter"
      ),
    ]

    for source in modalFieldSections {
      XCTAssertTrue(source.contains(".fawxSurface(.field)"))
      XCTAssertFalse(source.contains(".background(Color.fawxSurface)"))
      XCTAssertFalse(source.contains(".stroke(Color.fawxBorder, lineWidth: 1)"))
    }
  }

  func testSettingsViewIncludesArchivedThreadRecoverySurface() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/SettingsView.swift")

    XCTAssertTrue(source.contains("settingsSection(.threads)"))
    XCTAssertTrue(source.contains("Text(\"Archived threads\")"))
    XCTAssertTrue(source.contains("primaryActionTitle: \"Restore\""))
    XCTAssertTrue(source.contains("Button(\"Refresh\")"))
  }

  func testModelSelectionListIncludesFavoriteScopeAndDedicatedFavoriteAction() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/ModelSelectionList.swift")

    XCTAssertTrue(source.contains("let favoriteModelIDs: Set<String>"))
    XCTAssertTrue(source.contains("let toggleFavorite: (String) -> Void"))
    XCTAssertTrue(source.contains("ModelSelectionScope.allCases"))
    XCTAssertTrue(source.contains("modelCatalogScope_\\(scope.rawValue)"))
    XCTAssertTrue(source.contains("modelFavoriteButton_\\(model.modelID)"))
    XCTAssertTrue(source.contains("Star models from Recommended or All Models"))
  }

  func testMacOSCommandsUseThreadTerminology() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/FawxMacCommands.swift")

    XCTAssertTrue(source.contains("Button(\"New Thread\")"))
    XCTAssertTrue(source.contains("CommandMenu(\"Thread\")"))
    XCTAssertTrue(source.contains("Button(\"Threads\")"))
    XCTAssertFalse(source.contains("Button(\"New Session\")"))
    XCTAssertFalse(source.contains("CommandMenu(\"Session\")"))
  }

  func testStatusBarIncludesSessionMemoryButton() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/StatusBar.swift")

    XCTAssertTrue(source.contains("accessibilityIdentifier(\"sessionMemoryButton\")"))
    XCTAssertTrue(source.contains("accessibilityLabel(\"Open session memory\")"))
    XCTAssertTrue(source.contains("Text(\"Memory\")"))
  }

  func testChatDetailViewPresentsSessionMemoryPanel() throws {
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let sessionMemorySource = try sourceFile(at: "app/Fawx/Views/Shared/SessionMemoryPanel.swift")

    XCTAssertTrue(
      chatSource.contains(
        ".sessionMemoryPresentation(appState: appState, presentedSession: $presentedSessionMemory)"
      ))
    XCTAssertTrue(sessionMemorySource.contains("private struct SessionMemoryPresentationModifier"))
    XCTAssertTrue(sessionMemorySource.contains("SessionMemoryPanel(appState: appState, session: session)"))
  }

  func testChatDetailViewKeepsBackgroundNoticeOutOfComposerPrompt() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")

    XCTAssertFalse(source.contains("currentPhase: composerPhaseLabel"))
    XCTAssertFalse(source.contains("private var composerPhaseLabel: String?"))
    XCTAssertFalse(source.contains("sessionViewModel.selectedBackgroundActivityNotice?.message"))
  }

  func testSessionMemoryPanelValidatesAndCountsActiveFiles() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/SessionMemoryPanel.swift")

    XCTAssertTrue(
      source.contains(
        "\\(sanitizedDraft.activeFiles.count) / \\(SessionMemory.maxItems) active files"))
    XCTAssertTrue(
      source.contains("Keep active files to \\(SessionMemory.maxItems) items or fewer."))
    XCTAssertTrue(source.contains(".disabled(isDisabled || isAtItemLimit)"))
  }

  func testChatDetailViewStylesEmergencyCompactionBanner() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")

    XCTAssertTrue(source.contains("CompactionBannerStyle(isEmergency: info.isEmergency)"))
    XCTAssertTrue(source.contains("init(isEmergency: Bool)"))
    XCTAssertFalse(
      source.contains("message.localizedCaseInsensitiveContains(\"urgently optimized\")"))
    XCTAssertTrue(source.contains("Color.fawxWarning.opacity(0.12)"))
    XCTAssertTrue(source.contains("Color.fawxWarning.opacity(0.45)"))
  }

  func testMacOSContentViewBindsInspectorToSelectedThreadContext() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")

    XCTAssertTrue(source.contains("threadContext: selectedThreadContext"))
    XCTAssertTrue(source.contains("threadActivity: selectedThreadActivity"))
    XCTAssertTrue(source.contains("backgroundActivityNotice: selectedBackgroundActivityNotice"))
    XCTAssertTrue(source.contains("openSessionMemoryAction: presentSessionMemoryPanel"))
    XCTAssertTrue(source.contains("sessionViewModel.selectedThreadContextSnapshot"))
    XCTAssertTrue(source.contains("sessionViewModel.selectedThreadActivitySnapshot"))
    XCTAssertTrue(source.contains("sessionViewModel.selectedBackgroundActivityNotice"))
    XCTAssertTrue(source.contains("gitViewModel.bindThreadContext(selectedThreadContext)"))
    XCTAssertTrue(source.contains("ThreadContextHeader("))
    XCTAssertTrue(source.contains("ThreadContextPill"))
    XCTAssertFalse(source.contains("threadContextModelBadge"))
    XCTAssertFalse(source.contains("threadContextModelProviderBadge"))
    XCTAssertFalse(source.contains("private var selectedThreadModelInfo: ModelInfo?"))
    XCTAssertFalse(source.contains("modelInfo: selectedThreadModelInfo"))
    XCTAssertFalse(source.contains("title: \"Model\""))
    XCTAssertFalse(source.contains("title: \"Status\""))
    XCTAssertFalse(source.contains("title: \"Background\""))
    XCTAssertFalse(source.contains("context.threadStatus.rawValue.capitalized"))
  }

  func testChatComposerAndModelListExposeModelRouteTrust() throws {
    let inputBarSource = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let modelSelectionSource = try sourceFile(at: "app/Fawx/Views/Shared/ModelSelectionList.swift")
    let modelTrustBadgeSource = try sourceFile(at: "app/Fawx/Views/Shared/ModelDataTrustBadge.swift")

    XCTAssertTrue(inputBarSource.contains("activeModelProviderBadge"))
    XCTAssertTrue(inputBarSource.contains("composerModelProviderBadge"))
    XCTAssertTrue(inputBarSource.contains("activeModel.dataTrust.detail"))
    XCTAssertFalse(inputBarSource.contains("activeModelTrustBadge"))
    XCTAssertFalse(inputBarSource.contains("composerModelTrustBadge"))
    XCTAssertTrue(modelSelectionSource.contains("ModelDataTrustBadge(trust: model.dataTrust)"))
    XCTAssertTrue(modelTrustBadgeSource.contains("struct ModelDataTrustBadge"))
    XCTAssertTrue(modelTrustBadgeSource.contains("struct ModelProviderBadge"))
    XCTAssertTrue(modelTrustBadgeSource.contains("case .knownRouter:"))
    XCTAssertTrue(modelTrustBadgeSource.contains("return .fawxError"))
  }

  func testChatComposerUsesThreadScopedModelSelection() throws {
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let chatViewModelSource = try sourceFile(at: "app/Fawx/ViewModels/ChatViewModel.swift")
    let sessionViewModelSource = try sourceFile(at: "app/Fawx/ViewModels/SessionViewModel.swift")

    XCTAssertTrue(chatSource.contains("activeModel: chatViewModel.selectedThreadModel"))
    XCTAssertTrue(chatSource.contains("await chatViewModel.selectModelForCurrentThread(modelID)"))
    XCTAssertFalse(chatSource.contains("activeModel: appState.activeModel"))
    XCTAssertFalse(chatSource.contains("try? await appState.setModel(modelID)"))
    XCTAssertTrue(chatViewModelSource.contains("let requestedModelID = selectedThreadModelID"))
    XCTAssertTrue(chatViewModelSource.contains("modelID: requestedModelID"))
    XCTAssertTrue(sessionViewModelSource.contains("func updateModel(for sessionID: String, modelID: String) async -> Bool"))
  }

  func testChatComposerExposesQueuedTurnSteering() throws {
    let inputBarSource = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let queuedChipSource = try sourceFile(at: "app/Fawx/Views/Shared/QueuedMessageChip.swift")
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let chatViewModelSource = try sourceFile(at: "app/Fawx/ViewModels/ChatViewModel.swift")
    let clientSource = try sourceFile(at: "app/Fawx/Networking/FawxClient.swift")

    XCTAssertFalse(inputBarSource.contains("turnSteeringToggle"))
    XCTAssertFalse(inputBarSource.contains("Turn Steering"))
    XCTAssertTrue(inputBarSource.contains("QueuedMessageChip("))
    XCTAssertTrue(queuedChipSource.contains(".accessibilityIdentifier(\"queuedMessageSteerToggle\")"))
    XCTAssertTrue(chatSource.contains("toggleQueuedMessageSteering: chatViewModel.toggleQueuedMessageSteering"))
    XCTAssertTrue(chatViewModelSource.contains("var draftSteering: String"))
    XCTAssertTrue(chatViewModelSource.contains("func toggleQueuedMessageSteering()"))
    XCTAssertTrue(clientSource.contains("func steerSession(id: String, text: String)"))
  }

  func testStopStreamingUsesBackendSessionStopContract() throws {
    let clientSource = try sourceFile(at: "app/Fawx/Networking/FawxClient.swift")
    let chatViewModelSource = try sourceFile(at: "app/Fawx/ViewModels/ChatViewModel.swift")

    XCTAssertTrue(clientSource.contains("func stopSession(id: String) async throws -> StopSessionResponse"))
    XCTAssertTrue(clientSource.contains("Self.sessionPath(id: id, suffix: \"stop\")"))
    XCTAssertTrue(chatViewModelSource.contains("requestServerStop(for: sessionID)"))
    XCTAssertTrue(chatViewModelSource.contains("try await client.stopSession(id: sessionID)"))
    XCTAssertTrue(chatViewModelSource.contains("showServerStopFailure"))
    XCTAssertTrue(chatViewModelSource.contains("appState.showToast"))
  }

  func testCompactGitPanelPresentsThreadBoundContextAndGitSections() throws {
    let source = try sourceFile(at: "app/Fawx/Views/macOS/CompactGitPanel.swift")

    XCTAssertTrue(source.contains("case context"))
    XCTAssertTrue(source.contains("case git"))
    XCTAssertTrue(source.contains("ThreadContextSummaryCard"))
    XCTAssertTrue(source.contains("ThreadActivityCard"))
    XCTAssertTrue(source.contains("ThreadSupportCard"))
    XCTAssertTrue(source.contains("GitSummaryCard"))
    XCTAssertTrue(source.contains("Button(\"Open Full Git View\", action: openFullViewAction)"))
    XCTAssertTrue(
      source.contains("Select a thread to inspect its workspace, Git, and memory context."))
  }

  func testSidebarAndMobileRowsUseActivitySnapshotsWithoutRawToolCountBadges() throws {
    let sidebarSource = try sourceFile(at: "app/Fawx/Views/macOS/Sidebar.swift")
    let mobileSource = try sourceFile(at: "app/Fawx/Views/iOS/SessionListView.swift")
    let rowSource = try sourceFile(at: "app/Fawx/Views/Shared/SessionRowView.swift")

    XCTAssertTrue(sidebarSource.contains("sessionViewModel.threadActivitySnapshot(for: thread)"))
    XCTAssertTrue(sidebarSource.contains("let activity: ThreadActivitySnapshot"))
    XCTAssertTrue(sidebarSource.contains("activity.showsUnreadIndicator"))
    XCTAssertFalse(sidebarSource.contains("activity.compactBadgeLabel"))
    XCTAssertFalse(sidebarSource.contains("ThreadActivityBadge"))

    XCTAssertTrue(mobileSource.contains("sessionViewModel.activitySnapshot(for: session)"))
    XCTAssertTrue(mobileSource.contains("activityLabel: nil"))
    XCTAssertFalse(mobileSource.contains("activity?.compactBadgeLabel"))
    XCTAssertTrue(mobileSource.contains("backgroundActivitySection"))
    XCTAssertTrue(mobileSource.contains("sessionViewModel.backgroundActivityOverviewNotice"))
    XCTAssertTrue(mobileSource.contains("SessionRowView("))

    XCTAssertTrue(rowSource.contains("let activityLabel: String?"))
    XCTAssertTrue(rowSource.contains("let showsUnreadDot: Bool"))
  }

  func testChatMessagesAreSelectableWithoutPaintedBubbleContainers() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/MessageBubble.swift")
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let activitySource = try sourceFile(at: "app/Fawx/Views/Shared/ToolCallCard.swift")
    let codeBlockSource = try sourceFile(at: "app/Fawx/Views/Shared/CodeBlock.swift")

    XCTAssertTrue(source.contains(".textSelection(.enabled)"))
    XCTAssertTrue(source.contains("SelectableTranscriptText"))
    XCTAssertTrue(source.contains("TranscriptMarkdownText"))
    XCTAssertTrue(source.contains("struct TranscriptMarkdownContentView"))
    XCTAssertTrue(source.contains("Markdown(text)"))
    XCTAssertTrue(source.contains("init(transcriptMessage: TranscriptMessage, isFinalAnswer: Bool = false)"))
    XCTAssertTrue(source.contains("self.content = transcriptMessage.displayText"))
    XCTAssertTrue(source.contains("self.contentBlocks = [.text(transcriptMessage.displayText)]"))
    XCTAssertTrue(source.contains("NSPasteboard.general.setString(content, forType: .string)"))
    XCTAssertTrue(source.contains("Image(systemName: didCopy ? \"checkmark\" : \"doc.on.doc\")"))
    XCTAssertTrue(source.contains("Color.fawxSurfaceHover.opacity(FawxOpacity.surfaceMuted)"))
    XCTAssertTrue(source.contains("IntrinsicSelectableTextView: NSTextView"))
    XCTAssertTrue(source.contains(".inlineOnlyPreservingWhitespace"))
    XCTAssertTrue(source.contains("TranscriptMarkdownRenderer.attributedString"))
    XCTAssertTrue(source.contains("if textView.string != attributedString.string"))
    XCTAssertTrue(source.contains("textView.restoreValidSelectedRanges(selectedRanges)"))
    XCTAssertTrue(source.contains("textView.applyFawxTextSelectionChrome()"))
    XCTAssertTrue(activitySource.contains("TranscriptMarkdownContentView(text: text, alignment: .left)"))
    XCTAssertTrue(codeBlockSource.contains("SelectableCodeBlockText(content: content)"))
    XCTAssertTrue(codeBlockSource.contains("textContainer?.widthTracksTextView = false"))
    XCTAssertTrue(codeBlockSource.contains(".fixedSize(horizontal: true, vertical: true)"))
    XCTAssertFalse(codeBlockSource.contains("SelectableTranscriptText"))
    XCTAssertTrue(chatSource.contains("ForEach(chatViewModel.transcriptTurns)"))
    XCTAssertTrue(chatSource.contains("MessageBubble(transcriptMessage: message)"))
    XCTAssertFalse(source.contains("alignment: .right"))
    XCTAssertFalse(source.contains("frame(maxWidth: .infinity, alignment: .trailing)"))
    XCTAssertFalse(source.contains(".background(bubbleBackground)"))
    XCTAssertFalse(source.contains(".overlay(bubbleBorder)"))
    XCTAssertFalse(source.contains("Color.fawxUserBubbleText"))
  }

  func testChatTranscriptUsesCodexStyleActivityTimeline() throws {
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let activitySource = try sourceFile(at: "app/Fawx/Views/Shared/ToolCallCard.swift")

    XCTAssertTrue(chatSource.contains("AssistantTranscriptTurnView(turn: turn)"))
    XCTAssertTrue(chatSource.contains("ForEach(turn.chunks)"))
    XCTAssertTrue(chatSource.contains("WorkingNarrationBubble(narration: narration)"))
    XCTAssertTrue(chatSource.contains("AssistantActivityTimeline("))
    XCTAssertTrue(chatSource.contains("group: group"))
    XCTAssertTrue(chatSource.contains("case .finalAnswer(let message):"))
    XCTAssertTrue(chatSource.contains("MessageBubble(transcriptMessage: message, isFinalAnswer: true)"))
    XCTAssertTrue(chatSource.contains("ReasoningActivityView("))
    XCTAssertTrue(chatSource.contains("&& !isShowingCurrentTurnFinalResponse"))
    XCTAssertTrue(chatSource.contains("&& hasVisibleLiveActivity"))
    XCTAssertTrue(chatSource.contains("private var isShowingCurrentTurnFinalResponse"))
    XCTAssertTrue(chatSource.contains("chatViewModel.isCurrentSessionStreamingFinalResponse"))
    XCTAssertTrue(chatSource.contains("chatViewModel.transcriptTurns.hasCurrentTurnTerminalAssistantOutput"))
    XCTAssertFalse(chatSource.contains("hasVisibleCurrentTurnTerminalAssistantOutput"))
    XCTAssertFalse(chatSource.contains("streamingFooterStatusText"))
    XCTAssertTrue(chatSource.contains(".font(FawxTypography.chatBody.weight(.medium))"))
    XCTAssertFalse(chatSource.contains("ToolActivityGroupCard(group: group)"))
    XCTAssertFalse(chatSource.contains("StreamingStatusHeader("))
    XCTAssertFalse(chatSource.contains("StreamingPulseDots"))
    XCTAssertTrue(activitySource.contains("enum AssistantActivityState"))
    XCTAssertTrue(activitySource.contains("ActivityKindSummary"))
    XCTAssertFalse(activitySource.contains("typealias ToolActivityGroupCardSnapshot"))
    XCTAssertFalse(activitySource.contains("struct ToolActivityGroupCard"))
    XCTAssertTrue(activitySource.contains("FawxAnimation.expand"))
    XCTAssertTrue(activitySource.contains("@State private var isExpanded: Bool"))
    XCTAssertTrue(activitySource.contains("_isExpanded = State("))
    XCTAssertTrue(activitySource.contains("defaultExpanded"))
    XCTAssertTrue(activitySource.contains("hasCollapsedOnCompletion"))
    XCTAssertTrue(activitySource.contains("visibleToolCalls"))
    XCTAssertTrue(activitySource.contains("var actionPrefix: String"))
    XCTAssertTrue(activitySource.contains("case queued"))
    XCTAssertTrue(activitySource.contains("case running"))
    XCTAssertTrue(activitySource.contains("case completed"))
    XCTAssertTrue(activitySource.contains("case failed"))
    XCTAssertTrue(activitySource.contains("case cancelled"))
    XCTAssertTrue(activitySource.contains("case deferred"))
    XCTAssertTrue(activitySource.contains("struct AssistantActivityTimeline"))
    XCTAssertTrue(activitySource.contains("struct AssistantActivityEventSnapshot"))
  }

#if os(macOS)
  func testMacComposerHeightMeasurementAvoidsLayoutManagerReentry() throws {
    let source = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let composerSource = try snippet(
      in: source,
      startingAt: "private final class ComposerNSTextView: NSTextView {",
      endingBefore: "\n    private func pastedImageData()"
    )

    XCTAssertTrue(composerSource.contains("scheduleMeasuredHeightRefresh()"))
    XCTAssertFalse(composerSource.contains("ensureLayout(for:"))
    XCTAssertFalse(composerSource.contains("usedRect(for:"))
  }

  func testMacComposerHeightMeasurerHandlesWrappingAndTrailingBlankLines() {
    let font = NSFont.systemFont(ofSize: 14)
    let inset = NSSize(width: 0, height: 4)
    let longPrompt = "Summarize the review findings and include the branch, commit, and test plan."

    let wideHeight = MacComposerHeightMeasurer.measuredHeight(
      for: longPrompt,
      availableWidth: 600,
      font: font,
      textContainerInset: inset,
      lineFragmentPadding: 0
    )
    let narrowHeight = MacComposerHeightMeasurer.measuredHeight(
      for: longPrompt,
      availableWidth: 60,
      font: font,
      textContainerInset: inset,
      lineFragmentPadding: 0
    )
    let oneLineHeight = MacComposerHeightMeasurer.measuredHeight(
      for: "hello",
      availableWidth: 600,
      font: font,
      textContainerInset: inset,
      lineFragmentPadding: 0
    )
    let trailingBlankLineHeight = MacComposerHeightMeasurer.measuredHeight(
      for: "hello\n",
      availableWidth: 600,
      font: font,
      textContainerInset: inset,
      lineFragmentPadding: 0
    )

    XCTAssertGreaterThan(narrowHeight, wideHeight)
    XCTAssertGreaterThan(trailingBlankLineHeight, oneLineHeight)
  }

  func testMacComposerUsesNeutralTextCursorAndSharedPlaceholderGeometry() throws {
    let inputBarSource = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")

    XCTAssertFalse(chatSource.contains("accentColor: appState.accentColor.color"))
    XCTAssertFalse(inputBarSource.contains("let accentColor: Color"))
    XCTAssertTrue(inputBarSource.contains("@Environment(\\.fawxAccentInvalidationToken)"))
    XCTAssertTrue(inputBarSource.contains("insertionPointColor: macComposerInsertionPointColor"))
    XCTAssertTrue(inputBarSource.contains("return .fawxTextInsertionPoint"))
    XCTAssertTrue(inputBarSource.contains("textView.insertionPointColor = insertionPointColor"))
    XCTAssertTrue(inputBarSource.contains("textView.applyFawxTextSelectionChrome()"))
    XCTAssertTrue(inputBarSource.contains("textView.isAutomaticSpellingCorrectionEnabled = false"))
    XCTAssertTrue(inputBarSource.contains("textView.isContinuousSpellCheckingEnabled = false"))
    XCTAssertTrue(inputBarSource.contains("textView.isGrammarCheckingEnabled = false"))
    XCTAssertTrue(inputBarSource.contains("private let macComposerTextContainerInset"))
    XCTAssertTrue(inputBarSource.contains("private let macComposerLineFragmentPadding"))
    XCTAssertTrue(inputBarSource.contains(".padding(.leading, macComposerLineFragmentPadding)"))
    XCTAssertTrue(inputBarSource.contains(".padding(.top, macComposerTextContainerInset.height)"))
    XCTAssertTrue(inputBarSource.contains("textView.textContainerInset = macComposerTextContainerInset"))
    XCTAssertTrue(inputBarSource.contains("textView.textContainer?.lineFragmentPadding = macComposerLineFragmentPadding"))
  }
#endif

  func testMacAppearanceForegroundActivationDoesNotRefreshWhileStreaming() throws {
    let appSource = try sourceFile(at: "app/Fawx/FawxApp.swift")
    let foregroundRefreshSource = try snippet(
      in: appSource,
      startingAt: "    private func refreshForForegroundActivation() async {",
      endingBefore: "\n\n#if os(macOS)"
    )
    let streamingGuardSource = try snippet(
      in: String(foregroundRefreshSource),
      startingAt: "            guard !chatViewModel.isStreaming else {",
      endingBefore: "\n            }\n\n            _ = try await appState.client.health()"
    )

    XCTAssertTrue(streamingGuardSource.contains("return"))
    XCTAssertFalse(streamingGuardSource.contains("sessionViewModel.refresh()"))
    XCTAssertFalse(streamingGuardSource.contains("chatViewModel.loadMessages"))
    XCTAssertTrue(foregroundRefreshSource.contains("try await appState.refreshServerState()"))
    XCTAssertTrue(foregroundRefreshSource.contains("await chatViewModel.loadMessages"))
  }

  func testMacOSShellUsesSemanticSurfacePrimitives() throws {
    let colorSource = try sourceFile(at: "app/Fawx/Theme/Colors.swift")
    let visualStyleSource = try sourceFile(at: "app/Fawx/Theme/VisualStyle.swift")
    let appSource = try sourceFile(at: "app/Fawx/FawxApp.swift")
    let contentSource = try sourceFile(at: "app/Fawx/Views/macOS/ContentView.swift")
    let sidebarSource = try sourceFile(at: "app/Fawx/Views/macOS/Sidebar.swift")
    let settingsSource = try sourceFile(at: "app/Fawx/Views/macOS/SettingsView.swift")
    let chatSource = try sourceFile(at: "app/Fawx/Views/Shared/ChatDetailView.swift")
    let inputBarSource = try sourceFile(at: "app/Fawx/Views/Shared/InputBar.swift")
    let ripcordNotificationSource = try sourceFile(at: "app/Fawx/Views/Ripcord/RipcordNotification.swift")
    let macThemedRootSource = try snippet(
      in: appSource,
      startingAt: "#if os(macOS)\n        let fawxRoot = rootViewWithPermissionSheet",
      endingBefore: "\n#else"
    )
    let macOSMainContentSource = try snippet(
      in: contentSource,
      startingAt: "  private var macOSMainContent: some View {",
      endingBefore: "\n  private var sidebarView: some View {"
    )
    let settingsCategorySidebarSource = try snippet(
      in: settingsSource,
      startingAt: "    private var settingsCategorySidebar: some View {",
      endingBefore: "\n\n    private var settingsDetailPane: some View {"
    )
    let settingsCategoryRowSource = try snippet(
      in: settingsSource,
      startingAt: "private struct SettingsCategoryRow: View {",
      endingBefore: "\n\nprivate struct ThreadManagementAction: Identifiable {"
    )

    XCTAssertTrue(visualStyleSource.contains("enum FawxSurfaceRole"))
    XCTAssertTrue(visualStyleSource.contains("case rail"))
    XCTAssertTrue(visualStyleSource.contains("case composer"))
    XCTAssertTrue(colorSource.contains("static var fawxSurface: Color { fawxBackground }"))
    XCTAssertTrue(colorSource.contains("static var fawxAccent: Color { FawxAccentPalette.color }"))
    XCTAssertTrue(colorSource.contains("static var fawxAccentText: Color { FawxAccentPalette.textColor }"))
    XCTAssertTrue(colorSource.contains("func resolvedForFawxChrome(in scheme: FawxAccentContrastScheme)"))
    XCTAssertTrue(colorSource.contains("static var fawxTextInsertionPoint: NSColor"))
    XCTAssertFalse(colorSource.contains("controlAccentColor"))
    XCTAssertTrue(colorSource.contains("static var fawxTextSelectionBackground: NSColor"))
    XCTAssertTrue(colorSource.contains("func applyFawxTextSelectionChrome()"))
    XCTAssertTrue(appSource.contains("FawxAccentPalette.update(appState.accentColor)"))
    XCTAssertFalse(macThemedRootSource.contains(".tint(Color.fawxAccent)"))
    XCTAssertFalse(macThemedRootSource.contains(".tint(appState.accentColor.color)"))
    XCTAssertFalse(macThemedRootSource.contains(".accentColor(appState.accentColor.color)"))
    XCTAssertFalse(appSource.contains("let themedView = rootViewWithPermissionSheet"))
    XCTAssertTrue(appSource.contains(".containerBackground(Color.fawxBackground, for: .window)"))
    XCTAssertTrue(contentSource.contains("@AppStorage(\"show_threads_sidebar\")"))
    XCTAssertTrue(contentSource.contains("private var detailShell: some View"))
    XCTAssertTrue(contentSource.contains("shellDropdownCluster"))
    XCTAssertTrue(contentSource.contains("private struct ShellPanelMenuButton"))
    XCTAssertTrue(contentSource.contains("private struct BranchContextMenuButton"))
    XCTAssertTrue(contentSource.contains("private struct ShellDropdownLabel"))
    XCTAssertTrue(contentSource.contains("FawxDropdownMenu"))
    XCTAssertTrue(contentSource.contains("branchMenuTitle"))
    XCTAssertTrue(contentSource.contains("branchContextMenuButton"))
    XCTAssertTrue(contentSource.contains("shellPanelMenuButton"))
    XCTAssertFalse(contentSource.contains("private func shellPanelRail"))
    XCTAssertFalse(contentSource.contains("private struct ShellPanelToggleButton"))
    XCTAssertTrue(macOSMainContentSource.contains("HSplitView"))
    XCTAssertTrue(macOSMainContentSource.contains("showThreadsSidebar"))
    XCTAssertFalse(macOSMainContentSource.contains("NavigationSplitView"))
    XCTAssertFalse(macOSMainContentSource.contains("ToolbarItem"))
    XCTAssertFalse(macOSMainContentSource.contains(".toolbar"))
    XCTAssertTrue(
      visualStyleSource.contains("case .page, .rail, .section, .composer, .field, .transient, .callout:")
    )
    XCTAssertTrue(visualStyleSource.contains("struct FawxDropdownMenu"))
    XCTAssertTrue(visualStyleSource.contains("struct FawxDropdownActionRow"))
    XCTAssertTrue(visualStyleSource.contains(".fawxRowChrome("))
    XCTAssertTrue(
      visualStyleSource.contains("case .page, .rail, .section, .composer, .field, .transient, .callout, .code:")
    )
    XCTAssertTrue(visualStyleSource.contains("shadowStyle: FawxShadowStyle? = nil"))
    XCTAssertTrue(visualStyleSource.contains("enum FawxRowSelectionStyle"))
    XCTAssertTrue(visualStyleSource.contains("case accentOnly"))
    XCTAssertTrue(visualStyleSource.contains("func fawxSurface(_ role: FawxSurfaceRole)"))
    XCTAssertTrue(visualStyleSource.contains("func fawxRowChrome("))
    XCTAssertTrue(visualStyleSource.contains("if selectionStyle == .accentOnly"))

    XCTAssertTrue(sidebarSource.contains(".fawxSurface(.rail)"))
    XCTAssertTrue(sidebarSource.contains(".listStyle(.plain)"))
    XCTAssertTrue(sidebarSource.contains("private var sidebarSearchField: some View"))
    XCTAssertTrue(sidebarSource.contains("threadSearchField"))
    XCTAssertTrue(sidebarSource.contains(".fawxRowChrome(isSelected: isSelected"))
    XCTAssertTrue(sidebarSource.contains("selectionStyle: .accentOnly"))
    XCTAssertTrue(sidebarSource.contains("isActiveContext ? Color.fawxAccent"))
    XCTAssertFalse(sidebarSource.contains(".background(Color.fawxSurface)"))
    XCTAssertFalse(sidebarSource.contains(".searchable("))

    XCTAssertTrue(settingsCategorySidebarSource.contains(".fawxSurface(.rail)"))
    XCTAssertFalse(settingsCategorySidebarSource.contains(".stroke(Color.fawxBorder"))
    XCTAssertTrue(settingsCategoryRowSource.contains(".fawxRowChrome("))
    XCTAssertTrue(settingsCategoryRowSource.contains("selectionStyle: .accentOnly"))
    XCTAssertFalse(settingsCategoryRowSource.contains("Color.fawxSurfaceActive"))

    XCTAssertTrue(inputBarSource.contains(".fawxSurface(.composer)"))
    XCTAssertTrue(inputBarSource.contains("private var messageFieldBorderColor: Color?"))
    XCTAssertFalse(inputBarSource.contains(".fawxShadow(FawxShadow.floatingPanel)"))
    XCTAssertFalse(chatSource.contains("FawxShadow.loadingOverlay"))
    XCTAssertFalse(chatSource.contains("FawxShadow.elevatedCapsule"))
    XCTAssertFalse(ripcordNotificationSource.contains("FawxShadow.floatingPanel"))
  }

  func testSettingsDetailPanelsUseSemanticSurfacePrimitives() throws {
    let settingsSource = try sourceFile(at: "app/Fawx/Views/macOS/SettingsView.swift")
    let serverSource = try sourceFile(at: "app/Fawx/Views/Shared/ServerSettingsPanel.swift")
    let pairingSource = try sourceFile(at: "app/Fawx/Views/Shared/PairingSettingsPanel.swift")
    let usageSource = try sourceFile(at: "app/Fawx/Views/Shared/UsageSettingsPanel.swift")
    let telemetrySource = try sourceFile(at: "app/Fawx/Views/Shared/TelemetrySettingsPanel.swift")
    let synthesisSource = try sourceFile(at: "app/Fawx/Views/Shared/SynthesisSettingsPanel.swift")
    let permissionsSource = try sourceFile(at: "app/Fawx/Views/Shared/PermissionsSettingsPanel.swift")
    let appearanceSource = try sourceFile(at: "app/Fawx/Views/Shared/AppearanceSettingsPanel.swift")
    let authSource = try sourceFile(at: "app/Fawx/Views/Shared/AuthStatusList.swift")
    let sandboxSource = try sourceFile(at: "app/Fawx/Views/Shared/SandboxStatusCard.swift")
    let modelSelectionSource = try sourceFile(at: "app/Fawx/Views/Shared/ModelSelectionList.swift")

    let sectionPanelSources = [
      settingsSource,
      serverSource,
      pairingSource,
      usageSource,
      telemetrySource,
      synthesisSource,
      permissionsSource,
    ]
    for source in sectionPanelSources {
      XCTAssertTrue(source.contains(".fawxSurface(.section)"))
      XCTAssertFalse(source.contains(".background(Color.fawxSurface)"))
    }

    XCTAssertTrue(settingsSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(appearanceSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(appearanceSource.contains("ThemeSelectionControl"))
    XCTAssertTrue(appearanceSource.contains("themeSelectionControl"))
    XCTAssertFalse(appearanceSource.contains("Picker(\"Theme\""))
    XCTAssertTrue(appearanceSource.contains("accentColorPalette"))
    XCTAssertTrue(appearanceSource.contains("accentChannelBinding(.red)"))
    XCTAssertTrue(appearanceSource.contains("AccentChannelSlider"))
    XCTAssertTrue(appearanceSource.contains("tint: Color.fawxAccent"))
    XCTAssertTrue(appearanceSource.contains("appState.setAccentColor"))
    XCTAssertFalse(appearanceSource.contains("Slider(value: value, in: 0 ... 255"))
    XCTAssertTrue(telemetrySource.contains(".labelsHidden()\n            .tint(.fawxAccent)"))
    XCTAssertTrue(authSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(sandboxSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(modelSelectionSource.contains(".fawxSurface(.callout)"))
    XCTAssertTrue(modelSelectionSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(modelSelectionSource.contains(".fawxRowChrome(isSelected: isSelected)"))
    XCTAssertTrue(
      modelSelectionSource.contains(".foregroundStyle(isFavorite ? Color.fawxAccent : Color.fawxTextSecondary)")
    )
    XCTAssertFalse(
      modelSelectionSource.contains(".foregroundStyle(isFavorite ? Color.fawxWarning : Color.fawxTextSecondary)")
    )
    XCTAssertFalse(
      modelSelectionSource.contains(".background(isSelected ? Color.fawxAccent.opacity(0.08) : Color.fawxSurface)")
    )
  }

  func testCustomInstructionsSettingsUseAgentPreferenceContract() throws {
    let panelSource = try sourceFile(at: "app/Fawx/Views/Shared/SynthesisSettingsPanel.swift")
    let viewModelSource = try sourceFile(at: "app/Fawx/ViewModels/SynthesisViewModel.swift")
    let validationSource = try sourceFile(at: "engine/crates/fx-config/src/validation.rs")
    let systemPromptSource = try sourceFile(at: "engine/crates/fx-kernel/src/system_prompt.rs")
    let requestSource = try sourceFile(at: "engine/crates/fx-kernel/src/loop_engine/request.rs")
    let startupSource = try sourceFile(at: "engine/crates/fx-cli/src/startup.rs")

    XCTAssertTrue(viewModelSource.contains("static let customInstructionsMaxLength = 4000"))
    XCTAssertTrue(viewModelSource.contains("\"custom_instructions\""))
    XCTAssertTrue(viewModelSource.contains("\"custom_personality\""))
    XCTAssertTrue(viewModelSource.contains("let isCustomPersonality = personalityID == \"custom\""))
    XCTAssertTrue(
      viewModelSource.contains("\"custom_instructions\": .string(isCustomPersonality ? \"\" : instructions)")
    )
    XCTAssertTrue(viewModelSource.contains("[\"model\", \"synthesis_instruction\"]"))
    XCTAssertTrue(viewModelSource.contains("\"personality\""))
    XCTAssertTrue(viewModelSource.contains("id: \"professional\""))
    XCTAssertTrue(viewModelSource.contains("id: \"technical\""))
    XCTAssertTrue(viewModelSource.contains("id: \"caveman\""))
    XCTAssertTrue(viewModelSource.contains("id: \"custom\""))
    XCTAssertTrue(
      viewModelSource.contains("Define the interaction style in the custom instructions field below.")
    )
    XCTAssertTrue(viewModelSource.contains("normalized == \"minimal\""))
    XCTAssertTrue(viewModelSource.contains("appState.client.patchConfig(changes: preferencePatch"))
    XCTAssertTrue(viewModelSource.contains("appState.client.serverConfig()"))
    XCTAssertFalse(viewModelSource.contains("appState.client.setSynthesis"))
    XCTAssertFalse(viewModelSource.contains("appState.client.getSynthesis"))
    XCTAssertFalse(viewModelSource.contains("appState.client.clearSynthesis"))

    XCTAssertTrue(panelSource.contains("personalityPicker"))
    XCTAssertTrue(panelSource.contains("PersonalitySelectionControl"))
    XCTAssertTrue(panelSource.contains("SettingsActionButton"))
    XCTAssertTrue(panelSource.contains("SettingsInstructionsTextEditor"))
    XCTAssertTrue(panelSource.contains("viewModel.updateText($0)"))
    XCTAssertTrue(panelSource.contains(".fawxSurface(.field)"))
    XCTAssertTrue(panelSource.contains("remaining"))
    XCTAssertFalse(panelSource.contains(".pickerStyle(.segmented)"))
    XCTAssertFalse(panelSource.contains(".buttonStyle(.borderedProminent)"))

    XCTAssertTrue(validationSource.contains("MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 4000"))
    XCTAssertTrue(validationSource.contains("MAX_CUSTOM_INSTRUCTION_LENGTH: usize = 4000"))
    XCTAssertTrue(validationSource.contains("VALID_AGENT_PERSONALITIES"))
    XCTAssertTrue(validationSource.contains("\"caveman\" | \"custom\""))
    XCTAssertTrue(validationSource.contains("validate_custom_instructions"))
    XCTAssertTrue(systemPromptSource.contains("\"direct\" => Personality::Direct"))
    XCTAssertTrue(systemPromptSource.contains("\"caveman\" | \"minimal\" => Personality::Caveman"))
    XCTAssertTrue(systemPromptSource.contains("CAVEMAN_IDENTITY_TEMPLATE"))
    XCTAssertTrue(systemPromptSource.contains("DIRECT_IDENTITY_TEMPLATE"))
    XCTAssertTrue(systemPromptSource.contains("if personality == instructions => None"))
    XCTAssertTrue(systemPromptSource.contains("runtime_agent_preferences_from_config"))
    XCTAssertTrue(requestSource.contains("Configured agent preferences:"))
    XCTAssertTrue(requestSource.contains("with_agent_preferences"))
    XCTAssertTrue(startupSource.contains("runtime_agent_preferences_from_config(&config.agent)"))
    XCTAssertTrue(startupSource.contains(".agent_preferences("))
  }

  func testGitSurfacesCancelRefreshWhenHidden() throws {
    let gitViewSource = try sourceFile(at: "app/Fawx/Views/Shared/GitView.swift")
    let compactPanelSource = try sourceFile(at: "app/Fawx/Views/macOS/CompactGitPanel.swift")

    XCTAssertTrue(gitViewSource.contains("private struct GitOperationsMenu"))
    XCTAssertTrue(gitViewSource.contains("gitOperationsMenu"))
    XCTAssertTrue(gitViewSource.contains("private struct GitMenuLabel"))
    XCTAssertTrue(gitViewSource.contains("gitTargetPickerMenu"))
    XCTAssertTrue(gitViewSource.contains("FawxDropdownMenu"))
    XCTAssertTrue(gitViewSource.contains(".buttonStyle(.plain)"))
    XCTAssertFalse(gitViewSource.contains("GitInlineActionButton"))
    XCTAssertFalse(gitViewSource.contains("gitInlineActionBar"))
    XCTAssertFalse(gitViewSource.contains(".toolbar {"))
    XCTAssertFalse(gitViewSource.contains("ToolbarItemGroup"))
    XCTAssertTrue(gitViewSource.contains("viewModel.cancelRefresh()"))
    XCTAssertTrue(gitViewSource.contains(".onDisappear"))
    XCTAssertTrue(compactPanelSource.contains("viewModel.cancelRefresh()"))
    XCTAssertTrue(compactPanelSource.contains(".onChange(of: selectedSection)"))
    XCTAssertTrue(compactPanelSource.contains(".onDisappear"))
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
