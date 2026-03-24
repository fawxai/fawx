import CoreGraphics
import Foundation
import ImageIO
import Observation
import PDFKit
import UniformTypeIdentifiers

enum PendingAttachmentKind: String, Sendable, Hashable {
    case image
    case textFile
    case pdf
}

struct PendingAttachment: Identifiable, Sendable, Hashable {
    let id: UUID
    let kind: PendingAttachmentKind
    let filename: String
    let data: Data
    let mediaType: String
    let textContent: String?

    init(
        id: UUID = UUID(),
        kind: PendingAttachmentKind,
        filename: String,
        data: Data,
        mediaType: String,
        textContent: String? = nil
    ) {
        self.id = id
        self.kind = kind
        self.filename = filename
        self.data = data
        self.mediaType = mediaType
        self.textContent = textContent
    }
}

struct PreparedMessagePayload: Sendable, Hashable {
    let message: String
    let images: [ImagePayload]
    let documents: [DocumentPayload]
    let contentBlocks: [SessionContentBlock]
}

private struct QueuedDraft: Sendable, Hashable {
    let text: String
    let attachments: [PendingAttachment]

    var summaryText: String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }

        if attachments.count == 1 {
            return "Queued \(attachments[0].filename)"
        }

        return "Queued \(attachments.count) attachments"
    }
}

enum AttachmentComposerError: LocalizedError, Equatable {
    case tooManyAttachments(limit: Int)
    case unsupportedFileType(String)
    case unsupportedImageType(String)
    case invalidImageData
    case imageStillTooLarge
    case textFileTooLarge(String)
    case binaryTextFile(String)
    case pdfTooLarge(String)
    case pdfTooManyPages(String, Int)

    var errorDescription: String? {
        switch self {
        case .tooManyAttachments(let limit):
            return "You can attach up to \(limit) items per message."
        case .unsupportedFileType(let filename):
            return "Unsupported attachment type: \(filename)."
        case .unsupportedImageType(let mediaType):
            return "Unsupported image type: \(mediaType)."
        case .invalidImageData:
            return "That image couldn't be read."
        case .imageStillTooLarge:
            return "That image is still too large after resizing."
        case .textFileTooLarge(let filename):
            return "\(filename) is larger than 500KB."
        case .binaryTextFile(let filename):
            return "\(filename) isn't valid UTF-8 text."
        case .pdfTooLarge(let filename):
            return "\(filename) is larger than 10MB."
        case .pdfTooManyPages(let filename, let pageCount):
            return "\(filename) has \(pageCount) pages. PDFs are limited to 100 pages."
        }
    }
}

enum AttachmentComposer {
    static let maxAttachmentCount = 10
    static let maxImageBytes = 5 * 1024 * 1024
    static let maxTextBytes = 500 * 1024
    static let maxPDFBytes = 10 * 1024 * 1024
    static let maxPDFPages = 100

    private static let supportedImageMediaTypes: Set<String> = [
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
    ]

    private static let supportedImageFileExtensions: [String] = [
        "jpg", "jpeg", "png", "gif", "webp",
    ]

    private static let supportedTextFileExtensions: [String] = [
        "txt", "md", "csv", "json", "xml", "yaml", "yml",
        "py", "rs", "swift", "kt", "js", "ts", "html", "htm",
        "css", "sh", "toml", "tsv", "log",
    ]

    private static let supportedTextExtensions = Set(supportedTextFileExtensions)

    static let supportedPickerContentTypes: [UTType] = {
        let supportedFileExtensions =
            supportedImageFileExtensions + ["pdf"] + supportedTextFileExtensions
        var types: [UTType] = []
        for fileExtension in supportedFileExtensions {
            guard
                let type = UTType(filenameExtension: fileExtension),
                !types.contains(type)
            else {
                continue
            }
            types.append(type)
        }
        return types
    }()

    static func append(
        _ newAttachments: [PendingAttachment],
        to existingAttachments: [PendingAttachment]
    ) throws -> [PendingAttachment] {
        guard existingAttachments.count + newAttachments.count <= maxAttachmentCount else {
            throw AttachmentComposerError.tooManyAttachments(limit: maxAttachmentCount)
        }

        return existingAttachments + newAttachments
    }

    static func removeAttachment(id: UUID, from attachments: [PendingAttachment]) -> [PendingAttachment] {
        attachments.filter { $0.id != id }
    }

    static func pendingAttachment(fromFileURL url: URL) throws -> PendingAttachment {
        let filename = normalizedFilename(url.lastPathComponent, fallback: "Attachment")
        let data = try Data(contentsOf: url)
        let extensionType = url.pathExtension.lowercased()
        let type = UTType(filenameExtension: extensionType)
        let mediaType = type?.preferredMIMEType

        if extensionType == "pdf" || mediaType == "application/pdf" {
            return try pdfAttachment(data: data, filename: filename)
        }

        if isSupportedTextExtension(extensionType) {
            return try textFileAttachment(
                data: data,
                filename: filename,
                mediaType: mediaType ?? "text/plain"
            )
        }

        if let imageMediaType = detectedImageMediaType(data: data) ?? normalizedImageMediaType(
            preferredMIMEType: mediaType,
            fallbackFilename: filename
        ) {
            return try imageAttachment(data: data, filename: filename, mediaType: imageMediaType)
        }

        throw AttachmentComposerError.unsupportedFileType(filename)
    }

    static func pastedImageAttachment(data: Data) throws -> PendingAttachment {
        try imageAttachment(
            data: data,
            filename: "Pasted Image.png",
            mediaType: "image/png"
        )
    }

    static func imageAttachment(data: Data, filename: String, mediaType: String) throws -> PendingAttachment {
        let resolvedMediaType = try resolvedImageMediaType(data: data, fallbackMediaType: mediaType)

        let prepared = try normalizedImageData(data: data, originalMediaType: resolvedMediaType)
        return PendingAttachment(
            kind: .image,
            filename: filename,
            data: prepared.data,
            mediaType: prepared.mediaType
        )
    }

    static func textFileAttachment(
        data: Data,
        filename: String,
        mediaType: String
    ) throws -> PendingAttachment {
        guard data.count <= maxTextBytes else {
            throw AttachmentComposerError.textFileTooLarge(filename)
        }

        guard let text = String(data: data, encoding: .utf8) else {
            throw AttachmentComposerError.binaryTextFile(filename)
        }

        return PendingAttachment(
            kind: .textFile,
            filename: filename,
            data: data,
            mediaType: mediaType,
            textContent: text
        )
    }

    static func pdfAttachment(data: Data, filename: String) throws -> PendingAttachment {
        guard data.count <= maxPDFBytes else {
            throw AttachmentComposerError.pdfTooLarge(filename)
        }

        if let document = PDFDocument(data: data), document.pageCount > maxPDFPages {
            throw AttachmentComposerError.pdfTooManyPages(filename, document.pageCount)
        }

        return PendingAttachment(
            kind: .pdf,
            filename: filename,
            data: data,
            mediaType: "application/pdf"
        )
    }

    static func prepareMessage(
        message: String,
        attachments: [PendingAttachment]
    ) -> PreparedMessagePayload {
        let textFiles = attachments.filter { $0.kind == .textFile }
        let images = attachments.filter { $0.kind == .image }.map {
            ImagePayload(data: $0.data.base64EncodedString(), mediaType: $0.mediaType)
        }
        let documents = attachments.filter { $0.kind == .pdf }.map {
            DocumentPayload(
                data: $0.data.base64EncodedString(),
                mediaType: $0.mediaType,
                filename: $0.filename
            )
        }
        let composedMessage = injectedText(message: message, textAttachments: textFiles)
        let contentBlocks = makeContentBlocks(
            message: composedMessage,
            attachments: attachments
        )

        return PreparedMessagePayload(
            message: composedMessage,
            images: images,
            documents: documents,
            contentBlocks: contentBlocks
        )
    }

    private static func injectedText(
        message: String,
        textAttachments: [PendingAttachment]
    ) -> String {
        let trimmedMessage = message.trimmingCharacters(in: .whitespacesAndNewlines)
        let fileBlocks = textAttachments.compactMap { attachment -> String? in
            guard let text = attachment.textContent else {
                return nil
            }

            return """
            [file: \(attachment.filename)]
            \(text)
            [/file: \(attachment.filename)]
            """
        }

        var sections = fileBlocks
        if !trimmedMessage.isEmpty {
            sections.append(trimmedMessage)
        }

        return sections.joined(separator: "\n\n")
    }

    private static func makeContentBlocks(
        message: String,
        attachments: [PendingAttachment]
    ) -> [SessionContentBlock] {
        var blocks: [SessionContentBlock] = attachments.compactMap { attachment in
            switch attachment.kind {
            case .image:
                return .image(
                    mediaType: attachment.mediaType,
                    data: attachment.data.base64EncodedString()
                )
            case .pdf:
                return .document(
                    mediaType: attachment.mediaType,
                    data: attachment.data.base64EncodedString(),
                    filename: attachment.filename
                )
            case .textFile:
                return nil
            }
        }

        if !message.isEmpty {
            blocks.append(.text(message))
        }

        return blocks
    }

