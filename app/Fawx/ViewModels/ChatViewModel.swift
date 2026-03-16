import Foundation
import Observation

@MainActor
@Observable
final class ChatViewModel {
    enum TranscriptScrollBehavior {
        case animated
        case snap
    }

    enum StreamingPhase: Sendable, Equatable {
        case perceive
        case reason
        case act
        case other(String)

        init(rawValue: String) {
            switch rawValue.lowercased() {
            case "perceive":
                self = .perceive
            case "reason":
                self = .reason
            case "act":
                self = .act
            default:
                self = .other(rawValue)
            }
        }

        var composerLabel: String {
            switch self {
            case .perceive:
                return "Perceive"
            case .reason:
                return "Reason"
            case .act:
                return "Act"
            case .other(let rawValue):
                return rawValue.capitalized
            }
        }

        var streamingPlaceholder: String? {
            switch self {
            case .perceive:
                return "Preparing..."
            case .reason:
                return "Thinking..."
            case .act:
                return "Responding..."
            case .other:
                return nil
            }
        }
    }

    private static let sessionLoadDebounceMs = 50
    static let maxCachedSessions = 10

    var transcriptItems: [ChatTranscriptItem] = []
    var draftMessage = ""
    var queuedMessage: String?
    var isLoadingHistory = false
    var isStreaming = false
    var streamingText = ""
    var currentPhase: StreamingPhase?
    var errorMessage: String?
    var pendingTranscriptScrollBehavior: TranscriptScrollBehavior = .snap

    private let appState: AppState
    private let sessionViewModel: SessionViewModel
    private var currentSessionID: String?
    private var streamingSessionID: String?
    private var streamTask: Task<Void, Never>?
    private var retryRequest: RetryRequest?
    private var anonymousToolCallCounter = 0
    @ObservationIgnored private var historyLoadSequence = 0
    @ObservationIgnored private var transcriptCache: [String: [SessionMessage]] = [:]
    @ObservationIgnored private var transcriptCacheAccessOrder: [String] = []
    private var sessionLoadTask: Task<Void, Never>?

    init(appState: AppState, sessionViewModel: SessionViewModel) {
        self.appState = appState
        self.sessionViewModel = sessionViewModel
    }

    var activeStreamSessionID: String? {
        isStreaming ? streamingSessionID : nil
    }

    var isCurrentSessionStreaming: Bool {
        isStreaming && currentSessionID == streamingSessionID
    }

    var isStreamingInAnotherSession: Bool {
        isStreaming && currentSessionID != nil && currentSessionID != streamingSessionID
    }

    var visibleStreamingText: String {
        isCurrentSessionStreaming ? streamingText : ""
    }

    var visibleCurrentPhase: StreamingPhase? {
        isCurrentSessionStreaming ? currentPhase : nil
    }

    var composerPhaseLabel: String? {
        if let visibleCurrentPhase {
            return visibleCurrentPhase.composerLabel
        }

        if isStreamingInAnotherSession {
            return "Streaming in another session..."
        }

        return nil
    }

    var currentSessionTitle: String? {
        sessionViewModel.selectedSession?.displayTitle
    }

    var canRetryLastMessage: Bool {
        retryRequest != nil && !isStreaming
    }

    func invalidateSession(_ sessionID: String) {
        removeCachedMessages(for: sessionID)

        if currentSessionID == sessionID {
            transcriptItems = []
            pendingTranscriptScrollBehavior = .snap
        }
    }

    func prepareToDisplaySession(_ sessionID: String?) {
        errorMessage = nil
        pendingTranscriptScrollBehavior = .snap
        currentSessionID = sessionID
        anonymousToolCallCounter = 0

        guard let sessionID else {
            transcriptItems = []
            isLoadingHistory = false
            appState.clearContext()
            return
        }

        if let cachedMessages = cachedMessages(for: sessionID) {
            transcriptItems = makeTranscriptItems(from: cachedMessages)
            isLoadingHistory = false
        } else {
            transcriptItems = []
            isLoadingHistory = isStreaming && streamingSessionID == sessionID ? false : true
        }

        appState.clearContext()
    }

