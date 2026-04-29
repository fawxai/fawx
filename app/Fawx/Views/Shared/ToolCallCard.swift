import SwiftUI

enum AssistantActivityState: Equatable, CaseIterable {
    case queued
    case running
    case completed
    case failed
    case cancelled
    case deferred

    var label: String {
        switch self {
        case .queued:
            "Queued"
        case .running:
            "Running"
        case .completed:
            "Complete"
        case .failed:
            "Failed"
        case .cancelled:
            "Cancelled"
        case .deferred:
            "Deferred"
        }
    }

    var actionPrefix: String {
        switch self {
        case .queued:
            "Queued"
        case .running:
            "Running"
        case .completed:
            "Ran"
        case .failed:
            "Failed"
        case .cancelled:
            "Cancelled"
        case .deferred:
            "Deferred"
        }
    }

    var systemImage: String {
        switch self {
        case .queued:
            "clock"
        case .running:
            "arrow.triangle.2.circlepath"
        case .completed:
            "checkmark.circle"
        case .failed:
            "exclamationmark.triangle"
        case .cancelled:
            "xmark.circle"
        case .deferred:
            "pause.circle"
        }
    }

    var tint: Color {
        switch self {
        case .queued, .deferred:
            .fawxWarning
        case .running:
            .fawxAccent
        case .completed:
            .fawxTextSecondary
        case .failed, .cancelled:
            .fawxError
        }
    }
}

private func pluralForm(of singular: String) -> String {
    switch singular {
    case "search":
        return "searches"
    default:
        return "\(singular)s"
    }
}

struct AssistantActivityTimelineSnapshot: Equatable {
    enum DetailStyle: Equatable {
        case collapsed
        case liveStatusOnly
        case historicalPayload
    }

    let headerTitle: String
    let groupSummary: String
    let statusText: String
    let state: AssistantActivityState
    let accessibilityLabel: String
    let accessibilityHint: String
    let showsProgress: Bool
    let canExpand: Bool
    let isExpanded: Bool
    let hasToolCalls: Bool
    let visibleToolCalls: [ToolCallRecord]
    let rows: [AssistantActivityEventSnapshot]
    let detailStyle: DetailStyle

    init(group: ToolActivityGroupRecord, isExpanded: Bool) {
        let canExpand = !group.toolCalls.isEmpty
        let effectiveExpanded = canExpand && isExpanded
        let primaryToolCall = group.toolCalls.last(where: \.isRunning) ?? group.toolCalls.last
        let detailStyle: DetailStyle

        if !effectiveExpanded {
            detailStyle = .collapsed
        } else if group.isLive {
            detailStyle = .liveStatusOnly
        } else {
            detailStyle = .historicalPayload
        }

        let state = Self.state(for: group)
        let headerTitle = Self.headerTitle(
            primaryToolCall: primaryToolCall,
            toolCount: group.toolCount,
            toolCalls: group.toolCalls,
            state: state
        )
        let groupSummary = Self.groupSummary(for: group, primaryToolCall: primaryToolCall, state: state)

        self.headerTitle = headerTitle
        self.groupSummary = groupSummary
        self.statusText = state.label
        self.state = state
        accessibilityLabel = "\(headerTitle), \(groupSummary)"
        if !canExpand {
            accessibilityHint = "Tool activity details are unavailable."
        } else if effectiveExpanded {
            if group.isLive {
                accessibilityHint = "Collapse activity. Detailed arguments and output appear after the response finishes."
            } else {
                accessibilityHint = "Collapse activity"
            }
        } else {
            accessibilityHint = "Expand activity"
        }
        showsProgress = state == .running
        self.canExpand = canExpand
        self.isExpanded = effectiveExpanded
        hasToolCalls = !group.toolCalls.isEmpty
        visibleToolCalls = effectiveExpanded ? group.toolCalls : []
        rows = effectiveExpanded
            ? group.toolCalls.map { AssistantActivityEventSnapshot(toolCall: $0) }
            : []
        self.detailStyle = detailStyle
    }

    var showsPayloadDetails: Bool {
        detailStyle == .historicalPayload
    }

    private static func state(for group: ToolActivityGroupRecord) -> AssistantActivityState {
        if group.isLive && group.toolCalls.isEmpty {
            return .running
        }

        if group.runningCount > 0 {
            return .running
        }

        if group.errorCount > 0 {
            return .failed
        }

        return .completed
    }