    private static func normalizedImageData(
        data: Data,
        originalMediaType: String
    ) throws -> (data: Data, mediaType: String) {
        guard data.count > maxImageBytes else {
            return (data, originalMediaType)
        }

        guard
            let source = CGImageSourceCreateWithData(data as CFData, nil),
            let resizedImage = CGImageSourceCreateThumbnailAtIndex(
                source,
                0,
                [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: 2048,
                ] as CFDictionary
            ),
            let jpegData = jpegData(from: resizedImage, quality: 0.8)
        else {
            throw AttachmentComposerError.invalidImageData
        }

        guard jpegData.count <= maxImageBytes else {
            throw AttachmentComposerError.imageStillTooLarge
        }

        return (jpegData, "image/jpeg")
    }

    private static func jpegData(from image: CGImage, quality: CGFloat) -> Data? {
        let output = NSMutableData()
        guard
            let destination = CGImageDestinationCreateWithData(
                output,
                UTType.jpeg.identifier as CFString,
                1,
                nil
            )
        else {
            return nil
        }

        CGImageDestinationAddImage(
            destination,
            image,
            [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
        )

        guard CGImageDestinationFinalize(destination) else {
            return nil
        }

        return output as Data
    }

    private static func resolvedImageMediaType(
        data: Data,
        fallbackMediaType: String
    ) throws -> String {
        if let detectedMediaType = detectedImageMediaType(data: data) {
            guard supportedImageMediaTypes.contains(detectedMediaType) else {
                throw AttachmentComposerError.unsupportedImageType(detectedMediaType)
            }
            return detectedMediaType
        }

        guard supportedImageMediaTypes.contains(fallbackMediaType) else {
            throw AttachmentComposerError.unsupportedImageType(fallbackMediaType)
        }

        return fallbackMediaType
    }

    private static func detectedImageMediaType(data: Data) -> String? {
        if data.starts(with: [0xFF, 0xD8, 0xFF]) {
            return "image/jpeg"
        }

        if data.starts(with: [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return "image/png"
        }

        if data.starts(with: Data("GIF87a".utf8)) || data.starts(with: Data("GIF89a".utf8)) {
            return "image/gif"
        }

        if data.count >= 12,
           data[0...3] == Data("RIFF".utf8),
           data[8...11] == Data("WEBP".utf8) {
            return "image/webp"
        }

        guard
            let source = CGImageSourceCreateWithData(data as CFData, nil),
            let typeIdentifier = CGImageSourceGetType(source) as String?,
            let detectedMediaType = UTType(typeIdentifier)?.preferredMIMEType
        else {
            return nil
        }

        return detectedMediaType
    }

    private static func normalizedImageMediaType(
        preferredMIMEType: String?,
        fallbackFilename: String
    ) -> String? {
        if let preferredMIMEType, supportedImageMediaTypes.contains(preferredMIMEType) {
            return preferredMIMEType
        }

        let extensionType = URL(fileURLWithPath: fallbackFilename).pathExtension.lowercased()
        switch extensionType {
        case "jpg", "jpeg": return "image/jpeg"
        case "png": return "image/png"
        case "gif": return "image/gif"
        case "webp": return "image/webp"
        default: return nil
        }
    }

    private static func isSupportedTextExtension(_ extensionType: String) -> Bool {
        supportedTextExtensions.contains(extensionType)
    }

    private static func normalizedFilename(_ filename: String, fallback: String) -> String {
        let trimmed = filename.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? fallback : trimmed
    }
}

@MainActor
final class StreamingDisplayController {
    static let flushInterval: Duration = .milliseconds(50)
    static let bottomThreshold: CGFloat = 50
    private static let contentGrowthDetachAllowanceMultiplier: CGFloat = 1.25

    private let flushHandler: (String) -> Void
    private let flushIntervalDuration: Duration
    private let sleepHandler: @Sendable (Duration) async throws -> Void
    private var pendingTokens = ""
    private var renderTimer: Task<Void, Never>?
    private var pendingAutoScroll = false
    private var lastDistanceFromBottom: CGFloat = 0

    private(set) var isPinnedToBottom = true

    init(
        flushInterval: Duration = StreamingDisplayController.flushInterval,
        sleepHandler: @escaping @Sendable (Duration) async throws -> Void = { duration in
            try await Task.sleep(for: duration)
        },
        flushHandler: @escaping (String) -> Void
    ) {
        self.flushHandler = flushHandler
        self.flushIntervalDuration = flushInterval
        self.sleepHandler = sleepHandler
    }

    func appendToken(_ token: String) {
        guard !token.isEmpty else {
            return
        }

        pendingTokens += token
        startRenderTimerIfNeeded()
    }

    func streamDidEnd() {
        flushPendingTokens()
        stopRenderTimer()
    }

    func reset(repinToBottom: Bool = true) {
        stopRenderTimer()
        pendingTokens = ""
        pendingAutoScroll = false
        if repinToBottom {
            isPinnedToBottom = true
            lastDistanceFromBottom = 0
        }
    }

    func userDidScroll(
        distanceFromBottom: CGFloat,
        isUserInitiated: Bool = true,
        threshold: CGFloat = StreamingDisplayController.bottomThreshold
    ) {
        let clampedDistance = max(0, distanceFromBottom)
        if clampedDistance <= threshold {
            isPinnedToBottom = true
            pendingAutoScroll = false
            lastDistanceFromBottom = clampedDistance
            return
        }

        if !isUserInitiated {
            lastDistanceFromBottom = clampedDistance
            return
        }

        if pendingAutoScroll {
            pendingAutoScroll = false

            // Ignore one small jump caused by content growth before our scheduled
            // scroll catches up, but still allow a real user scroll-away to detach.
            let contentGrowthDetachAllowance = threshold * Self.contentGrowthDetachAllowanceMultiplier
            if clampedDistance - lastDistanceFromBottom <= contentGrowthDetachAllowance {
                lastDistanceFromBottom = clampedDistance
                return
            }
        }

        isPinnedToBottom = false
        lastDistanceFromBottom = clampedDistance
    }

    func setPinnedToBottom(_ pinnedToBottom: Bool, distanceFromBottom: CGFloat) {
        let clampedDistance = max(0, distanceFromBottom)
        isPinnedToBottom = pinnedToBottom
        lastDistanceFromBottom = clampedDistance
        if pinnedToBottom {
            pendingAutoScroll = false
        }
    }

    private func startRenderTimerIfNeeded() {
        guard renderTimer == nil else {
            return
        }

        renderTimer = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else {
                    return
                }

                do {
                    try await self.sleepHandler(self.flushIntervalDuration)
                } catch is CancellationError {
                    return
                } catch {
                    return
                }

                self.handleRenderTimerTick()
            }
        }
    }

    private func handleRenderTimerTick() {
        flushPendingTokens()

        if pendingTokens.isEmpty {
            stopRenderTimer()
        }
    }

    private func flushPendingTokens() {
        guard !pendingTokens.isEmpty else {
            return
        }

        let flushedTokens = pendingTokens
        pendingTokens = ""
        if isPinnedToBottom {
            pendingAutoScroll = true
        }
        flushHandler(flushedTokens)
    }

    private func stopRenderTimer() {
        renderTimer?.cancel()
        renderTimer = nil
    }
}

@MainActor
@Observable
final class ChatViewModel {
    enum TranscriptScrollBehavior {
        case animated
        case snap
        case preservePosition
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

    struct CompactionBannerInfo: Equatable {
        let message: String
        let isEmergency: Bool
    }

    private struct SessionStreamingState {
        var text = ""
        var phase: StreamingPhase?
    }

    private static let sessionLoadDebounceMs = 50
    static let permissionPromptTimeoutSeconds = 60
    private static let permissionPromptTimeout: Duration = .seconds(permissionPromptTimeoutSeconds)
    private static let compactionBannerTimeout: Duration = .seconds(4)
    static let maxCachedSessions = 10

    private(set) var transcriptUpdateID = 0
    var transcriptItems: [ChatTranscriptItem] = [] {
        didSet {
            if oldValue != transcriptItems {
                transcriptUpdateID &+= 1
            }
        }
    }
    private var draftsBySession: [String: String] = [:]
    private var pendingAttachmentsBySession: [String: [PendingAttachment]] = [:]
    private var queuedDraftsBySession: [String: QueuedDraft] = [:]
    var isLoadingHistory = false
    var isStreaming: Bool {
        !streamStates.isEmpty
    }
    var errorMessage: String? {
        get { errorMessagesBySession[errorStorageKey(for: currentSessionID)] }
        set { setErrorMessage(newValue, for: currentSessionID) }
    }
    var activePermissionPrompt: PermissionPrompt?
    var isRespondingToPermissionPrompt = false
    var permissionPromptErrorMessage: String?
    var pendingTranscriptScrollBehavior: TranscriptScrollBehavior = .snap
    var compactionBannerInfo: CompactionBannerInfo? {
        guard let currentSessionID else {
            return nil
        }

        return compactionBannerInfosBySession[currentSessionID]
    }