    func showEmptyState() {
        historyLoadSequence += 1
        if isStreaming {
            resetVisibleState()
            return
        }

        cleanup()
        retryRequest = nil
        resetVisibleState()
    }

    func scheduleLoadMessages(for sessionID: String?, force: Bool = false) {
        sessionLoadTask?.cancel()
        sessionLoadTask = Task { [weak self] in
            do {
                try await Task.sleep(for: .milliseconds(Self.sessionLoadDebounceMs))
            } catch is CancellationError {
                return
            } catch {
                return
            }

            guard !Task.isCancelled else {
                return
            }

            await self?.loadMessages(for: sessionID, force: force)
        }
    }

    func cancelScheduledLoad() {
        sessionLoadTask?.cancel()
        sessionLoadTask = nil
    }

    func loadMessages(for sessionID: String?, force: Bool = false) async {
        guard shouldLoadMessages(for: sessionID, force: force) else {
            return
        }

        if await handleActiveStreamingSessionIfNeeded(for: sessionID) {
            return
        }

        let loadSequence = beginLoad(for: sessionID)

        guard let sessionID else {
            clearLoadedSession()
            return
        }

        if let cachedTranscript = cachedTranscript(for: sessionID) {
            applyCachedTranscript(cachedTranscript)
        } else {
            showLoadingPlaceholder()
        }

        await fetchAndApplyMessages(for: sessionID, loadSequence: loadSequence)
    }

    private func shouldLoadMessages(for sessionID: String?, force: Bool) -> Bool {
        force || currentSessionID != sessionID
    }

    private func isActiveStreamingSession(_ sessionID: String?) -> Bool {
        isStreaming && streamingSessionID == sessionID
    }

    private func handleActiveStreamingSessionIfNeeded(for sessionID: String?) async -> Bool {
        guard isActiveStreamingSession(sessionID) else {
            return false
        }

        historyLoadSequence += 1
        isLoadingHistory = false
        currentSessionID = sessionID
        errorMessage = nil
        pendingTranscriptScrollBehavior = .snap

        if let sessionID, let cachedMessages = cachedTranscript(for: sessionID) {
            transcriptItems = makeTranscriptItems(from: cachedMessages)
            await appState.refreshContext(for: sessionID)
        }

        return true
    }

    private func beginLoad(for sessionID: String?) -> Int {
        historyLoadSequence += 1
        let loadSequence = historyLoadSequence

        if shouldPreserveBackgroundStream(for: sessionID) {
            currentSessionID = sessionID
            errorMessage = nil
            pendingTranscriptScrollBehavior = .snap
        } else {
            cleanup()
            currentSessionID = sessionID
            errorMessage = nil
            retryRequest = nil
        }

        return loadSequence
    }

    private func shouldPreserveBackgroundStream(for sessionID: String?) -> Bool {
        isStreaming && streamingSessionID != nil && streamingSessionID != sessionID
    }

    private func clearLoadedSession() {
        transcriptItems = []
        appState.clearContext()
    }

    private func cachedTranscript(for sessionID: String) -> [SessionMessage]? {
        cachedMessages(for: sessionID)
    }

    private func applyCachedTranscript(_ cachedMessages: [SessionMessage]) {
        pendingTranscriptScrollBehavior = .snap
        let cachedItems = makeTranscriptItems(from: cachedMessages)
        if transcriptItems != cachedItems {
            transcriptItems = cachedItems
        }
        isLoadingHistory = false
    }

    private func showLoadingPlaceholder() {
        pendingTranscriptScrollBehavior = .snap
        transcriptItems = []
        isLoadingHistory = true
    }