    private static func headerTitle(
        primaryToolCall: ToolCallRecord?,
        toolCount: Int,
        toolCalls: [ToolCallRecord],
        state: AssistantActivityState
    ) -> String {
        guard let primaryToolCall else {
            return state == .running ? "Thinking" : "Activity"
        }

        if (state == .completed || state == .running), toolCount > 1 {
            return aggregateActivityTitle(for: toolCount, toolCalls: toolCalls, state: state)
        }

        let additionalToolCount = max(0, toolCount - 1)
        let narratedTitle = ToolActivityNarrator.title(for: primaryToolCall, state: state)
        if additionalToolCount > 0 {
            return "\(narratedTitle) +\(additionalToolCount)"
        }
        return narratedTitle
    }

    private static func aggregateActivityTitle(
        for toolCount: Int,
        toolCalls: [ToolCallRecord],
        state: AssistantActivityState
    ) -> String {
        let completedToolCalls = toolCalls.filter { !$0.isRunning }
        let summarizedToolCalls = state == .running && !completedToolCalls.isEmpty
            ? completedToolCalls
            : toolCalls
        let summary = ActivityKindSummary(toolCalls: summarizedToolCalls)
        guard summary.hasClassifiedActivity else {
            return "Ran \(summarizedToolCalls.count) \(plural("tool", count: summarizedToolCalls.count))"
        }

        return summary.completedSummary
    }

    private static func groupSummary(
        for group: ToolActivityGroupRecord,
        primaryToolCall: ToolCallRecord?,
        state: AssistantActivityState
    ) -> String {
        if group.toolCalls.isEmpty {
            return group.isLive ? "Working through the request" : "Activity complete"
        }

        if group.isLive,
           let primaryToolCall,
           let detail = ToolActivityNarrator.detail(for: primaryToolCall, state: state) {
            return detail
        }

        let countLabel = "\(group.toolCount) \(plural("tool", count: group.toolCount))"

        if group.runningCount > 0 {
            let runningLabel = group.runningCount == 1 ? "1 running" : "\(group.runningCount) running"
            return "\(countLabel), \(runningLabel)"
        }

        if group.errorCount > 0 {
            let failedLabel = group.errorCount == 1 ? "1 failed" : "\(group.errorCount) failed"
            return "\(countLabel), \(failedLabel)"
        }

        if group.completedCount == group.toolCount {
            return "\(countLabel), completed"
        }

        return countLabel
    }

    private static func plural(_ singular: String, count: Int) -> String {
        count == 1 ? singular : pluralForm(of: singular)
    }
}

struct CompletedWorkSummarySnapshot: Equatable {
    let elapsedText: String
    let summaryText: String?
    let hasActivity: Bool
    let entries: [CompletedWorkEntrySnapshot]

    init(summary: CompletedWorkSummaryRecord) {
        elapsedText = summary.elapsedText
        summaryText = summary.summaryText
        entries = summary.entries
            .compactMap(CompletedWorkEntrySnapshot.init(entry:))
        hasActivity = !entries.isEmpty
    }
}

enum CompletedWorkEntrySnapshot: Identifiable, Equatable {
    case narration(CompletedWorkNarrationSnapshot)
    case toolChunk(CompletedWorkChunkSnapshot)
    case turnSteering(TurnSteeringRecord)

    init?(entry: CompletedWorkEntry) {
        switch entry {
        case .narration(let narration):
            let snapshot = CompletedWorkNarrationSnapshot(narration: narration)
            guard snapshot.hasVisibleContent else {
                return nil
            }
            self = .narration(snapshot)
        case .toolActivityGroup(let group):
            let snapshot = CompletedWorkChunkSnapshot(group: group)
            guard snapshot.hasVisibleContent else {
                return nil
            }
            self = .toolChunk(snapshot)
        case .turnSteering(let steering):
            guard !steering.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return nil
            }
            self = .turnSteering(steering)
        }
    }

    var id: String {
        switch self {
        case .narration(let narration):
            return narration.id
        case .toolChunk(let chunk):
            return chunk.id
        case .turnSteering(let steering):
            return "turn-steering:\(steering.id)"
        }
    }
}

struct CompletedWorkNarrationSnapshot: Identifiable, Equatable {
    let id: String
    let text: String

    init(narration: CompletedWorkNarrationRecord) {
        id = narration.id
        text = narration.text
    }

    var hasVisibleContent: Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty != nil
    }
}

struct CompletedWorkChunkSnapshot: Identifiable, Equatable {
    let id: String
    let toolTitle: String?
    let state: AssistantActivityState
    let rows: [AssistantActivityEventSnapshot]

    init(group: ToolActivityGroupRecord) {
        id = group.id
        toolTitle = Self.toolTitle(for: group)
        state = AssistantActivityTimelineSnapshot(group: group, isExpanded: false).state
        rows = group.toolCalls.map { AssistantActivityEventSnapshot(toolCall: $0) }
    }