    private let appState: AppState
    private let sessionViewModel: SessionViewModel
    private let compactionBannerSleepHandler: @Sendable (Duration) async throws -> Void
    private var currentSessionID: String?
    private var errorMessagesBySession: [String: String] = [:]
    private var retryRequestsBySession: [String: RetryRequest] = [:]
    private var liveToolGroupsBySession: [String: ToolActivityGroupRecord] = [:]
    private var anonymousToolCallCountersBySession: [String: Int] = [:]
    private var queuedPermissionPrompts: [PermissionPrompt] = []
    private var streamStates: [String: SessionStreamingState] = [:]
    private var compactionBannerInfosBySession: [String: CompactionBannerInfo] = [:]
    @ObservationIgnored private var historyLoadSequence = 0
    @ObservationIgnored private var transcriptCache: [String: [SessionMessage]] = [:]
    @ObservationIgnored private var transcriptCacheAccessOrder: [String] = []
    @ObservationIgnored private var permissionPromptTimeoutTask: Task<Void, Never>?
    @ObservationIgnored private var streamTasks: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var streamingDisplayControllers: [String: StreamingDisplayController] = [:]
    @ObservationIgnored private var compactionBannerDismissTasks: [String: Task<Void, Never>] = [:]
    private var sessionLoadTask: Task<Void, Never>?

    init(
        appState: AppState,
        sessionViewModel: SessionViewModel,
        compactionBannerSleepHandler: @escaping @Sendable (Duration) async throws -> Void = { duration in
            try await Task.sleep(for: duration)
        }
    ) {
        self.appState = appState
        self.sessionViewModel = sessionViewModel
        self.compactionBannerSleepHandler = compactionBannerSleepHandler
    }

    var draftMessage: String {
        get { draftsBySession[draftStorageKey(for: currentSessionID)] ?? "" }
        set {
            let key = draftStorageKey(for: currentSessionID)
            if newValue.isEmpty {
                draftsBySession.removeValue(forKey: key)
            } else {
                draftsBySession[key] = newValue
            }
        }
    }

    var pendingAttachments: [PendingAttachment] {
        get { pendingAttachmentsBySession[draftStorageKey(for: currentSessionID)] ?? [] }
        set {
            let key = draftStorageKey(for: currentSessionID)
            if newValue.isEmpty {
                pendingAttachmentsBySession.removeValue(forKey: key)
            } else {
                pendingAttachmentsBySession[key] = newValue
            }
        }
    }

    var queuedMessage: String? {
        guard let currentSessionID else {
            return nil
        }
        return queuedDraftsBySession[currentSessionID]?.summaryText
    }

    var activeStreamSessionIDs: Set<String> {
        Set(streamStates.keys)
    }

    var activeStreamSessionID: String? {
        if let currentSessionID, activeStreamSessionIDs.contains(currentSessionID) {
            return currentSessionID
        }

        return activeStreamSessionIDs.sorted().first
    }

    var isCurrentSessionStreaming: Bool {
        guard let currentSessionID else {
            return false
        }
        return activeStreamSessionIDs.contains(currentSessionID)
    }

    var isStreamingInAnotherSession: Bool {
        guard let currentSessionID else {
            return false
        }
        return activeStreamSessionIDs.contains { $0 != currentSessionID }
    }

    var visibleStreamingText: String {
        guard let currentSessionID, isCurrentSessionStreaming else {
            return ""
        }

        return streamStates[currentSessionID]?.text ?? ""
    }

    var visibleCurrentPhase: StreamingPhase? {
        guard let currentSessionID, isCurrentSessionStreaming else {
            return nil
        }

        return streamStates[currentSessionID]?.phase
    }

    var shouldAutoScrollStreamingUpdates: Bool {
        guard let currentSessionID, isCurrentSessionStreaming else {
            return true
        }

        return streamingDisplayController(for: currentSessionID).isPinnedToBottom
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
        guard let currentSessionID else {
            return false
        }

        return retryRequestsBySession[currentSessionID] != nil && !isCurrentSessionStreaming
    }

    var pendingPermissionPromptCount: Int {
        queuedPermissionPrompts.count + (activePermissionPrompt == nil ? 0 : 1)
    }

    var hasPendingPermissionPrompt: Bool {
        pendingPermissionPromptCount > 0
    }

    var permissionPromptIndicatorText: String? {
        if let activePermissionPrompt {
            return activePermissionPrompt.indicatorText
        }

        guard pendingPermissionPromptCount > 0 else {
            return nil
        }

        if pendingPermissionPromptCount == 1 {
            return "1 approval request pending"
        }

        return "\(pendingPermissionPromptCount) approval requests pending"
    }

    func invalidateSession(_ sessionID: String) {
        removeCachedMessages(for: sessionID)
        liveToolGroupsBySession.removeValue(forKey: sessionID)
        anonymousToolCallCountersBySession.removeValue(forKey: sessionID)
        draftsBySession.removeValue(forKey: draftStorageKey(for: sessionID))
        pendingAttachmentsBySession.removeValue(forKey: draftStorageKey(for: sessionID))
        queuedDraftsBySession.removeValue(forKey: sessionID)
        errorMessagesBySession.removeValue(forKey: errorStorageKey(for: sessionID))
        retryRequestsBySession.removeValue(forKey: sessionID)
        clearCompactionBanner(for: sessionID)

        if currentSessionID == sessionID {
            transcriptItems = []
            pendingTranscriptScrollBehavior = .snap
        }
    }

    func prepareToDisplaySession(_ sessionID: String?) {
        pendingTranscriptScrollBehavior = .snap
        currentSessionID = sessionID

        guard let sessionID else {
            transcriptItems = []
            isLoadingHistory = false
            appState.clearContext()
            return
        }

        if let cachedMessages = cachedMessages(for: sessionID) {
            transcriptItems = makeTranscriptItems(for: sessionID, messages: cachedMessages)
            isLoadingHistory = false
        } else {
            transcriptItems = transcriptItemsWithLiveToolActivity(for: sessionID)
            isLoadingHistory = isSessionStreaming(sessionID) ? false : true
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
        retryRequestsBySession.removeAll()
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
        guard let sessionID else {
            return false
        }

        return isSessionStreaming(sessionID)
    }

    private func handleActiveStreamingSessionIfNeeded(for sessionID: String?) async -> Bool {
        guard isActiveStreamingSession(sessionID) else {
            return false
        }

        historyLoadSequence += 1
        isLoadingHistory = false
        currentSessionID = sessionID
        pendingTranscriptScrollBehavior = .snap

        if let sessionID, let cachedMessages = cachedTranscript(for: sessionID) {
            transcriptItems = makeTranscriptItems(for: sessionID, messages: cachedMessages)
            await appState.refreshContext(for: sessionID)
        } else if let sessionID {
            transcriptItems = transcriptItemsWithLiveToolActivity(for: sessionID)
        }

        return true
    }

    private func beginLoad(for sessionID: String?) -> Int {
        historyLoadSequence += 1
        let loadSequence = historyLoadSequence

        if shouldPreserveBackgroundStream(for: sessionID) {
            currentSessionID = sessionID
            pendingTranscriptScrollBehavior = .snap
        } else {
            cleanup()
            currentSessionID = sessionID
            if let sessionID {
                retryRequestsBySession.removeValue(forKey: sessionID)
            }
        }

        return loadSequence
    }

    private func shouldPreserveBackgroundStream(for sessionID: String?) -> Bool {
        activeStreamSessionIDs.contains { $0 != sessionID }
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
        let cachedItems = makeTranscriptItems(for: currentSessionID, messages: cachedMessages)
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
            clearErrorMessage(for: sessionID)
            await appState.refreshContext(for: sessionID)
        } catch is CancellationError {
            return
        } catch {
            await handleLoadMessagesError(error, sessionID: sessionID, loadSequence: loadSequence)
        }
    }

    private func applyFetchedMessages(_ messages: [SessionMessage], for sessionID: String) {
        let mergedMessages = mergedFetchedMessages(messages, for: sessionID)
        cacheMessages(mergedMessages, for: sessionID)
        reconcileLiveToolGroupWithHistory(for: sessionID, messages: mergedMessages)
        guard currentSessionID == sessionID else {
            return
        }
        let updatedItems = makeTranscriptItems(for: sessionID, messages: mergedMessages)
        if transcriptItems != updatedItems {
            pendingTranscriptScrollBehavior = .snap
            transcriptItems = updatedItems
        }
    }

    private func mergedFetchedMessages(
        _ fetchedMessages: [SessionMessage],
        for sessionID: String
    ) -> [SessionMessage] {
        guard let existingMessages = transcriptCache[sessionID], !existingMessages.isEmpty else {
            return fetchedMessages
        }

        if areEquivalentMessageSequences(existingMessages, fetchedMessages) {
            return fetchedMessages
        }

        if isEquivalentMessagePrefix(fetchedMessages, of: existingMessages) {
            return existingMessages
        }

        if isEquivalentMessagePrefix(existingMessages, of: fetchedMessages) {
            return fetchedMessages
        }

        return mergeFetchedMessagesPreservingRelativeOrder(
            localMessages: existingMessages,
            fetchedMessages: fetchedMessages
        )
    }