    private func fetchAndApplyMessages(for sessionID: String, loadSequence: Int) async {
        defer {
            if historyLoadSequence == loadSequence {
                isLoadingHistory = false
            }
        }

        do {
            let response = try await appState.client.sessionMessages(id: sessionID, limit: 200)
            guard historyLoadSequence == loadSequence, currentSessionID == sessionID else {
                return
            }
            applyFetchedMessages(response.messages, for: sessionID)
            await appState.refreshContext(for: sessionID)
        } catch is CancellationError {
            return
        } catch {
            await handleLoadMessagesError(error, sessionID: sessionID, loadSequence: loadSequence)
        }
    }

    private func applyFetchedMessages(_ messages: [SessionMessage], for sessionID: String) {
        cacheMessages(messages, for: sessionID)
        let updatedItems = makeTranscriptItems(from: messages)
        if transcriptItems != updatedItems {
            pendingTranscriptScrollBehavior = .snap
            transcriptItems = updatedItems
        }
    }

    private func handleLoadMessagesError(_ error: Error, sessionID: String, loadSequence: Int) async {
        guard historyLoadSequence == loadSequence else {
            return
        }

        if case APIError.httpStatus(let code, _) = error, code == 404 {
            await handleMissingSessionLoad(sessionID)
            return
        }

        removeCachedMessages(for: sessionID)
        transcriptItems = []
        errorMessage = error.localizedDescription
        pendingTranscriptScrollBehavior = .snap
        await appState.refreshContext(for: nil)
        await appState.noteRecoverableRequestFailure(error)
    }

    private func handleMissingSessionLoad(_ sessionID: String) async {
        removeCachedMessages(for: sessionID)
        sessionViewModel.removeSession(sessionID)
        currentSessionID = nil
        transcriptItems = []
        errorMessage = "Session no longer exists."
        pendingTranscriptScrollBehavior = .snap
        await appState.refreshContext(for: nil)
    }

    func sendDraft() {
        let trimmed = draftMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return
        }

        guard appState.connectionStatus == .connected else {
            errorMessage = "Reconnecting to Fawx. Try sending again once the connection is restored."
            return
        }

        draftMessage = ""

        if isStreaming {
            queuedMessage = trimmed
            return
        }