    var hasVisibleContent: Bool {
        toolTitle != nil || !rows.isEmpty
    }

    var canExpand: Bool {
        !rows.isEmpty
    }

    private static func toolTitle(for group: ToolActivityGroupRecord) -> String? {
        guard group.toolCount > 0 else {
            return nil
        }

        if group.toolCount == 1, let toolCall = group.toolCalls.first {
            let row = AssistantActivityEventSnapshot(toolCall: toolCall)
            return row.title
        }

        return ActivityKindSummary(toolCalls: group.toolCalls).completedChunkTitle
    }
}

private struct ActivityKindSummary {
    let fileCount: Int
    let searchCount: Int
    let commandCount: Int
    let editCount: Int
    let otherCount: Int

    init(toolCalls: [ToolCallRecord]) {
        var fileCount = 0
        var searchCount = 0
        var commandCount = 0
        var editCount = 0
        var otherCount = 0

        for toolCall in toolCalls {
            switch toolCall.activityDescriptor.kind {
            case .file:
                fileCount += 1
            case .search:
                searchCount += 1
            case .command:
                commandCount += 1
            case .edit:
                editCount += 1
            case .other:
                otherCount += 1
            }
        }

        self.fileCount = fileCount
        self.searchCount = searchCount
        self.commandCount = commandCount
        self.editCount = editCount
        self.otherCount = otherCount
    }

    var hasClassifiedActivity: Bool {
        fileCount + searchCount + commandCount + editCount > 0
    }

    var completedSummary: String {
        var parts: [String] = []

        if editCount > 0 {
            parts.append("edited \(editCount) \(Self.plural("file", count: editCount))")
        }
        if fileCount > 0 {
            parts.append("explored \(fileCount) \(Self.plural("file", count: fileCount))")
        }
        if searchCount > 0 {
            parts.append("\(searchCount) \(Self.plural("search", count: searchCount))")
        }
        if commandCount > 0 {
            parts.append("ran \(commandCount) \(Self.plural("command", count: commandCount))")
        }
        if otherCount > 0 {
            parts.append("used \(otherCount) \(Self.plural("tool", count: otherCount))")
        }

        guard let first = parts.first else {
            return "Ran activity"
        }

        return first.prefix(1).uppercased()
            + String(first.dropFirst())
            + joinedSuffix(for: Array(parts.dropFirst()))
    }

    var completedChunkTitle: String {
        if commandCount == totalCount {
            return "Ran \(commandCount) \(Self.plural("command", count: commandCount))"
        }
        if editCount == totalCount {
            return "Edited \(editCount) \(Self.plural("file", count: editCount))"
        }
        if fileCount == totalCount {
            return "Read \(fileCount) \(Self.plural("file", count: fileCount))"
        }
        if searchCount == totalCount {
            return "Ran \(searchCount) \(Self.plural("search", count: searchCount))"
        }
        if hasClassifiedActivity {
            return completedSummary
        }
        return "Used \(totalCount) \(Self.plural("tool", count: totalCount))"
    }

    private var totalCount: Int {
        fileCount + searchCount + commandCount + editCount + otherCount
    }

    private static func plural(_ singular: String, count: Int) -> String {
        count == 1 ? singular : pluralForm(of: singular)
    }

    private func joinedSuffix(for parts: [String]) -> String {
        guard !parts.isEmpty else {
            return ""
        }

        return ", " + parts.joined(separator: ", ")
    }
}

struct AssistantActivityEventSnapshot: Identifiable, Equatable {
    let id: String
    let title: String
    let summary: String
    let state: AssistantActivityState
    let arguments: String
    let result: String?
    let isError: Bool
    let detailSections: [AssistantActivityDetailSectionSnapshot]
    let hasDetails: Bool

    init(toolCall: ToolCallRecord) {
        id = toolCall.id
        state = Self.state(for: toolCall)
        title = Self.title(for: toolCall, state: state)
        detailSections = AssistantActivityDetailBuilder.sections(for: toolCall)
        hasDetails = !detailSections.isEmpty
        summary = Self.summary(for: toolCall, state: state, detailSections: detailSections)
        arguments = toolCall.arguments
        result = toolCall.result
        isError = toolCall.isError
    }

    private static func state(for toolCall: ToolCallRecord) -> AssistantActivityState {
        if toolCall.isRunning {
            return .running
        }

        if toolCall.isError {
            return .failed
        }

        return .completed
    }

    private static func title(
        for toolCall: ToolCallRecord,
        state: AssistantActivityState
    ) -> String {
        ToolActivityNarrator.title(for: toolCall, state: state)
    }