    private func mergeFetchedMessagesPreservingRelativeOrder(
        localMessages: [SessionMessage],
        fetchedMessages: [SessionMessage]
    ) -> [SessionMessage] {
        let sharedPrefixCount = commonEquivalentPrefixCount(localMessages, fetchedMessages)
        guard sharedPrefixCount > 0 else {
            return fetchedMessages
        }

        var mergedMessages = Array(fetchedMessages.prefix(sharedPrefixCount))
        var nextLocalIndex = sharedPrefixCount
        var nextFetchedIndex = sharedPrefixCount

        while nextLocalIndex < localMessages.count, nextFetchedIndex < fetchedMessages.count {
            if messagesAreEquivalentForTranscriptMerge(
                localMessages[nextLocalIndex],
                fetchedMessages[nextFetchedIndex]
            ) {
                mergedMessages.append(fetchedMessages[nextFetchedIndex])
                nextLocalIndex += 1
                nextFetchedIndex += 1
                continue
            }

            guard let alignment = nextEquivalentMessageAlignment(
                localMessages: localMessages,
                startingAt: nextLocalIndex,
                fetchedMessages: fetchedMessages,
                startingAt: nextFetchedIndex
            ) else {
                mergedMessages.append(contentsOf: fetchedMessages[nextFetchedIndex...])
                mergedMessages.append(contentsOf: localMessages[nextLocalIndex...])
                return mergedMessages
            }

            if nextFetchedIndex < alignment.fetchedIndex {
                mergedMessages.append(contentsOf: fetchedMessages[nextFetchedIndex..<alignment.fetchedIndex])
            }
            if nextLocalIndex < alignment.localIndex {
                mergedMessages.append(contentsOf: localMessages[nextLocalIndex..<alignment.localIndex])
            }
            mergedMessages.append(fetchedMessages[alignment.fetchedIndex])
            nextLocalIndex = alignment.localIndex + 1
            nextFetchedIndex = alignment.fetchedIndex + 1
        }

        if nextFetchedIndex < fetchedMessages.count {
            mergedMessages.append(contentsOf: fetchedMessages[nextFetchedIndex...])
        }
        if nextLocalIndex < localMessages.count {
            mergedMessages.append(contentsOf: localMessages[nextLocalIndex...])
        }

        return mergedMessages
    }

    private func commonEquivalentPrefixCount(
        _ lhs: [SessionMessage],
        _ rhs: [SessionMessage]
    ) -> Int {
        let sharedCount = min(lhs.count, rhs.count)
        var prefixCount = 0

        while prefixCount < sharedCount,
              messagesAreEquivalentForTranscriptMerge(lhs[prefixCount], rhs[prefixCount])
        {
            prefixCount += 1
        }

        return prefixCount
    }

    /// Finds the nearest equivalent message pair between the local and fetched sequences,
    /// starting from the given indices. "Nearest" is defined by minimum combined distance
    /// from the start indices, with ties broken by preferring an earlier local index.
    ///
    /// Time complexity: O(L × F) where L = remaining local messages and F = remaining fetched
    /// messages. Each local candidate triggers a linear scan of the fetched tail via
    /// `indexOfEquivalentMessage`. This is acceptable for transcript-sized inputs (tens to
    /// low hundreds of messages).
    private func nextEquivalentMessageAlignment(
        localMessages: [SessionMessage],
        startingAt localStartIndex: Int,
        fetchedMessages: [SessionMessage],
        startingAt fetchedStartIndex: Int
    ) -> (localIndex: Int, fetchedIndex: Int)? {
        guard localStartIndex < localMessages.count, fetchedStartIndex < fetchedMessages.count else {
            return nil
        }

        var bestAlignment: (localIndex: Int, fetchedIndex: Int, distance: Int)?

        for localIndex in localStartIndex..<localMessages.count {
            guard let fetchedIndex = indexOfEquivalentMessage(
                localMessages[localIndex],
                in: fetchedMessages,
                startingAt: fetchedStartIndex
            ) else {
                continue
            }

            let distance = (localIndex - localStartIndex) + (fetchedIndex - fetchedStartIndex)
            if let bestAlignment {
                if distance > bestAlignment.distance {
                    continue
                }
                if distance == bestAlignment.distance, localIndex > bestAlignment.localIndex {
                    continue
                }
            }

            bestAlignment = (localIndex, fetchedIndex, distance)
        }

        guard let bestAlignment else {
            return nil
        }

        return (bestAlignment.localIndex, bestAlignment.fetchedIndex)
    }

    private func indexOfEquivalentMessage(
        _ message: SessionMessage,
        in messages: [SessionMessage],
        startingAt startIndex: Int
    ) -> Int? {
        guard startIndex < messages.count else {
            return nil
        }

        for index in startIndex..<messages.count where messagesAreEquivalentForTranscriptMerge(message, messages[index]) {
            return index
        }

        return nil
    }

    private func areEquivalentMessageSequences(
        _ lhs: [SessionMessage],
        _ rhs: [SessionMessage]
    ) -> Bool {
        lhs.count == rhs.count && isEquivalentMessagePrefix(lhs, of: rhs)
    }

    private func isEquivalentMessagePrefix(
        _ prefix: [SessionMessage],
        of messages: [SessionMessage]
    ) -> Bool {
        guard prefix.count <= messages.count else {
            return false
        }

        for (lhs, rhs) in zip(prefix, messages) {
            guard messagesAreEquivalentForTranscriptMerge(lhs, rhs) else {
                return false
            }
        }

        return true
    }

    private func messagesAreEquivalentForTranscriptMerge(
        _ lhs: SessionMessage,
        _ rhs: SessionMessage
    ) -> Bool {
        lhs.role == rhs.role && lhs.contentBlocks == rhs.contentBlocks
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
        setErrorMessage(error.localizedDescription, for: sessionID)
        pendingTranscriptScrollBehavior = .snap
        await appState.refreshContext(for: nil)
        await appState.noteRecoverableRequestFailure(error)
    }

    private func handleMissingSessionLoad(_ sessionID: String) async {
        removeCachedMessages(for: sessionID)
        clearErrorMessage(for: sessionID)
        sessionViewModel.removeSession(sessionID)
        currentSessionID = nil
        transcriptItems = []
        setErrorMessage("Session no longer exists.", for: nil)
        pendingTranscriptScrollBehavior = .snap
        await appState.refreshContext(for: nil)
    }

