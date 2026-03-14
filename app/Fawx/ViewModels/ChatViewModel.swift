import Foundation
import Observation

@MainActor
@Observable
final class ChatViewModel {
    var transcriptItems: [ChatTranscriptItem] = []
    var draftMessage = ""
    var queuedMessage: String?
    var isLoadingHistory = false
    var isStreaming = false
    var streamingText = ""
    var currentPhase: String?
    var errorMessage: String?

    private let appState: AppState
    private let sessionViewModel: SessionViewModel
    private var currentSessionID: String?
    private var streamTask: Task<Void, Never>?
    private var retryRequest: RetryRequest?
    private var anonymousToolCallCounter = 0

    init(appState: AppState, sessionViewModel: SessionViewModel) {
        self.appState = appState
        self.sessionViewModel = sessionViewModel
    }

    var activeStreamSessionID: String? {
        isStreaming ? currentSessionID : nil
    }

    var currentSessionTitle: String? {
        sessionViewModel.selectedSession?.displayTitle
    }

    var canRetryLastMessage: Bool {
        retryRequest != nil && !isStreaming
    }

    func showEmptyState() {
        cleanup()
        currentSessionID = nil
        transcriptItems = []
        errorMessage = nil
        retryRequest = nil
        appState.clearContext()
    }

    func loadMessages(for sessionID: String?, force: Bool = false) async {
        guard force || currentSessionID != sessionID else {
            return
        }

        cleanup()
        currentSessionID = sessionID
        errorMessage = nil
        retryRequest = nil

        guard let sessionID else {
            transcriptItems = []
            appState.clearContext()
            return
        }

        isLoadingHistory = true
        defer { isLoadingHistory = false }

        do {
            let response = try await appState.client.sessionMessages(id: sessionID, limit: 200)
            transcriptItems = response.messages.map(ChatTranscriptItem.message)
            await appState.refreshContext(for: sessionID)
            try? await appState.refreshServerState()
        } catch {
            if case APIError.httpStatus(let code, _) = error, code == 404 {
                sessionViewModel.removeSession(sessionID)
                currentSessionID = nil
                transcriptItems = []
                errorMessage = "Session no longer exists."
                await appState.refreshContext(for: nil)
                return
            }

            transcriptItems = []
            errorMessage = error.localizedDescription
            await appState.refreshContext(for: nil)
            await appState.noteRecoverableRequestFailure(error)
        }
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
        isStreaming = false
        streamingText = ""
        currentPhase = nil
        queuedMessage = nil
        anonymousToolCallCounter = 0
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
        transcriptItems.append(.message(userMessage))
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
                        beginToolCall(id: id, name: name)
                    case .toolCallDelta(let id, let argumentsDelta):
                        updateToolCall(id: id) { toolCall in
                            toolCall.arguments += argumentsDelta
                        }
                    case .toolCallComplete(let id, let name, let arguments):
                        completeToolCall(id: id, name: name, arguments: arguments)
                    case .toolResult(let id, let output, let isError):
                        finishToolCall(id: id, output: output, isError: isError)
                    case .phase(let phase):
                        currentPhase = phase.capitalized
                    case .done(let response):
                        finalResponse = response
                    case .engineError(_, let message, let recoverable):
                        if !recoverable {
                            errorMessage = message
                            streamFailed = true
                        }
                    case .error(let message):
                        errorMessage = message
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
                errorMessage = "Response interrupted. \(error.localizedDescription)"
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
        guard currentSessionID == sessionID else {
            resetStreamingState()
            return
        }

        let content = finalResponse ?? streamingText
        if !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let assistantMessage = SessionMessage(role: .assistant, content: content, timestamp: timestamp)
            transcriptItems.append(.message(assistantMessage))
            sessionViewModel.updatePreview(for: sessionID, text: content, model: appState.activeModel?.modelID)
        }

        retryRequest = nil
        resetStreamingState()

        try? await appState.refreshServerState()
        await appState.refreshContext(for: sessionID)
        await sessionViewModel.refresh()
        await sendQueuedMessageIfNeeded()
    }

    private func finalizeCancellation(timestamp: Int, sessionID: String) async {
        guard currentSessionID == sessionID else {
            resetStreamingState()
            return
        }

        if !streamingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let interrupted = streamingText + "\n\n(interrupted)"
            let assistantMessage = SessionMessage(role: .assistant, content: interrupted, timestamp: timestamp)
            transcriptItems.append(.message(assistantMessage))
            sessionViewModel.updatePreview(for: sessionID, text: interrupted, model: appState.activeModel?.modelID)
        }

        resetStreamingState()
        await appState.refreshContext(for: sessionID)
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

            transcriptItems = response.messages.map(ChatTranscriptItem.message)
            retryRequest = nil
            resetStreamingState()
            await appState.refreshContext(for: sessionID)
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
        currentPhase = nil
        anonymousToolCallCounter = 0
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