    private static func summary(
        for toolCall: ToolCallRecord,
        state: AssistantActivityState,
        detailSections: [AssistantActivityDetailSectionSnapshot]
    ) -> String {
        switch state {
        case .running:
            return ToolActivityNarrator.detail(for: toolCall, state: state) ?? "Waiting for result"
        case .failed:
            if let progressSummary = ToolProgressNarrator.summary(for: toolCall.progress) {
                return progressSummary
            }
            return compactOutputSummary(toolCall.result) ?? "Finished with an error"
        case .completed:
            if let progressSummary = ToolProgressNarrator.summary(for: toolCall.progress) {
                return progressSummary
            }
            if detailSections.contains(where: { $0.title == "Diff" }) {
                return "Diff available"
            }
            if AssistantActivityDetailBuilder.isCodeMutation(toolCall.name) {
                return "Code change recorded"
            }
            if detailSections.contains(where: { $0.title == "Shell" }) {
                return "Command output available"
            }
            if !detailSections.isEmpty {
                return "Details available"
            }
            if toolCall.arguments.nonEmpty != nil {
                return "Inputs captured"
            }
            return "Completed"
        case .queued:
            return "Waiting to start"
        case .cancelled:
            return "Stopped before completion"
        case .deferred:
            return "Paused by policy"
        }
    }

    private static func compactOutputSummary(_ output: String?) -> String? {
        output?
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first(where: { !$0.isEmpty })
    }
}

private enum ToolProgressNarrator {
    static func summary(for progress: ToolProgressRecord?) -> String? {
        guard let progress else {
            return nil
        }

        if progress.isDuplicate {
            return compact("Repeated work", target: progress.targetDisplay)
        }

        if progress.isRetryableFailure {
            return compact("Retryable issue", target: progress.targetDisplay)
        }

        if progress.didAdvance {
            if progress.isMutation {
                return compact("Advanced task", target: progress.targetDisplay)
            }
            return compact("Advanced evidence", target: progress.targetDisplay)
        }

        return nil
    }

    private static func compact(_ prefix: String, target: String?) -> String {
        guard let target else {
            return prefix
        }

        let trimmed = target.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return prefix
        }

        return "\(prefix): \(trimmed.count > 96 ? String(trimmed.prefix(93)) + "..." : trimmed)"
    }
}

struct AssistantActivityDetailSectionSnapshot: Identifiable, Equatable {
    let id: String
    let title: String
    let language: String?
    let content: String

    init(title: String, language: String?, content: String) {
        id = "\(title):\(language ?? "")"
        self.title = title
        self.language = language
        self.content = content
    }
}

private enum AssistantActivityDetailBuilder {
    static func sections(for toolCall: ToolCallRecord) -> [AssistantActivityDetailSectionSnapshot] {
        let descriptor = toolCall.activityDescriptor
        if descriptor.isCodeMutation {
            return codeMutationSections(for: toolCall)
        }

        if descriptor.normalizedName == "git_diff" {
            return diffSections(for: toolCall)
        }

        if descriptor.isCommand {
            return commandSections(for: toolCall)
        }

        return genericSections(for: toolCall)
    }

    private static func codeMutationSections(
        for toolCall: ToolCallRecord
    ) -> [AssistantActivityDetailSectionSnapshot] {
        var sections: [AssistantActivityDetailSectionSnapshot] = []

        if let diff = codeDiff(for: toolCall) {
            sections.append(
                AssistantActivityDetailSectionSnapshot(
                    title: "Diff",
                    language: "diff",
                    content: diff
                )
            )
        }

        if toolCall.isError, let result = toolCall.result?.nonEmpty {
            sections.append(
                AssistantActivityDetailSectionSnapshot(
                    title: "Error Output",
                    language: nil,
                    content: result
                )
            )
        }

        return sections
    }

    private static func diffSections(for toolCall: ToolCallRecord) -> [AssistantActivityDetailSectionSnapshot] {
        guard let result = toolCall.result?.nonEmpty else {
            return genericSections(for: toolCall)
        }

        return [
            AssistantActivityDetailSectionSnapshot(
                title: "Diff",
                language: "diff",
                content: result
            )
        ]
    }

    private static func commandSections(for toolCall: ToolCallRecord) -> [AssistantActivityDetailSectionSnapshot] {
        guard let transcript = shellTranscript(for: toolCall)?.nonEmpty else {
            return genericSections(for: toolCall)
        }

        return [
            AssistantActivityDetailSectionSnapshot(
                title: "Shell",
                language: "shell",
                content: transcript
            )
        ]
    }