        Task {
            await send(trimmed)
        }
    }

    func retryLastMessage() {
        guard let retryRequest, !isStreaming else {
            return
        }

        self.retryRequest = nil
        Task {
            await send(retryRequest.text, forceSessionID: retryRequest.sessionID)
        }
    }

    func dismissQueuedMessage() {
        queuedMessage = nil
    }

    func stopStreaming() {
        streamTask?.cancel()
        streamTask = nil
    }

    func cleanup() {
        stopStreaming()
        resetStreamingState()
        queuedMessage = nil
    }

    private func send(_ text: String, forceSessionID: String? = nil) async {
        errorMessage = nil
        currentPhase = nil
        var targetSessionID = forceSessionID ?? currentSessionID

        if targetSessionID == nil {
            do {
                let createdSession = try await appState.client.createSession(model: appState.activeModel?.modelID)
                sessionViewModel.upsert(createdSession)
                sessionViewModel.select(createdSession.id)
                currentSessionID = createdSession.id
                targetSessionID = createdSession.id
            } catch {
                draftMessage = text
                errorMessage = "Failed to create session. \(error.localizedDescription)"
                return
            }
        }

        guard let sessionID = targetSessionID else {
            errorMessage = "No session available."
            return
        }

        let timestamp = Int(Date().timeIntervalSince1970)
        let userMessage = SessionMessage(role: .user, content: text, timestamp: timestamp)
        appendMessage(userMessage, for: sessionID)
        sessionViewModel.updatePreview(for: sessionID, text: text, model: appState.activeModel?.modelID)
        retryRequest = RetryRequest(text: text, sessionID: sessionID)

        do {
            let stream = try await appState.client.sendMessageStream(
                sessionID: sessionID,
                message: text
            )
            startStreaming(stream, sessionID: sessionID, retryText: text)
        } catch {
            errorMessage = "Failed to send message. \(error.localizedDescription)"
        }
    }

    private func startStreaming(
        _ stream: AsyncThrowingStream<SSEEvent, Error>,
        sessionID: String,
        retryText: String
    ) {
        stopStreaming()
        isStreaming = true
        streamingSessionID = sessionID
        streamingText = ""
        currentPhase = nil

        let assistantTimestamp = Int(Date().timeIntervalSince1970)
        streamTask = Task {
            var finalResponse: String?
            var streamFailed = false

            do {
                for try await event in stream {
                    switch event {
                    case .textDelta(let text):
                        streamingText += text
                    case .toolCallStart(let id, let name):
                        if currentSessionID == sessionID {
                            beginToolCall(id: id, name: name)
                        }
                    case .toolCallDelta(let id, let argumentsDelta):
                        if currentSessionID == sessionID {
                            updateToolCall(id: id) { toolCall in
                                toolCall.arguments += argumentsDelta
                            }
                        }
                    case .toolCallComplete(let id, let name, let arguments):
                        if currentSessionID == sessionID {
                            completeToolCall(id: id, name: name, arguments: arguments)
                        }
                    case .toolResult(let id, let output, let isError):
                        if currentSessionID == sessionID {
                            finishToolCall(id: id, output: output, isError: isError)
                        }
                    case .phase(let phase):
                        currentPhase = StreamingPhase(rawValue: phase)
                    case .done(let response):
                        finalResponse = response
                    case .engineError(_, let message, let recoverable):
                        if !recoverable {
                            handleStreamError(message, sessionID: sessionID)
                            streamFailed = true
                        }
                    case .error(let message):
                        handleStreamError(message, sessionID: sessionID)
                        streamFailed = true
                    }
                }

                if streamFailed && finalResponse == nil {
                    let recovered = await recoverInterruptedStream(
                        sessionID: sessionID,
                        retryText: retryText
                    )
                    if !recovered {
                        retryRequest = RetryRequest(text: retryText, sessionID: sessionID)
                        await finalizeCancellation(timestamp: assistantTimestamp, sessionID: sessionID)
                    }
                } else {
                    await finalizeStream(
                        timestamp: assistantTimestamp,
                        finalResponse: finalResponse,
                        sessionID: sessionID
                    )
                }
            } catch is CancellationError {
                await finalizeCancellation(timestamp: assistantTimestamp, sessionID: sessionID)
            } catch {
                handleStreamError("Response interrupted. \(error.localizedDescription)", sessionID: sessionID)
                let recovered = await recoverInterruptedStream(
                    sessionID: sessionID,
                    retryText: retryText
                )
                if !recovered {
                    retryRequest = RetryRequest(text: retryText, sessionID: sessionID)
                    await finalizeCancellation(timestamp: assistantTimestamp, sessionID: sessionID)
                }
            }

            streamTask = nil
        }
    }

    private func finalizeStream(timestamp: Int, finalResponse: String?, sessionID: String) async {
        let content = finalResponse ?? streamingText
        if !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let assistantMessage = SessionMessage(role: .assistant, content: content, timestamp: timestamp)
            appendMessage(assistantMessage, for: sessionID)
            sessionViewModel.updatePreview(for: sessionID, text: content, model: appState.activeModel?.modelID)
        }

        retryRequest = nil
        resetStreamingState()

        if currentSessionID == sessionID {
            await appState.refreshContext(for: sessionID)
        }
        await sessionViewModel.refresh()
        await sendQueuedMessageIfNeeded()
    }

    private func finalizeCancellation(timestamp: Int, sessionID: String) async {
        if !streamingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let interrupted = streamingText + "\n\n(interrupted)"
            let assistantMessage = SessionMessage(role: .assistant, content: interrupted, timestamp: timestamp)
            appendMessage(assistantMessage, for: sessionID)
            sessionViewModel.updatePreview(for: sessionID, text: interrupted, model: appState.activeModel?.modelID)
        }

        resetStreamingState()
        if currentSessionID == sessionID {
            await appState.refreshContext(for: sessionID)
        }
    }

    private func recoverInterruptedStream(sessionID: String, retryText: String) async -> Bool {
        do {
            let response = try await appState.client.sessionMessages(id: sessionID, limit: 200)
            guard
                let lastUserIndex = response.messages.lastIndex(where: { message in
                    message.role == .user && message.content == retryText
                }),
                response.messages.indices.contains(response.messages.index(after: lastUserIndex))
            else {
                return false
            }

            cacheMessages(response.messages, for: sessionID)
            if currentSessionID == sessionID {
                transcriptItems = makeTranscriptItems(from: response.messages)
            }
            retryRequest = nil
            resetStreamingState()
            if currentSessionID == sessionID {
                await appState.refreshContext(for: sessionID)
            }
            await sessionViewModel.refresh()
            await sendQueuedMessageIfNeeded()
            return true
        } catch {
            await appState.noteRecoverableRequestFailure(error)
            return false
        }
    }

    private func sendQueuedMessageIfNeeded() async {
        guard let queued = queuedMessage?.trimmingCharacters(in: .whitespacesAndNewlines), !queued.isEmpty else {
            queuedMessage = nil
            return
        }
        guard appState.connectionStatus == .connected else {
            return
        }

        queuedMessage = nil
        await send(queued)
    }

    private func resetStreamingState() {
        streamingText = ""
        isStreaming = false
        streamingSessionID = nil
        currentPhase = nil
        anonymousToolCallCounter = 0
    }

    private func appendMessage(_ message: SessionMessage, for sessionID: String) {
        let existingMessages: [SessionMessage]
        if let cachedMessages = cachedMessages(for: sessionID) {
            existingMessages = cachedMessages
        } else if currentSessionID == sessionID {
            existingMessages = transcriptItems.compactMap(\.sessionMessage)
        } else {
            existingMessages = []
        }

        let updatedMessages = existingMessages + [message]
        cacheMessages(updatedMessages, for: sessionID)
        if currentSessionID == sessionID {
            pendingTranscriptScrollBehavior = .animated
            transcriptItems = makeTranscriptItems(from: updatedMessages)
        }
    }

    private func handleStreamError(_ message: String, sessionID: String) {
        if currentSessionID == sessionID {
            errorMessage = message
        }
    }

    func makeTranscriptItems(from messages: [SessionMessage]) -> [ChatTranscriptItem] {
        var duplicateCounts: [String: Int] = [:]

        return messages.map { message in
            let baseID = messageStableIDBase(for: message)
            let occurrence = duplicateCounts[baseID, default: 0]
            duplicateCounts[baseID] = occurrence + 1
            let id = occurrence == 0 ? baseID : "\(baseID)#\(occurrence)"
            return .message(TranscriptMessage(id: id, message: message))
        }
    }

    private func messageStableIDBase(for message: SessionMessage) -> String {
        [
            message.role.rawValue,
            String(message.timestamp),
            Self.stableDigest(for: message.content)
        ].joined(separator: ":")
    }

    static func stableDigest(for content: String) -> String {
        var hash: UInt64 = 14_695_981_039_346_656_037

        for byte in content.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1_099_511_628_211
        }

        return String(hash, radix: 16)
    }

    func cacheMessages(_ messages: [SessionMessage], for sessionID: String) {
        transcriptCache[sessionID] = messages
        touchCachedSession(sessionID)
        trimTranscriptCacheIfNeeded()
    }

    func cachedMessages(for sessionID: String) -> [SessionMessage]? {
        guard let messages = transcriptCache[sessionID] else {
            return nil
        }

        touchCachedSession(sessionID)
        return messages
    }

    private func removeCachedMessages(for sessionID: String) {
        transcriptCache.removeValue(forKey: sessionID)
        transcriptCacheAccessOrder.removeAll { $0 == sessionID }
    }

    private func touchCachedSession(_ sessionID: String) {
        transcriptCacheAccessOrder.removeAll { $0 == sessionID }
        transcriptCacheAccessOrder.append(sessionID)
    }

    private func trimTranscriptCacheIfNeeded() {
        let protectedSessions = Set([currentSessionID, streamingSessionID].compactMap { $0 })
        var scannedEntries = 0

        while transcriptCache.count > Self.maxCachedSessions, scannedEntries < transcriptCacheAccessOrder.count {
            let leastRecentSessionID = transcriptCacheAccessOrder.removeFirst()
            if protectedSessions.contains(leastRecentSessionID) {
                transcriptCacheAccessOrder.append(leastRecentSessionID)
                scannedEntries += 1
                continue
            }

            transcriptCache.removeValue(forKey: leastRecentSessionID)
            scannedEntries = 0
        }

        while transcriptCache.count > Self.maxCachedSessions, let leastRecentSessionID = transcriptCacheAccessOrder.first {
            transcriptCacheAccessOrder.removeFirst()
            transcriptCache.removeValue(forKey: leastRecentSessionID)
        }
    }

    private func resetVisibleState() {
        isLoadingHistory = false
        currentSessionID = nil
        transcriptItems = []
        errorMessage = nil
        pendingTranscriptScrollBehavior = .snap
        appState.clearContext()
    }

    private func beginToolCall(id: String?, name: String?) {
        let toolCallID = stableToolCallID(for: id)

        if let index = transcriptItems.firstIndex(where: { item in
            if case .toolCall(let toolCall) = item {
                return toolCall.id == toolCallID
            }
            return false
        }) {
            updateTranscriptItem(at: index) { toolCall in
                toolCall.name = name ?? toolCall.name
                toolCall.isRunning = true
            }
            return
        }

        transcriptItems.append(.toolCall(
            ToolCallRecord(
                id: toolCallID,
                name: name ?? "tool",
                arguments: "",
                result: nil,
                isRunning: true,
                isError: false
            )
        ))
    }

    private func completeToolCall(id: String?, name: String?, arguments: String) {
        updateToolCall(id: id) { toolCall in
            if let name {
                toolCall.name = name
            }
            toolCall.arguments = arguments
        }
    }

    private func finishToolCall(id: String?, output: String, isError: Bool) {
        updateToolCall(id: id) { toolCall in
            toolCall.result = output
            toolCall.isRunning = false
            toolCall.isError = isError
        }
    }

    private func updateToolCall(id: String?, update: (inout ToolCallRecord) -> Void) {
        let toolCallID = stableToolCallID(for: id)

        if let index = transcriptItems.firstIndex(where: { item in
            if case .toolCall(let toolCall) = item {
                return toolCall.id == toolCallID
            }
            return false
        }) {
            updateTranscriptItem(at: index, update: update)
        } else {
            var toolCall = ToolCallRecord(
                id: toolCallID,
                name: "tool",
                arguments: "",
                result: nil,
                isRunning: true,
                isError: false
            )
            update(&toolCall)
            transcriptItems.append(.toolCall(toolCall))
        }
    }

    private func updateTranscriptItem(
        at index: Int,
        update: (inout ToolCallRecord) -> Void
    ) {
        guard case .toolCall(var toolCall) = transcriptItems[index] else {
            return
        }
        update(&toolCall)
        transcriptItems[index] = .toolCall(toolCall)
    }

    private func stableToolCallID(for rawID: String?) -> String {
        if let rawID, !rawID.isEmpty {
            return rawID
        }

        anonymousToolCallCounter += 1
        return "tool-\(anonymousToolCallCounter)"
    }
}

private struct RetryRequest {
    let text: String
    let sessionID: String
}

#if DEBUG
extension ChatViewModel {
    func appendMessageForTesting(_ message: SessionMessage, sessionID: String) {
        appendMessage(message, for: sessionID)
    }

    func handleStreamErrorForTesting(_ message: String, sessionID: String) {
        handleStreamError(message, sessionID: sessionID)
    }

    func setStreamingStateForTesting(
        isStreaming: Bool,
        currentSessionID: String?,
        streamingSessionID: String?,
        streamingText: String = "",
        phase: StreamingPhase? = nil
    ) {
        self.isStreaming = isStreaming
        self.currentSessionID = currentSessionID
        self.streamingSessionID = streamingSessionID
        self.streamingText = streamingText
        self.currentPhase = phase
    }
}
#endif