    func addAttachment(fromFileURL url: URL) {
        do {
            let attachment = try AttachmentComposer.pendingAttachment(fromFileURL: url)
            try appendPendingAttachment(attachment)
            clearErrorMessage(for: currentSessionID)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func addImageAttachment(data: Data, filename: String, mediaType: String) {
        do {
            let attachment = try AttachmentComposer.imageAttachment(
                data: data,
                filename: filename,
                mediaType: mediaType
            )
            try appendPendingAttachment(attachment)
            clearErrorMessage(for: currentSessionID)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func addPastedImage(data: Data) {
        do {
            let attachment = try AttachmentComposer.pastedImageAttachment(data: data)
            try appendPendingAttachment(attachment)
            clearErrorMessage(for: currentSessionID)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func removeAttachment(id: UUID) {
        pendingAttachments = AttachmentComposer.removeAttachment(
            id: id,
            from: pendingAttachments
        )
    }

    private func appendPendingAttachment(_ attachment: PendingAttachment) throws {
        pendingAttachments = try AttachmentComposer.append([attachment], to: pendingAttachments)
    }

    func sendDraft() {
        let trimmed = draftMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        let attachments = pendingAttachments
        guard !trimmed.isEmpty || !attachments.isEmpty else {
            return
        }

        guard appState.connectionStatus == .connected else {
            errorMessage = "Reconnecting to Fawx. Try sending again once the connection is restored."
            return
        }

        draftMessage = ""
        pendingAttachments = []

        if isCurrentSessionStreaming, let currentSessionID {
            queuedDraftsBySession[currentSessionID] = QueuedDraft(
                text: trimmed,
                attachments: attachments
            )
            return
        }

        Task {
            await send(trimmed, attachments: attachments)
        }
    }

    func retryLastMessage() {
        guard
            let currentSessionID,
            let retryRequest = retryRequestsBySession[currentSessionID],
            !isCurrentSessionStreaming
        else {
            return
        }

        retryRequestsBySession.removeValue(forKey: currentSessionID)
        Task {
            await send(
                retryRequest.text,
                attachments: retryRequest.attachments,
                forceSessionID: retryRequest.sessionID
            )
        }
    }

    func dismissQueuedMessage() {
        clearQueuedMessage(for: currentSessionID)
    }

    func stopStreaming() {
        // Permission prompts are still app-global, so canceling the visible stream
        // must also tear down any visible approval state.
        clearPermissionPromptState()
        guard let currentSessionID else {
            return
        }

        stopStreaming(sessionID: currentSessionID)
    }

    func stopStreaming(sessionID: String) {
        streamTasks[sessionID]?.cancel()
    }

    func cleanup() {
        clearPermissionPromptState()
        for sessionID in Array(streamTasks.keys) {
            stopStreaming(sessionID: sessionID)
        }
        clearQueuedMessages()
        clearCompactionBanners()
    }

    func updateStreamingDistanceFromBottom(
        _ distanceFromBottom: CGFloat,
        isUserInitiated: Bool = true
    ) {
        guard let currentSessionID, isCurrentSessionStreaming else {
            return
        }

        streamingDisplayController(for: currentSessionID).userDidScroll(
            distanceFromBottom: distanceFromBottom,
            isUserInitiated: isUserInitiated
        )
    }

    func updateStreamingPinnedState(isPinnedToBottom: Bool, distanceFromBottom: CGFloat) {
        guard let currentSessionID, isCurrentSessionStreaming else {
            return
        }

        streamingDisplayController(for: currentSessionID).setPinnedToBottom(
            isPinnedToBottom,
            distanceFromBottom: distanceFromBottom
        )
    }

    private func send(
        _ text: String,
        attachments: [PendingAttachment] = [],
        forceSessionID: String? = nil
    ) async {
        await appState.synchronizeLocalConnectionIfNeeded()
        var targetSessionID = forceSessionID ?? currentSessionID
        setErrorMessage(nil, for: targetSessionID)
        let payload = AttachmentComposer.prepareMessage(message: text, attachments: attachments)

        if targetSessionID == nil {
            do {
                let createdSession = try await appState.client.createSession(model: appState.activeModel?.modelID)
                sessionViewModel.upsert(createdSession)
                sessionViewModel.select(createdSession.id)
                currentSessionID = createdSession.id
                targetSessionID = createdSession.id
            } catch {
                draftMessage = text
                pendingAttachments = attachments
                setErrorMessage("Failed to create session. \(error.localizedDescription)", for: nil)
                return
            }
        }

        guard let sessionID = targetSessionID else {
            setErrorMessage("No session available.", for: forceSessionID ?? currentSessionID)
            return
        }

        let timestamp = Int(Date().timeIntervalSince1970)
        let userMessage = SessionMessage(
            role: .user,
            contentBlocks: payload.contentBlocks,
            timestamp: timestamp
        )
        appendMessage(userMessage, for: sessionID)
        let previewText = userMessage.transcriptDisplayText
        sessionViewModel.updatePreview(
            for: sessionID,
            text: previewText,
            model: appState.activeModel?.modelID
        )
        retryRequestsBySession[sessionID] = RetryRequest(
            text: text,
            attachments: attachments,
            sessionID: sessionID
        )

        do {
            let stream = try await appState.client.sendMessageStream(
                sessionID: sessionID,
                message: payload.message,
                images: payload.images,
                documents: payload.documents
            )
            startStreaming(
                stream,
                sessionID: sessionID,
                retryText: text,
                retryAttachments: attachments
            )
        } catch {
            setErrorMessage("Failed to send message. \(error.localizedDescription)", for: sessionID)
        }
    }

    private func startStreaming(
        _ stream: AsyncThrowingStream<SSEEvent, Error>,
        sessionID: String,
        retryText: String,
        retryAttachments: [PendingAttachment]
    ) {
        stopStreaming(sessionID: sessionID)
        streamStates[sessionID] = SessionStreamingState(text: "", phase: nil)
        streamingDisplayController(for: sessionID).reset(repinToBottom: true)

        let assistantTimestamp = Int(Date().timeIntervalSince1970)
        let task = Task {
            var finalResponse: String?
            var streamFailed = false

            do {
                streamLoop: for try await event in stream {
                    switch event {
                    case .textDelta(let text):
                        streamingDisplayController(for: sessionID).appendToken(text)
                    case .notification(let title, let body):
                        await NotificationService.shared.send(title: title, body: body)
                    case .toolCallStart(let id, let name):
                        beginToolCall(sessionID: sessionID, id: id, name: name)
                    case .toolCallDelta(let id, let argumentsDelta):
                        updateToolCall(sessionID: sessionID, id: id) { toolCall in
                            toolCall.arguments += argumentsDelta
                        }
                    case .toolCallComplete(let id, let name, let arguments):
                        completeToolCall(sessionID: sessionID, id: id, name: name, arguments: arguments)
                    case .toolResult(let id, let output, let isError):
                        finishToolCall(sessionID: sessionID, id: id, output: output, isError: isError)
                    case .permissionPrompt(let prompt):
                        enqueuePermissionPrompt(prompt)
                    case .phase(let phase):
                        setStreamingPhase(StreamingPhase(rawValue: phase), for: sessionID)
                    case .contextCompacted(
                        let tier,
                        let messagesRemoved,
                        let tokensBefore,
                        let tokensAfter,
                        let usageRatio
                    ):
                        handleContextCompacted(
                            tier: tier,
                            messagesRemoved: messagesRemoved,
                            tokensBefore: tokensBefore,
                            tokensAfter: tokensAfter,
                            usageRatio: usageRatio,
                            sessionID: sessionID
                        )
                    case .done(let response):
                        finalResponse = response
                    case .engineError(_, let message, let recoverable):
                        if !recoverable {
                            handleStreamError(message, sessionID: sessionID)
                            streamFailed = true
                            break streamLoop
                        }
                    case .error(let message):
                        handleStreamError(message, sessionID: sessionID)
                        streamFailed = true
                        break streamLoop
                    }
                }

                if streamFailed && finalResponse == nil {
                    let recovered = await recoverInterruptedStream(
                        sessionID: sessionID,
                        retryContentBlocks: payloadContentBlocks(
                            text: retryText,
                            attachments: retryAttachments
                        )
                    )
                    if !recovered {
                        retryRequestsBySession[sessionID] = RetryRequest(
                            text: retryText,
                            attachments: retryAttachments,
                            sessionID: sessionID
                        )
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
                    retryContentBlocks: payloadContentBlocks(
                        text: retryText,
                        attachments: retryAttachments
                    )
                )
                if !recovered {
                    retryRequestsBySession[sessionID] = RetryRequest(
                        text: retryText,
                        attachments: retryAttachments,
                        sessionID: sessionID
                    )
                    await finalizeCancellation(timestamp: assistantTimestamp, sessionID: sessionID)
                }
            }
        }

        streamTasks[sessionID] = task
    }

    private func finalizeStream(timestamp: Int, finalResponse: String?, sessionID: String) async {
        streamingDisplayController(for: sessionID).streamDidEnd()
        let content = finalResponse ?? streamingText(for: sessionID)
        if !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let assistantMessage = SessionMessage(role: .assistant, content: content, timestamp: timestamp)
            appendMessage(assistantMessage, for: sessionID)
            sessionViewModel.updatePreview(for: sessionID, text: content, model: appState.activeModel?.modelID)
        }

        retryRequestsBySession.removeValue(forKey: sessionID)
        clearErrorMessage(for: sessionID)
        resetStreamingState(for: sessionID)
        await refreshSessionTranscriptFromServer(sessionID: sessionID)

        if currentSessionID == sessionID {
            await appState.refreshContext(for: sessionID)
        }
        await sessionViewModel.refresh()
        await sendQueuedMessageIfNeeded(finishedSessionID: sessionID)
    }

    private func finalizeCancellation(timestamp: Int, sessionID: String) async {
        streamingDisplayController(for: sessionID).streamDidEnd()
        let currentStreamingText = streamingText(for: sessionID)
        if !currentStreamingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let interrupted = currentStreamingText + "\n\n(interrupted)"
            let assistantMessage = SessionMessage(role: .assistant, content: interrupted, timestamp: timestamp)
            appendMessage(assistantMessage, for: sessionID)
            sessionViewModel.updatePreview(for: sessionID, text: interrupted, model: appState.activeModel?.modelID)
        }

        resetStreamingState(for: sessionID)
        if currentSessionID == sessionID {
            await appState.refreshContext(for: sessionID)
        }
    }

    private func recoverInterruptedStream(
        sessionID: String,
        retryContentBlocks: [SessionContentBlock]
    ) async -> Bool {
        streamingDisplayController(for: sessionID).streamDidEnd()
        do {
            let response = try await appState.client.sessionMessages(id: sessionID, limit: 200)
            guard
                let lastUserIndex = response.messages.lastIndex(where: { message in
                    message.role == .user && message.contentBlocks == retryContentBlocks
                }),
                response.messages.indices.contains(response.messages.index(after: lastUserIndex))
            else {
                return false
            }

            let mergedMessages = mergedFetchedMessages(response.messages, for: sessionID)
            cacheMessages(mergedMessages, for: sessionID)
            reconcileLiveToolGroupWithHistory(for: sessionID, messages: mergedMessages)
            if currentSessionID == sessionID {
                transcriptItems = makeTranscriptItems(for: sessionID, messages: mergedMessages)
            }
            clearErrorMessage(for: sessionID)
            retryRequestsBySession.removeValue(forKey: sessionID)
            resetStreamingState(for: sessionID)
            if currentSessionID == sessionID {
                await appState.refreshContext(for: sessionID)
            }
            await sessionViewModel.refresh()
            await sendQueuedMessageIfNeeded(finishedSessionID: sessionID)
            return true
        } catch {
            await appState.noteRecoverableRequestFailure(error)
            return false
        }
    }

    private func payloadContentBlocks(
        text: String,
        attachments: [PendingAttachment]
    ) -> [SessionContentBlock] {
        AttachmentComposer.prepareMessage(message: text, attachments: attachments).contentBlocks
    }

    private func sendQueuedMessageIfNeeded(finishedSessionID: String) async {
        guard let queuedDelivery = consumeQueuedMessageIfReady(finishedSessionID: finishedSessionID) else {
            return
        }

        await send(
            queuedDelivery.text,
            attachments: queuedDelivery.attachments,
            forceSessionID: queuedDelivery.sessionID
        )
    }

    private func resetStreamingState(for sessionID: String) {
        streamTasks.removeValue(forKey: sessionID)
        streamStates.removeValue(forKey: sessionID)
        streamingDisplayControllers[sessionID]?.reset(repinToBottom: false)
        streamingDisplayControllers.removeValue(forKey: sessionID)
        if streamStates.isEmpty {
            clearPermissionPromptState()
        }
    }

    private func draftStorageKey(for sessionID: String?) -> String {
        sessionID ?? ""
    }

    private func errorStorageKey(for sessionID: String?) -> String {
        sessionID ?? ""
    }

    private func setErrorMessage(_ message: String?, for sessionID: String?) {
        let key = errorStorageKey(for: sessionID)
        if let message = message?.trimmingCharacters(in: .whitespacesAndNewlines), !message.isEmpty {
            errorMessagesBySession[key] = message
        } else {
            errorMessagesBySession.removeValue(forKey: key)
        }
    }

    private func clearErrorMessage(for sessionID: String?) {
        setErrorMessage(nil, for: sessionID)
    }

    private func clearQueuedMessage(for sessionID: String?) {
        guard let sessionID else {
            return
        }

        queuedDraftsBySession.removeValue(forKey: sessionID)
    }

    private func clearQueuedMessages() {
        queuedDraftsBySession.removeAll()
    }

    private func handleContextCompacted(
        tier: String,
        messagesRemoved: Int,
        tokensBefore: Int,
        tokensAfter: Int,
        usageRatio: Double,
        sessionID: String
    ) {
        guard currentSessionID == sessionID else {
            return
        }

        let updatedContext = (
            appState.currentContext
                ?? ContextInfo(
                    usedTokens: tokensAfter,
                    maxTokens: 0,
                    percentage: usageRatio,
                    compactionThreshold: 0
                )
        ).applyingCompaction(usedTokens: tokensAfter, usageRatio: usageRatio)

        let derivedBeforePercentage = ContextInfo(
            usedTokens: tokensBefore,
            maxTokens: updatedContext.maxTokens,
            percentage: .nan,
            compactionThreshold: updatedContext.compactionThreshold
        ).normalizedPercentage
        let beforePercentage: Double
        if let currentContext = appState.currentContext {
            let currentPercentage = currentContext.normalizedPercentage
            if currentContext.maxTokens > 0 || currentPercentage > 0 || tokensBefore == 0 {
                beforePercentage = currentPercentage
            } else {
                beforePercentage = derivedBeforePercentage
            }
        } else {
            beforePercentage = derivedBeforePercentage
        }
        let afterPercentage = updatedContext.normalizedPercentage

        appState.currentContext = updatedContext
        showCompactionBanner(
            makeCompactionBannerInfo(
                tier: tier,
                messagesRemoved: messagesRemoved,
                beforePercentage: beforePercentage,
                afterPercentage: afterPercentage
            ),
            for: sessionID
        )
    }

    private func makeCompactionBannerInfo(
        tier: String,
        messagesRemoved: Int,
        beforePercentage: Double,
        afterPercentage: Double
    ) -> CompactionBannerInfo {
        let isEmergency = tier.lowercased() == "emergency"
        let prefix = isEmergency ? "Context urgently optimized" : "Context optimized"
        let compactedMessageCount = messagesRemoved == 1
            ? "1 message compacted"
            : "\(messagesRemoved) messages compacted"

        return CompactionBannerInfo(
            message: "\(prefix): \(compactedMessageCount), \(Int(beforePercentage.rounded()))% → \(Int(afterPercentage.rounded()))%",
            isEmergency: isEmergency
        )
    }

    private func showCompactionBanner(_ info: CompactionBannerInfo, for sessionID: String) {
        compactionBannerDismissTasks[sessionID]?.cancel()
        compactionBannerInfosBySession[sessionID] = info
        compactionBannerDismissTasks[sessionID] = Task { [weak self] in
            guard let self else {
                return
            }

            do {
                try await self.compactionBannerSleepHandler(Self.compactionBannerTimeout)
            } catch is CancellationError {
                return
            } catch {
                return
            }

            self.dismissCompactionBannerIfNeeded(expectedInfo: info, sessionID: sessionID)
        }
    }

    private func dismissCompactionBannerIfNeeded(expectedInfo: CompactionBannerInfo, sessionID: String) {
        guard compactionBannerInfosBySession[sessionID] == expectedInfo else {
            return
        }

        clearCompactionBanner(for: sessionID)
    }

    private func clearCompactionBanner(for sessionID: String) {
        compactionBannerDismissTasks[sessionID]?.cancel()
        compactionBannerDismissTasks.removeValue(forKey: sessionID)
        compactionBannerInfosBySession.removeValue(forKey: sessionID)
    }

    private func clearCompactionBanners() {
        for task in compactionBannerDismissTasks.values {
            task.cancel()
        }
        compactionBannerDismissTasks.removeAll()
        compactionBannerInfosBySession.removeAll()
    }

    private func consumeQueuedMessageIfReady(
        finishedSessionID: String
    ) -> (text: String, attachments: [PendingAttachment], sessionID: String?)? {
        guard let queuedDraft = queuedDraftsBySession[finishedSessionID] else {
            return nil
        }
        guard appState.connectionStatus == .connected else {
            return nil
        }

        queuedDraftsBySession.removeValue(forKey: finishedSessionID)
        return (queuedDraft.text, queuedDraft.attachments, finishedSessionID)
    }

    func respondToPermissionPrompt(_ decision: PermissionPromptDecision) {
        Task {
            await submitPermissionPromptDecision(decision)
        }
    }

    private func enqueuePermissionPrompt(_ prompt: PermissionPrompt) {
        permissionPromptErrorMessage = nil

        if activePermissionPrompt?.id == prompt.id {
            activePermissionPrompt = prompt
            isRespondingToPermissionPrompt = false
            schedulePermissionPromptTimeout(for: prompt)
            return
        }

        if let index = queuedPermissionPrompts.firstIndex(where: { $0.id == prompt.id }) {
            queuedPermissionPrompts[index] = prompt
        } else {
            queuedPermissionPrompts.append(prompt)
        }

        activateNextPermissionPromptIfNeeded()
    }

    private func activateNextPermissionPromptIfNeeded() {
        guard activePermissionPrompt == nil else {
            return
        }

        guard !queuedPermissionPrompts.isEmpty else {
            return
        }

        activePermissionPrompt = queuedPermissionPrompts.removeFirst()
        isRespondingToPermissionPrompt = false
        permissionPromptErrorMessage = nil

        if let activePermissionPrompt {
            schedulePermissionPromptTimeout(for: activePermissionPrompt)
        }
    }

    private func schedulePermissionPromptTimeout(for prompt: PermissionPrompt) {
        permissionPromptTimeoutTask?.cancel()
        permissionPromptTimeoutTask = Task { [weak self] in
            do {
                try await Task.sleep(for: Self.permissionPromptTimeout)
            } catch is CancellationError {
                return
            } catch {
                return
            }

            await self?.autoDenyPermissionPromptIfNeeded(promptID: prompt.id)
        }
    }

    private func autoDenyPermissionPromptIfNeeded(promptID: String) async {
        guard activePermissionPrompt?.id == promptID else {
            return
        }

        await submitPermissionPromptDecision(.deny, isAutomatic: true)
    }

    private func submitPermissionPromptDecision(
        _ decision: PermissionPromptDecision,
        isAutomatic: Bool = false
    ) async {
        guard let prompt = activePermissionPrompt, !isRespondingToPermissionPrompt else {
            return
        }

        isRespondingToPermissionPrompt = true
        permissionPromptErrorMessage = nil
        permissionPromptTimeoutTask?.cancel()

        do {
            try await appState.client.respondToPermissionPrompt(id: prompt.id, decision: decision)
            finishActivePermissionPrompt(id: prompt.id)

            if isAutomatic {
                appState.showToast(message: "Approval request timed out and was denied.", style: .warning)
            }
        } catch is CancellationError {
            isRespondingToPermissionPrompt = false
            if activePermissionPrompt?.id == prompt.id {
                schedulePermissionPromptTimeout(for: prompt)
            }
        } catch let apiError as APIError where apiError.statusCode == 404 || apiError.statusCode == 409 {
            finishActivePermissionPrompt(id: prompt.id)

            if isAutomatic {
                appState.showToast(message: "Approval request expired.", style: .warning)
            }
        } catch {
            isRespondingToPermissionPrompt = false
            permissionPromptErrorMessage = "Couldn't send approval response. \(error.localizedDescription)"

            if activePermissionPrompt?.id == prompt.id {
                schedulePermissionPromptTimeout(for: prompt)
            }
        }
    }

    private func finishActivePermissionPrompt(id: String) {
        permissionPromptTimeoutTask?.cancel()
        permissionPromptTimeoutTask = nil
        permissionPromptErrorMessage = nil

        guard activePermissionPrompt?.id == id else {
            isRespondingToPermissionPrompt = false
            return
        }

        activePermissionPrompt = nil
        isRespondingToPermissionPrompt = false
        activateNextPermissionPromptIfNeeded()
    }

    private func clearPermissionPromptState() {
        permissionPromptTimeoutTask?.cancel()
        permissionPromptTimeoutTask = nil
        activePermissionPrompt = nil
        queuedPermissionPrompts.removeAll(keepingCapacity: true)
        isRespondingToPermissionPrompt = false
        permissionPromptErrorMessage = nil
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
            let shouldPreserveScrollPosition =
                isSessionStreaming(sessionID)
                && !streamingDisplayController(for: sessionID).isPinnedToBottom
            pendingTranscriptScrollBehavior = shouldPreserveScrollPosition ? .preservePosition : .animated
            transcriptItems = makeTranscriptItems(for: sessionID, messages: updatedMessages)
        }
    }

    private func handleStreamError(_ message: String, sessionID: String) {
        setErrorMessage(message, for: sessionID)
    }

    private func isSessionStreaming(_ sessionID: String) -> Bool {
        streamStates[sessionID] != nil
    }

    private func streamingText(for sessionID: String) -> String {
        streamStates[sessionID]?.text ?? ""
    }

    private func setStreamingPhase(_ phase: StreamingPhase?, for sessionID: String) {
        guard streamStates[sessionID] != nil else {
            return
        }

        streamStates[sessionID]?.phase = phase
    }

    private func appendStreamingText(_ text: String, for sessionID: String) {
        guard !text.isEmpty, streamStates[sessionID] != nil else {
            return
        }

        streamStates[sessionID]?.text += text
    }

    private func streamingDisplayController(for sessionID: String) -> StreamingDisplayController {
        if let controller = streamingDisplayControllers[sessionID] {
            return controller
        }

        let controller = StreamingDisplayController { [weak self] flushedText in
            self?.appendStreamingText(flushedText, for: sessionID)
        }
        streamingDisplayControllers[sessionID] = controller
        return controller
    }

    func makeTranscriptItems(
        for sessionID: String?,
        messages: [SessionMessage]
    ) -> [ChatTranscriptItem] {
        var duplicateCounts: [String: Int] = [:]
        var items: [ChatTranscriptItem] = []
        var pendingHistoricalGroupIndex: Int?

        for message in messages {
            switch message.role {
            case .tool:
                let toolResults = toolResults(from: message)
                if !toolResults.isEmpty, let pendingHistoricalGroupIndex {
                    applyToolResults(toolResults, to: &items, groupIndex: pendingHistoricalGroupIndex)
                    continue
                }

                let displayText = message.transcriptDisplayText
                if !displayText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    items.append(messageTranscriptItem(message, displayText: displayText, duplicateCounts: &duplicateCounts))
                }
                pendingHistoricalGroupIndex = nil
            case .user, .assistant, .system:
                let displayText = message.transcriptDisplayText
                if !displayText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    items.append(messageTranscriptItem(message, displayText: displayText, duplicateCounts: &duplicateCounts))
                }

                let historicalToolCalls = toolCalls(from: message)
                if !historicalToolCalls.isEmpty {
                    items.append(.toolActivityGroup(
                        ToolActivityGroupRecord(
                            id: historicalToolGroupID(for: message, toolCalls: historicalToolCalls),
                            toolCalls: historicalToolCalls,
                            isLive: false
                        )
                    ))
                    pendingHistoricalGroupIndex = items.count - 1
                } else {
                    pendingHistoricalGroupIndex = nil
                }
            }
        }

        if let sessionID, let liveToolGroup = liveToolGroupOverlay(for: sessionID, messages: messages) {
            items.append(.toolActivityGroup(liveToolGroup))
        }

        return items
    }

    private func messageTranscriptItem(
        _ message: SessionMessage,
        displayText: String,
        duplicateCounts: inout [String: Int]
    ) -> ChatTranscriptItem {
        let baseID = messageStableIDBase(for: message)
        let occurrence = duplicateCounts[baseID, default: 0]
        duplicateCounts[baseID] = occurrence + 1
        let id = occurrence == 0 ? baseID : "\(baseID)#\(occurrence)"
        return .message(TranscriptMessage(id: id, message: message, displayText: displayText))
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
        let protectedSessions = activeStreamSessionIDs.union(Set([currentSessionID].compactMap { $0 }))
        var scannedEntries = 0

        while transcriptCache.count > Self.maxCachedSessions, scannedEntries < transcriptCacheAccessOrder.count {
            let leastRecentSessionID = transcriptCacheAccessOrder.removeFirst()
            if protectedSessions.contains(leastRecentSessionID) {
                transcriptCacheAccessOrder.append(leastRecentSessionID)
                scannedEntries += 1
                continue
            }

            transcriptCache.removeValue(forKey: leastRecentSessionID)
            liveToolGroupsBySession.removeValue(forKey: leastRecentSessionID)
            anonymousToolCallCountersBySession.removeValue(forKey: leastRecentSessionID)
            scannedEntries = 0
        }

        while transcriptCache.count > Self.maxCachedSessions, let leastRecentSessionID = transcriptCacheAccessOrder.first {
            transcriptCacheAccessOrder.removeFirst()
            transcriptCache.removeValue(forKey: leastRecentSessionID)
            liveToolGroupsBySession.removeValue(forKey: leastRecentSessionID)
            anonymousToolCallCountersBySession.removeValue(forKey: leastRecentSessionID)
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

    private func transcriptItemsWithLiveToolActivity(for sessionID: String) -> [ChatTranscriptItem] {
        guard let liveToolGroup = liveToolGroupsBySession[sessionID] else {
            return []
        }

        return [.toolActivityGroup(liveToolGroup)]
    }

    private func liveToolGroupOverlay(
        for sessionID: String,
        messages: [SessionMessage]
    ) -> ToolActivityGroupRecord? {
        guard let liveToolGroup = liveToolGroupsBySession[sessionID] else {
            return nil
        }

        if isLiveToolGroupRepresentedInHistory(liveToolGroup, messages: messages) {
            return nil
        }

        return liveToolGroup
    }

    private func reconcileLiveToolGroupWithHistory(for sessionID: String, messages: [SessionMessage]) {
        guard let liveToolGroup = liveToolGroupsBySession[sessionID] else {
            return
        }

        if isLiveToolGroupRepresentedInHistory(liveToolGroup, messages: messages) {
            liveToolGroupsBySession.removeValue(forKey: sessionID)
        }
    }

    private func refreshVisibleTranscriptForToolActivity(
        sessionID: String,
        preferPreservePosition: Bool
    ) {
        guard currentSessionID == sessionID else {
            return
        }

        let cachedMessages = transcriptCache[sessionID] ?? []
        pendingTranscriptScrollBehavior = preferPreservePosition ? .preservePosition : .animated
        transcriptItems = makeTranscriptItems(for: sessionID, messages: cachedMessages)
    }

    private func beginToolCall(sessionID: String, id: String?, name: String?) {
        let toolCallID = stableToolCallID(for: sessionID, rawID: id)
        var toolGroup = activeOrFreshLiveToolGroup(for: sessionID)

        if let index = toolGroup.toolCalls.firstIndex(where: { $0.id == toolCallID }) {
            toolGroup.toolCalls[index].name = name ?? toolGroup.toolCalls[index].name
            toolGroup.toolCalls[index].isRunning = true
        } else {
            toolGroup.toolCalls.append(
                ToolCallRecord(
                    id: toolCallID,
                    name: name ?? "tool",
                    arguments: "",
                    result: nil,
                    isRunning: true,
                    isError: false
                )
            )
        }

        liveToolGroupsBySession[sessionID] = toolGroup
        refreshVisibleTranscriptForToolActivity(
            sessionID: sessionID,
            preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
        )
    }

    private func completeToolCall(sessionID: String, id: String?, name: String?, arguments: String) {
        updateToolCall(sessionID: sessionID, id: id) { toolCall in
            if let name {
                toolCall.name = name
            }
            toolCall.arguments = arguments
        }
    }

    private func finishToolCall(sessionID: String, id: String?, output: String, isError: Bool) {
        updateToolCall(sessionID: sessionID, id: id) { toolCall in
            toolCall.result = output
            toolCall.isRunning = false
            toolCall.isError = isError
        }
    }

    private func updateToolCall(
        sessionID: String,
        id: String?,
        update: (inout ToolCallRecord) -> Void
    ) {
        let toolCallID = stableToolCallID(for: sessionID, rawID: id)
        var toolGroup = activeOrFreshLiveToolGroup(for: sessionID)

        if let index = toolGroup.toolCalls.firstIndex(where: { $0.id == toolCallID }) {
            update(&toolGroup.toolCalls[index])
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
            toolGroup.toolCalls.append(toolCall)
        }

        liveToolGroupsBySession[sessionID] = toolGroup
        refreshVisibleTranscriptForToolActivity(
            sessionID: sessionID,
            preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
        )
    }

    private func activeOrFreshLiveToolGroup(for sessionID: String) -> ToolActivityGroupRecord {
        if let existingGroup = liveToolGroupsBySession[sessionID] {
            return existingGroup
        }

        return ToolActivityGroupRecord(
            id: "live-\(sessionID)",
            toolCalls: [],
            isLive: true
        )
    }

    private func shouldPreserveScrollPosition(for sessionID: String) -> Bool {
        isSessionStreaming(sessionID)
            && currentSessionID == sessionID
            && !streamingDisplayController(for: sessionID).isPinnedToBottom
    }

    private func isLiveToolGroupRepresentedInHistory(
        _ liveToolGroup: ToolActivityGroupRecord,
        messages: [SessionMessage]
    ) -> Bool {
        guard !liveToolGroup.toolCalls.isEmpty else {
            return false
        }

        let historicalToolUses = Set(messages.flatMap { message in
            message.contentBlocks.compactMap { block -> String? in
                guard case .toolUse(let id, _, _) = block else {
                    return nil
                }
                return id
            }
        })
        let historicalToolResults = Set(messages.flatMap { message in
            toolResults(from: message).map(\.id)
        })

        return liveToolGroup.toolCalls.allSatisfy { toolCall in
            guard !toolCall.isRunning, historicalToolUses.contains(toolCall.id) else {
                return false
            }

            if toolCall.result != nil {
                return historicalToolResults.contains(toolCall.id)
            }

            return true
        }
    }

    private func toolCalls(from message: SessionMessage) -> [ToolCallRecord] {
        message.contentBlocks.compactMap { block in
            guard case .toolUse(let id, let name, let input) = block else {
                return nil
            }

            let arguments = renderedJSONValue(input)
            return ToolCallRecord(
                id: id,
                name: name,
                arguments: arguments,
                result: nil,
                isRunning: false,
                isError: false
            )
        }
    }

    private func toolResults(from message: SessionMessage) -> [(id: String, output: String, isError: Bool)] {
        message.contentBlocks.compactMap { block in
            guard case .toolResult(let toolUseID, let content, let storedIsError) = block else {
                return nil
            }

            let renderedOutput = renderedJSONValue(content)
            let legacyPrefixedError = renderedOutput.hasPrefix("[ERROR]")
            let isError = storedIsError ?? legacyPrefixedError
            let cleanedOutput = legacyPrefixedError
                ? renderedOutput.replacingOccurrences(of: "[ERROR] ", with: "")
                : renderedOutput
            return (toolUseID, cleanedOutput, isError)
        }
    }

    private func applyToolResults(
        _ toolResults: [(id: String, output: String, isError: Bool)],
        to items: inout [ChatTranscriptItem],
        groupIndex: Int
    ) {
        guard case .toolActivityGroup(var group) = items[groupIndex] else {
            return
        }

        for toolResult in toolResults {
            if let toolIndex = group.toolCalls.firstIndex(where: { $0.id == toolResult.id }) {
                group.toolCalls[toolIndex].result = toolResult.output
                group.toolCalls[toolIndex].isRunning = false
                group.toolCalls[toolIndex].isError = toolResult.isError
            } else {
                group.toolCalls.append(
                    ToolCallRecord(
                        id: toolResult.id,
                        name: "Unknown tool",
                        arguments: "",
                        result: toolResult.output,
                        isRunning: false,
                        isError: toolResult.isError
                    )
                )
            }
        }

        items[groupIndex] = .toolActivityGroup(group)
    }

    private func historicalToolGroupID(
        for message: SessionMessage,
        toolCalls: [ToolCallRecord]
    ) -> String {
        let component = toolCalls
            .map {
                "\($0.id.utf8.count)#\($0.id)\($0.displayName.utf8.count)#\($0.displayName)"
            }
            .joined()
        return "history:\(messageStableIDBase(for: message)):\(Self.stableDigest(for: component))"
    }

    private func renderedJSONValue(_ value: JSONValue) -> String {
        switch value {
        case .null:
            return ""
        default:
            return value.description
        }
    }

    private func refreshSessionTranscriptFromServer(sessionID: String) async {
        do {
            let response = try await appState.client.sessionMessages(id: sessionID, limit: 200)
            applyFetchedMessages(response.messages, for: sessionID)
        } catch is CancellationError {
            return
        } catch {
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func stableToolCallID(for sessionID: String, rawID: String?) -> String {
        if let rawID, !rawID.isEmpty {
            return rawID
        }

        let nextIndex = anonymousToolCallCountersBySession[sessionID, default: 0] + 1
        anonymousToolCallCountersBySession[sessionID] = nextIndex
        return "tool-\(nextIndex)"
    }
}

private struct RetryRequest {
    let text: String
    let attachments: [PendingAttachment]
    let sessionID: String
}

#if DEBUG
extension ChatViewModel {
    func makeTranscriptItems(from messages: [SessionMessage]) -> [ChatTranscriptItem] {
        makeTranscriptItems(for: currentSessionID, messages: messages)
    }

    func appendMessageForTesting(_ message: SessionMessage, sessionID: String) {
        appendMessage(message, for: sessionID)
    }

    func handleStreamErrorForTesting(_ message: String, sessionID: String) {
        handleStreamError(message, sessionID: sessionID)
    }

    func clearErrorMessageForTesting(sessionID: String?) {
        clearErrorMessage(for: sessionID)
    }

    func setStreamingStateForTesting(
        isStreaming: Bool,
        currentSessionID: String?,
        streamingSessionID: String?,
        streamingText: String = "",
        phase: StreamingPhase? = nil
    ) {
        self.currentSessionID = currentSessionID
        streamStates.removeAll()
        streamTasks.removeAll()
        streamingDisplayControllers.removeAll()

        if isStreaming, let streamingSessionID {
            streamStates[streamingSessionID] = SessionStreamingState(text: streamingText, phase: phase)
            _ = streamingDisplayController(for: streamingSessionID)
        }
    }

    func setStreamingSessionsForTesting(
        _ sessions: [String: (text: String, phase: StreamingPhase?)],
        currentSessionID: String?
    ) {
        self.currentSessionID = currentSessionID
        streamStates = sessions.reduce(into: [:]) { partialResult, entry in
            partialResult[entry.key] = SessionStreamingState(text: entry.value.text, phase: entry.value.phase)
        }
        streamTasks.removeAll()
        streamingDisplayControllers.removeAll()
        for sessionID in sessions.keys {
            _ = streamingDisplayController(for: sessionID)
        }
    }

    func enqueuePermissionPromptForTesting(_ prompt: PermissionPrompt) {
        enqueuePermissionPrompt(prompt)
    }

    func finishActivePermissionPromptForTesting(id: String) {
        finishActivePermissionPrompt(id: id)
    }

    func stopStreamingForTesting() {
        stopStreaming()
    }

    func cleanupForTesting() {
        cleanup()
    }

    func resetStreamingStateForTesting(sessionID: String) {
        resetStreamingState(for: sessionID)
    }

    func appendStreamingTokenForTesting(_ token: String) {
        guard let currentSessionID else {
            return
        }

        streamingDisplayController(for: currentSessionID).appendToken(token)
    }

    func flushStreamingDisplayForTesting() {
        guard let currentSessionID else {
            return
        }

        streamingDisplayController(for: currentSessionID).streamDidEnd()
    }

    func updateStreamingDistanceFromBottomForTesting(_ distanceFromBottom: CGFloat) {
        updateStreamingDistanceFromBottom(distanceFromBottom)
    }

    var isPinnedToBottomForTesting: Bool {
        guard let currentSessionID else {
            return true
        }

        return streamingDisplayController(for: currentSessionID).isPinnedToBottom
    }

    func streamingTextForTesting(sessionID: String) -> String {
        streamingText(for: sessionID)
    }

    func consumeQueuedMessageForTesting(
        finishedSessionID: String,
        connectionStatus: ConnectionStatus = .connected
    ) -> (text: String, attachments: [PendingAttachment], sessionID: String?)? {
        let previousConnectionStatus = appState.connectionStatus
        appState.connectionStatus = connectionStatus
        defer {
            appState.connectionStatus = previousConnectionStatus
        }

        return consumeQueuedMessageIfReady(finishedSessionID: finishedSessionID)
    }

    func applyFetchedMessagesForTesting(_ messages: [SessionMessage], sessionID: String) {
        applyFetchedMessages(messages, for: sessionID)
    }

    func beginToolCallForTesting(sessionID: String, id: String?, name: String?) {
        beginToolCall(sessionID: sessionID, id: id, name: name)
    }

    func completeToolCallForTesting(sessionID: String, id: String?, name: String?, arguments: String) {
        completeToolCall(sessionID: sessionID, id: id, name: name, arguments: arguments)
    }

    func finishToolCallForTesting(sessionID: String, id: String?, output: String, isError: Bool) {
        finishToolCall(sessionID: sessionID, id: id, output: output, isError: isError)
    }

    func handleContextCompactedForTesting(
        sessionID: String,
        tier: String = "slide",
        messagesRemoved: Int = 12,
        tokensBefore: Int = 68,
        tokensAfter: Int = 42,
        usageRatio: Double = 0.42
    ) {
        handleContextCompacted(
            tier: tier,
            messagesRemoved: messagesRemoved,
            tokensBefore: tokensBefore,
            tokensAfter: tokensAfter,
            usageRatio: usageRatio,
            sessionID: sessionID
        )
    }

    func setCurrentContextForTesting(_ context: ContextInfo?) {
        appState.currentContext = context
    }

    var currentContextForTesting: ContextInfo? {
        appState.currentContext
    }
}
#endif