    private static func genericSections(for toolCall: ToolCallRecord) -> [AssistantActivityDetailSectionSnapshot] {
        var sections: [AssistantActivityDetailSectionSnapshot] = []
        if let arguments = toolCall.arguments.nonEmpty {
            sections.append(
                AssistantActivityDetailSectionSnapshot(
                    title: "Inputs",
                    language: "json",
                    content: arguments
                )
            )
        }
        if let result = toolCall.result?.nonEmpty {
            sections.append(
                AssistantActivityDetailSectionSnapshot(
                    title: toolCall.isError ? "Error Output" : "Result",
                    language: nil,
                    content: result
                )
            )
        }
        return sections
    }

    private static func shellTranscript(for toolCall: ToolCallRecord) -> String? {
        let command = toolCall.activityDescriptor.argumentValue(["command", "cmd"])
        let result = toolCall.result?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty

        switch (command, result) {
        case (.some(let command), .some(let result)):
            return "$ \(command)\n\(result)"
        case (.some(let command), .none):
            return "$ \(command)"
        case (.none, .some(let result)):
            return result
        case (.none, .none):
            return nil
        }
    }

    private static func codeDiff(for toolCall: ToolCallRecord) -> String? {
        switch toolCall.activityDescriptor.normalizedName {
        case "apply_patch":
            return stringValue(["patch", "diff"], in: toolCall.arguments)
                ?? rawPatch(from: toolCall.arguments)
        case "edit_file":
            return editFileDiff(from: toolCall.arguments)
        case "write_file":
            return writeFileDiff(from: toolCall.arguments)
        default:
            return nil
        }
    }

    private static func editFileDiff(from arguments: String) -> String? {
        guard let object = objectValue(from: arguments),
              let path = object["path"]?.stringValue?.nonEmpty,
              let oldText = object["old_text"]?.stringValue,
              let newText = object["new_text"]?.stringValue
        else {
            return nil
        }

        return [
            "--- a/\(path)",
            "+++ b/\(path)",
            "@@",
            prefixedLines(oldText, prefix: "-"),
            prefixedLines(newText, prefix: "+"),
        ]
        .joined(separator: "\n")
    }

    private static func writeFileDiff(from arguments: String) -> String? {
        guard let object = objectValue(from: arguments),
              let path = object["path"]?.stringValue?.nonEmpty,
              let content = object["content"]?.stringValue
        else {
            return nil
        }

        return [
            "--- /dev/null",
            "+++ b/\(path)",
            "@@",
            prefixedLines(content, prefix: "+"),
        ]
        .joined(separator: "\n")
    }

    private static func rawPatch(from arguments: String) -> String? {
        let trimmed = arguments.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix("*** Begin Patch") || trimmed.hasPrefix("diff --git") {
            return trimmed
        }
        return nil
    }

    private static func prefixedLines(_ value: String, prefix: String) -> String {
        let lines = value.split(separator: "\n", omittingEmptySubsequences: false)
        guard !lines.isEmpty else {
            return prefix
        }
        return lines.map { "\(prefix)\($0)" }.joined(separator: "\n")
    }

    private static func stringValue(_ keys: [String], in arguments: String) -> String? {
        guard let object = objectValue(from: arguments) else {
            return nil
        }

        for key in keys {
            if let value = object[key]?.stringValue?.nonEmpty {
                return value
            }
        }
        return nil
    }

    private static func objectValue(from arguments: String) -> [String: JSONValue]? {
        guard let data = arguments.data(using: .utf8),
              let value = try? JSONDecoder().decode(JSONValue.self, from: data),
              case .object(let object) = value
        else {
            return nil
        }
        return object
    }

    static func isCodeMutation(_ toolName: String) -> Bool {
        ToolActivityDescriptor(name: toolName, arguments: "").isCodeMutation
    }
}

private enum ToolActivityNarrator {
    static func title(for toolCall: ToolCallRecord, state: AssistantActivityState) -> String {
        let descriptor = toolCall.activityDescriptor
        let name = descriptor.normalizedName
        let target = descriptor.primaryTarget.map { compact($0, maxLength: 80) }

        switch name {
        case "read_file", "read":
            return phrase(state: state, running: "Reading", completed: "Read", failed: "Failed reading", target: target ?? "file")
        case "write_file", "edit_file", "apply_patch":
            return phrase(state: state, running: "Editing", completed: "Edited", failed: "Failed editing", target: target ?? "file")
        case "search_text", "search_files", "rg", "grep":
            return phrase(state: state, running: "Searching", completed: "Searched", failed: "Failed searching", target: target ?? "workspace")
        case "run_command", "exec_command", "shell":
            return phrase(state: state, running: "Running", completed: "Ran", failed: "Failed running", target: target ?? "command")
        case "git_status":
            return state == .running ? "Checking git status" : state == .failed ? "Failed checking git status" : "Checked git status"
        case "git_diff":
            return state == .running ? "Inspecting git diff" : state == .failed ? "Failed inspecting git diff" : "Inspected git diff"
        case "git_checkpoint":
            return state == .running ? "Creating git checkpoint" : state == .failed ? "Failed creating git checkpoint" : "Created git checkpoint"
        case "web_fetch", "fetch_url":
            return phrase(state: state, running: "Fetching", completed: "Fetched", failed: "Failed fetching", target: target ?? "web page")
        case "web_search":
            return phrase(state: state, running: "Searching web for", completed: "Searched web for", failed: "Failed web search for", target: target ?? "query")
        case "memory_search":
            return phrase(state: state, running: "Searching memory for", completed: "Searched memory for", failed: "Failed memory search for", target: target ?? "context")
        default:
            return phrase(
                state: state,
                running: "Running",
                completed: "Ran",
                failed: "Failed",
                target: humanizedToolName(toolCall.displayName)
            )
        }
    }

    static func detail(for toolCall: ToolCallRecord, state: AssistantActivityState) -> String? {
        guard state == .running else {
            return nil
        }

        let descriptor = toolCall.activityDescriptor
        let name = descriptor.normalizedName
        switch name {
        case "run_command", "exec_command", "shell":
            return descriptor.argumentValue(["command", "cmd"])
                .map { compact($0, maxLength: 96) }
        case "search_text", "search_files", "rg", "grep", "web_search", "memory_search":
            return descriptor.argumentValue(["pattern", "query", "q"])
                .map { "Looking for \(compact($0, maxLength: 72))" }
        case "read_file", "write_file", "edit_file", "apply_patch", "web_fetch", "fetch_url":
            return descriptor.primaryTarget.map { compact($0, maxLength: 96) }
        default:
            return descriptor.primaryTarget.map { compact($0, maxLength: 96) }
        }
    }

    private static func phrase(
        state: AssistantActivityState,
        running: String,
        completed: String,
        failed: String,
        target: String
    ) -> String {
        switch state {
        case .running, .queued:
            return "\(running) \(target)"
        case .failed:
            return "\(failed) \(target)"
        case .cancelled:
            return "Stopped \(target)"
        case .deferred:
            return "Paused \(target)"
        case .completed:
            return "\(completed) \(target)"
        }
    }

    private static func humanizedToolName(_ name: String) -> String {
        let humanized = name
            .replacingOccurrences(of: "_", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return humanized.isEmpty ? "tool" : humanized
    }

    private static func compact(_ value: String, maxLength: Int) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count > maxLength else {
            return trimmed
        }
        return String(trimmed.prefix(max(0, maxLength - 3))) + "..."
    }
}

struct AssistantActivityTimeline: View {
    @Environment(\.containerWidth) private var containerWidth

    let group: ToolActivityGroupRecord
    let shouldCollapseOnComplete: Bool

    @State private var isExpanded: Bool
    @State private var hasCollapsedOnCompletion = false
    @State private var isHovering = false

    init(
        group: ToolActivityGroupRecord,
        defaultExpanded: Bool = false,
        shouldCollapseOnComplete: Bool = false
    ) {
        self.group = group
        self.shouldCollapseOnComplete = shouldCollapseOnComplete
        _isExpanded = State(initialValue: defaultExpanded)
    }

    private var snapshot: AssistantActivityTimelineSnapshot {
        AssistantActivityTimelineSnapshot(group: group, isExpanded: isExpanded)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            timelineContent

            Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onChange(of: group.isLive) { _, isLive in
            guard shouldCollapseOnComplete, !isLive, isExpanded, !hasCollapsedOnCompletion else {
                return
            }

            withAnimation(FawxAnimation.expand) {
                isExpanded = false
            }
            hasCollapsedOnCompletion = true
        }
    }

    private var timelineContent: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if snapshot.hasToolCalls {
                Button {
                    guard snapshot.canExpand else {
                        return
                    }
                    withAnimation(FawxAnimation.expand) {
                        isExpanded.toggle()
                    }
                } label: {
                    headerLabel
                }
                .buttonStyle(.plain)
                .disabled(!snapshot.canExpand)
                .accessibilityLabel(snapshot.accessibilityLabel)
                .accessibilityHint(snapshot.accessibilityHint)
    #if os(macOS)
                .onHover { isHovering = $0 }
    #endif
            }

            if snapshot.isExpanded {
                expandedTimeline
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .frame(maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth), alignment: .leading)
        .accessibilityElement(children: .contain)
    }

    private var headerLabel: some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
            if snapshot.showsProgress {
                ProgressView()
                    .controlSize(.small)
                    .scaleEffect(0.62)
                    .tint(summaryColor)
            }

            Text(snapshot.headerTitle)
                .font(FawxTypography.status.weight(.medium))
                .foregroundStyle(summaryColor)
                .lineLimit(1)

            if snapshot.canExpand {
                Image(systemName: snapshot.isExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(summaryColor.opacity(isHovering ? 1 : 0.7))
            }
        }
        .padding(.vertical, 1)
        .contentShape(Rectangle())
    }

    private var summaryColor: Color {
        switch snapshot.state {
        case .failed, .cancelled:
            Color.fawxError
        case .queued, .deferred:
            Color.fawxWarning
        case .running, .completed:
            Color.fawxTextSecondary
        }
    }

    private var expandedTimeline: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if snapshot.detailStyle == .liveStatusOnly {
                Text("Detailed arguments and output appear after the response finishes.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .padding(.leading, activityRailInset)
            }

            ForEach(Array(snapshot.rows.enumerated()), id: \.element.id) { index, row in
                if index > 0 {
                    Divider()
                        .overlay(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                        .padding(.leading, activityRailInset)
                }

                AssistantActivityEventRow(
                    row: row,
                    showsPayloadDetails: snapshot.showsPayloadDetails
                )
            }
        }
        .padding(.leading, FawxSpacing.paddingSM)
        .overlay(alignment: .leading) {
            Rectangle()
                .fill(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                .frame(width: 1)
                .padding(.leading, 7)
        }
    }

    private var activityRailInset: CGFloat {
        FawxSpacing.paddingLG
    }
}

struct CompletedWorkSummaryView: View {
    @Environment(\.containerWidth) private var containerWidth

    let summary: CompletedWorkSummaryRecord

    @State private var isExpanded = false
    @State private var isHovering = false

    private var snapshot: CompletedWorkSummarySnapshot {
        CompletedWorkSummarySnapshot(summary: summary)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Button {
                    guard snapshot.hasActivity else {
                        return
                    }
                    withAnimation(FawxAnimation.expand) {
                        isExpanded.toggle()
                    }
                } label: {
                    headerLabel
                }
                .buttonStyle(.plain)
                .disabled(!snapshot.hasActivity)
                .accessibilityLabel(summary.elapsedText)
                .accessibilityHint(snapshot.hasActivity ? "Toggle completed work details" : "")
    #if os(macOS)
                .onHover { isHovering = $0 }
    #endif

                if let summaryText = snapshot.summaryText {
                    AssistantActivityNarrationView(text: summaryText)
                }

                if isExpanded, snapshot.hasActivity {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                        ForEach(snapshot.entries) { entry in
                            switch entry {
                            case .narration(let narration):
                                AssistantActivityNarrationView(text: narration.text)
                            case .toolChunk(let chunk):
                                CompletedWorkChunkView(chunk: chunk)
                            case .turnSteering(let steering):
                                CompletedWorkSteeringView(steering: steering)
                            }
                        }
                    }
                    .padding(.leading, FawxSpacing.paddingLG)
                    .transition(.opacity.combined(with: .move(edge: .top)))
                }
            }
            .frame(maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth), alignment: .leading)

            Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var headerLabel: some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
            Text(summary.elapsedText)
                .font(FawxTypography.status.weight(.semibold))
                .foregroundStyle(Color.fawxSuccess)
                .lineLimit(1)

            if snapshot.hasActivity {
                Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Color.fawxSuccess.opacity(isHovering ? 1 : 0.7))
            }
        }
        .padding(.vertical, 1)
        .contentShape(Rectangle())
    }
}

private struct CompletedWorkSteeringView: View {
    let steering: TurnSteeringRecord

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
            Image(systemName: "arrow.turn.down.right")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 14, alignment: .center)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text("Steered this turn")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Text(steering.text)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface.opacity(FawxOpacity.surfaceMuted))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderSubtle), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct CompletedWorkChunkView: View {
    let chunk: CompletedWorkChunkSnapshot

    @State private var isExpanded = false
    @State private var isHovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Button {
                guard chunk.canExpand else {
                    return
                }
                withAnimation(FawxAnimation.expand) {
                    isExpanded.toggle()
                }
            } label: {
                toolHeaderLabel(title: chunk.toolTitle ?? "Tool activity")
            }
            .buttonStyle(.plain)
            .disabled(!chunk.canExpand)
            .accessibilityLabel(chunk.toolTitle ?? "Tool activity")
            .accessibilityHint(chunk.canExpand ? "Toggle completed tool details" : "")
#if os(macOS)
            .onHover { isHovering = $0 }
#endif

            if isExpanded, chunk.canExpand {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    ForEach(Array(chunk.rows.enumerated()), id: \.element.id) { index, row in
                        if index > 0 {
                            Divider()
                                .overlay(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                                .padding(.leading, activityRailInset)
                        }

                        AssistantActivityEventRow(
                            row: row,
                            showsPayloadDetails: true
                        )
                    }
                }
                .padding(.leading, FawxSpacing.paddingLG)
                .overlay(alignment: .leading) {
                    Rectangle()
                        .fill(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                        .frame(width: 1)
                        .padding(.leading, 7)
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }

    private func toolHeaderLabel(title: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.status.weight(.medium))
                .foregroundStyle(summaryColor)
                .lineLimit(1)

            if chunk.canExpand {
                Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(summaryColor.opacity(isHovering ? 1 : 0.7))
            }
        }
        .padding(.vertical, 1)
        .contentShape(Rectangle())
    }

    private var summaryColor: Color {
        switch chunk.state {
        case .failed, .cancelled:
            Color.fawxError
        case .queued, .deferred:
            Color.fawxWarning
        case .running, .completed:
            Color.fawxTextSecondary
        }
    }

    private var activityRailInset: CGFloat {
        FawxSpacing.paddingLG
    }
}

private struct AssistantActivityNarrationView: View {
    let text: String

    var body: some View {
        narrationContent
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, FawxSpacing.paddingXS)
            .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var narrationContent: some View {
#if os(macOS)
        TranscriptMarkdownContentView(text: text, alignment: .left)
#else
        TranscriptMarkdownText(text: text)
            .textSelection(.enabled)
#endif
    }
}

private struct AssistantActivityEventRow: View {
    let row: AssistantActivityEventSnapshot
    let showsPayloadDetails: Bool

    @State private var isDetailsExpanded = false
    @State private var isHovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if canExpandDetails {
                Button {
                    withAnimation(FawxAnimation.expand) {
                        isDetailsExpanded.toggle()
                    }
                } label: {
                    headerRow
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(row.title), \(row.summary)")
                .accessibilityHint(isDetailsExpanded ? "Collapse tool details" : "Expand tool details")
        #if os(macOS)
                .onHover { isHovering = $0 }
        #endif
            } else {
                headerRow
            }

            if canExpandDetails && isDetailsExpanded {
                detailSections
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(.leading, FawxSpacing.paddingLG)
        .padding(.vertical, FawxSpacing.paddingXS)
    }

    private var canExpandDetails: Bool {
        showsPayloadDetails && row.hasDetails
    }

    private var headerRow: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
            activityNode
                .padding(.top, 3)

            VStack(alignment: .leading, spacing: 2) {
                Text(row.title)
                    .font(FawxTypography.status.weight(.semibold))
                    .foregroundStyle(Color.fawxText)
                    .lineLimit(1)

                if !row.summary.isEmpty {
                    Text(row.summary)
                        .font(FawxTypography.status)
                        .foregroundStyle(row.isError ? Color.fawxError : Color.fawxTextSecondary)
                        .lineLimit(showsPayloadDetails ? 2 : 1)
                }
            }

            Spacer(minLength: 0)

            if canExpandDetails {
                Image(systemName: isDetailsExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Color.fawxTextSecondary.opacity(isHovering ? 1 : 0.7))
                    .padding(.top, 2)
            }
        }
        .contentShape(Rectangle())
    }

    private var detailSections: some View {
        ForEach(row.detailSections) { section in
            detailSection(title: section.title) {
                CodeBlock(language: section.language, content: section.content)
            }
        }
    }

    private var activityNode: some View {
        ZStack {
            Circle()
                .stroke(row.state.tint.opacity(0.75), lineWidth: 1)
                .frame(width: 12, height: 12)

            if row.state == .completed {
                Circle()
                    .fill(row.state.tint.opacity(0.75))
                    .frame(width: 4, height: 4)
            } else {
                Image(systemName: row.state.systemImage)
                    .font(.system(size: 7, weight: .semibold))
                    .foregroundStyle(row.state.tint)
            }
        }
        .frame(width: 14, height: 14)
    }

    private func detailSection<Content: View>(
        title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .textCase(.uppercase)
                .accessibilityAddTraits(.isHeader)

            content()
        }
        .padding(.top, FawxSpacing.paddingXS)
        .padding(.leading, FawxSpacing.paddingLG)
        .accessibilityElement(children: .contain)
    }
}
