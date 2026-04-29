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
  let steering: String?
  let disposition: QueuedDraftDisposition

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

private enum QueuedDraftDisposition: Sendable, Hashable {
  case message
  case steering
}

private enum AcceptedSteeringAnchor: Sendable, Hashable {
  case sessionStart
  case afterMessage(String)
  case afterItem(String)
}

private extension AcceptedSteeringAnchor {
  var isLiveItemAnchor: Bool {
    if case .afterItem = self {
      return true
    }
    return false
  }
}

private struct AcceptedSteeringRecord: Sendable, Hashable {
  let record: TurnSteeringRecord
  let anchor: AcceptedSteeringAnchor
}

private struct HistoricalToolActivityDraft: Sendable, Hashable {
  let toolCalls: [ToolCallRecord]
}

private struct HistoricalTextTranscriptDraft: Sendable, Hashable {
  let text: String
  /// Historical session messages do not yet persist the live stream's
  /// `final_answer_delta` boundary. Keep the structural fallback isolated here
  /// so a future persisted phase marker can replace it without touching the
  /// transcript renderer.
  let isFinalAnswerCandidate: Bool
}

private enum HistoricalMessageTranscriptPart: Sendable, Hashable {
  case text(HistoricalTextTranscriptDraft)
  case activity(HistoricalToolActivityDraft)
}

private enum StreamReductionResult {
  case `continue`
  case finish(String?)
  case fail
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

  static func removeAttachment(id: UUID, from attachments: [PendingAttachment])
    -> [PendingAttachment]
  {
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

    if let imageMediaType = detectedImageMediaType(data: data)
      ?? normalizedImageMediaType(
        preferredMIMEType: mediaType,
        fallbackFilename: filename
      )
    {
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

  static func imageAttachment(data: Data, filename: String, mediaType: String) throws
    -> PendingAttachment
  {
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
      data[8...11] == Data("WEBP".utf8)
    {
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

  func pendingTextHasSuffix(_ text: String) -> Bool {
    !text.isEmpty && pendingTokens.hasSuffix(text)
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
  private static let elapsedDurationFormatter: DateComponentsFormatter = {
    let formatter = DateComponentsFormatter()
    formatter.allowedUnits = [.hour, .minute, .second]
    formatter.maximumUnitCount = 2
    formatter.unitsStyle = .full
    formatter.zeroFormattingBehavior = [.dropAll]
    return formatter
  }()

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
        return "Working"
      case .reason:
        return "Thinking"
      case .act:
        return "Working"
      case .other(let rawValue):
        return rawValue.capitalized
      }
    }

    var streamingPlaceholder: String? {
      switch self {
      case .perceive:
        return "Preparing"
      case .reason:
        return "Thinking"
      case .act:
        return "Working"
      case .other:
        return nil
      }
    }
  }

  enum TranscriptPhaseBoundary: Sendable, Equatable {
    case collectingWork
    case executingTools
    case summarizing
    case finalizing
    case completed
    case other(String)

    init(rawValue: String) {
      switch rawValue.lowercased() {
      case "collecting_work":
        self = .collectingWork
      case "executing_tools":
        self = .executingTools
      case "summarizing":
        self = .summarizing
      case "finalizing":
        self = .finalizing
      case "completed":
        self = .completed
      default:
        self = .other(rawValue)
      }
    }

    var composerLabel: String? {
      switch self {
      case .collectingWork:
        return "Working"
      case .executingTools:
        return "Using Tools"
      case .summarizing:
        return "Summarizing"
      case .finalizing:
        return "Writing"
      case .completed:
        return nil
      case .other(let rawValue):
        // Unknown wire phases are displayed as title-cased fallbacks. Add a
        // curated enum case when a new server phase becomes part of the contract.
        return rawValue
          .split(separator: "_")
          .map { $0.capitalized }
          .joined(separator: " ")
      }
    }

    var allowsOpenEndedFinalAnswer: Bool {
      switch self {
      case .finalizing, .completed:
        return true
      case .collectingWork, .executingTools, .summarizing, .other:
        return false
      }
    }
  }

  enum StreamingProgressKind: Sendable, Equatable {
    case researching
    case writingArtifact
    case implementing
    case awaitingDirection
    case other(String)

    init(rawValue: String) {
      switch rawValue.lowercased() {
      case "researching":
        self = .researching
      case "writing_artifact":
        self = .writingArtifact
      case "implementing":
        self = .implementing
      case "awaiting_direction":
        self = .awaitingDirection
      default:
        self = .other(rawValue)
      }
    }

    var label: String {
      switch self {
      case .researching:
        return "Working"
      case .writingArtifact:
        return "Writing"
      case .implementing:
        return "Implementing"
      case .awaitingDirection:
        return "Awaiting Direction"
      case .other(let rawValue):
        return rawValue.capitalized
      }
    }
  }

  struct StreamingProgress: Sendable, Equatable {
    let kind: StreamingProgressKind
    let message: String
  }

  struct CompactionBannerInfo: Equatable {
    let message: String
    let isEmergency: Bool
  }

  private struct SessionStreamingState {
    var text = ""
    var activityNarration = ""
    var responsePreviewText = ""
    var completedSummaryText: String?
    var hasTypedActivityEvents = false
    var hasTypedFinalAnswerEvents = false
    var phase: StreamingPhase?
    var transcriptPhase: TranscriptPhaseBoundary?
    var progress: StreamingProgress?
    var startedAt = Date()
    var modelID: String?
  }

  private struct CompletedStreamSummary: Equatable {
    let messageID: String
    let contentDigest: String
    let terminalTextDigest: String
    let elapsedText: String
    let summaryText: String?
    let activityGroups: [ToolActivityGroupRecord]
    let entries: [CompletedWorkEntry]
  }

  private static let sessionLoadDebounceMs = 50
  static let permissionPromptTimeoutSeconds = 60
  private static let permissionPromptTimeout: Duration = .seconds(permissionPromptTimeoutSeconds)
  private static let compactionBannerTimeout: Duration = .seconds(4)
  static let maxCachedSessions = 10
  private static let maxRetiredLiveToolGroupsPerSession = 64
  private static let liveActivityIDPrefix = "__fawx_live_activity__"
  private static let liveCompletedSummaryIDPrefix = "__fawx_live_completed_summary__"
  private static let liveFinalAnswerIDPrefix = "__fawx_live_final_answer__"

  private(set) var transcriptUpdateID = 0
  var transcriptItems: [ChatTranscriptItem] = [] {
    didSet {
      if oldValue != transcriptItems {
        transcriptUpdateID &+= 1
      }
    }
  }
  var transcriptTurns: [TranscriptTurn] {
    transcriptItems.transcriptTurns()
  }
  private var draftsBySession: [String: String] = [:]
  private var pendingAttachmentsBySession: [String: [PendingAttachment]] = [:]
  private var steeringDraftsBySession: [String: String] = [:]
  private var queuedDraftsBySession: [String: QueuedDraft] = [:]
  var isLoadingHistory = false
  var isStreaming: Bool {
    !streamStates.isEmpty
  }
  var isUpdatingThreadRuntimeSettings = false
  var selectedThreadModel: ModelInfo? {
    modelInfo(for: selectedThreadModelID)
  }
  var selectedThreadThinkingLevel: ThinkingLevel? {
    let levels = selectedThreadThinkingLevels
    if let currentSessionID,
       let sessionLevel = sessionViewModel.thinkingLevel(for: currentSessionID),
       levels.contains(sessionLevel) {
      return sessionLevel
    }
    if currentSessionID == nil,
       let draftThreadThinkingLevel,
       levels.contains(draftThreadThinkingLevel) {
      return draftThreadThinkingLevel
    }
    if let appLevel = appState.thinkingLevel,
       levels.contains(appLevel) {
      return appLevel
    }
    return ThinkingLevel.disabledLevel(in: levels) ?? levels.first
  }
  var selectedThreadThinkingLevels: [ThinkingLevel] {
    let modelLevels = selectedThreadModel?.thinkingLevels ?? []
    return modelLevels.isEmpty ? appState.availableThinkingLevels : modelLevels
  }
  var selectedThreadModelID: String? {
    if let currentSessionID {
      return sessionViewModel.modelID(for: currentSessionID)
        ?? appState.activeModel?.modelID
    }

    return draftThreadModelID ?? appState.activeModel?.modelID
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
  private var liveToolGroupsBySession: [String: [ToolActivityGroupRecord]] = [:]
  private var liveNarrationByActivityIDBySession: [String: [String: WorkingNarrationRecord]] = [:]
  private var liveActivityOrderBySession: [String: [String]] = [:]
  private var retiredLiveToolGroupsBySession: [String: [ToolActivityGroupRecord]] = [:]
  private var retiredNarrationByActivityIDBySession: [String: [String: WorkingNarrationRecord]] = [:]
  private var anonymousToolCallCountersBySession: [String: Int] = [:]
  private var queuedPermissionPrompts: [PermissionPrompt] = []
  private var streamStates: [String: SessionStreamingState] = [:]
  private var draftThreadModelID: String?
  private var completedStreamSummariesBySession: [String: [CompletedStreamSummary]] = [:]
  // Accepted steering is current-turn UI state only. It is intentionally
  // in-memory and drops on app restart because replaying old steering into a
  // later turn would misrepresent what the running agent actually received.
  private var acceptedSteeringRecordsBySession: [String: [AcceptedSteeringRecord]] = [:]
  private var compactionBannerInfosBySession: [String: CompactionBannerInfo] = [:]
  @ObservationIgnored private var historyLoadSequence = 0
  @ObservationIgnored private var acceptedSteeringRecordSequence: UInt64 = 0
  @ObservationIgnored private var transcriptCache: [String: [SessionMessage]] = [:]
  @ObservationIgnored private var transcriptCacheAccessOrder: [String] = []
  @ObservationIgnored private var permissionPromptTimeoutTask: Task<Void, Never>?
  @ObservationIgnored private var streamTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var streamingDisplayControllers:
    [String: StreamingDisplayController] = [:]
  @ObservationIgnored private var compactionBannerDismissTasks: [String: Task<Void, Never>] = [:]
  private var draftThreadThinkingLevel: ThinkingLevel?
  private var sessionLoadTask: Task<Void, Never>?

  init(
    appState: AppState,
    sessionViewModel: SessionViewModel,
    compactionBannerSleepHandler: @escaping @Sendable (Duration) async throws -> Void = {
      duration in
      try await Task.sleep(for: duration)
    }
  ) {
    self.appState = appState
    self.sessionViewModel = sessionViewModel
    self.compactionBannerSleepHandler = compactionBannerSleepHandler
    syncThreadRuntimeActivity()
  }

  private func modelInfo(for modelID: String?) -> ModelInfo? {
    guard let modelID else {
      return appState.activeModel
    }

    // Display-only fallback for sessions created with a model that is no longer in
    // the current catalog. Do not use this synthetic value for authentication.
    return appState.availableModels.first { $0.modelID == modelID }
      ?? ModelInfo(
        modelID: modelID,
        provider: "Thread",
        authMethod: "session",
        displayName: nil,
        recommended: false
      )
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

  var draftSteering: String {
    get {
      if let currentSessionID, let queuedDraft = queuedDraftsBySession[currentSessionID] {
        return queuedDraft.steering ?? ""
      }
      return steeringDraftsBySession[draftStorageKey(for: currentSessionID)] ?? ""
    }
    set {
      if let currentSessionID, let queuedDraft = queuedDraftsBySession[currentSessionID] {
        queuedDraftsBySession[currentSessionID] = QueuedDraft(
          text: queuedDraft.text,
          attachments: queuedDraft.attachments,
          steering: newValue.isEmpty ? nil : newValue,
          disposition: queuedDraft.disposition
        )
        return
      }

      let key = draftStorageKey(for: currentSessionID)
      if newValue.isEmpty {
        steeringDraftsBySession.removeValue(forKey: key)
      } else {
        steeringDraftsBySession[key] = newValue
      }
    }
  }

  var queuedMessage: String? {
    guard let currentSessionID else {
      return nil
    }
    return queuedDraftsBySession[currentSessionID]?.summaryText
  }

  var queuedMessageIsSteering: Bool {
    guard let currentSessionID else {
      return false
    }
    return queuedDraftsBySession[currentSessionID]?.disposition == .steering
  }

  var queuedMessageCanSteer: Bool {
    guard let currentSessionID, let queuedDraft = queuedDraftsBySession[currentSessionID] else {
      return false
    }
    return !queuedDraft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && queuedDraft.attachments.isEmpty
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

  var isCurrentSessionStreamingFinalResponse: Bool {
    guard let currentSessionID,
          isCurrentSessionStreaming,
          let streamState = streamStates[currentSessionID]
    else {
      return false
    }

    return streamState.hasTypedFinalAnswerEvents
      || !streamState.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      || streamState.transcriptPhase?.allowsOpenEndedFinalAnswer == true
  }

  var visibleCurrentPhase: StreamingPhase? {
    guard let currentSessionID, isCurrentSessionStreaming else {
      return nil
    }

    return streamStates[currentSessionID]?.phase
  }

  var visibleProgress: StreamingProgress? {
    guard let currentSessionID, isCurrentSessionStreaming else {
      return nil
    }

    return streamStates[currentSessionID]?.progress
  }

  var visibleStreamingStartedAt: Date? {
    guard let currentSessionID, isCurrentSessionStreaming else {
      return nil
    }

    return streamStates[currentSessionID]?.startedAt
  }

  func visibleStreamingElapsedText(now: Date = Date()) -> String? {
    guard let startedAt = visibleStreamingStartedAt else {
      return nil
    }

    return streamingElapsedFootnoteText(
      startedAt: startedAt,
      endedAt: now,
      minimumSeconds: 15
    )
  }

  private func streamingElapsedString(_ seconds: Int) -> String {
    Self.elapsedDurationFormatter.string(from: TimeInterval(seconds))
      ?? "\(seconds) seconds"
  }

  private func streamingElapsedFootnoteText(
    startedAt: Date,
    endedAt: Date,
    minimumSeconds: Int
  ) -> String? {
    let elapsed = max(0, Int(endedAt.timeIntervalSince(startedAt)))
    guard elapsed >= minimumSeconds else {
      return nil
    }

    return "Worked for \(streamingElapsedString(elapsed))"
  }

  var shouldAutoScrollStreamingUpdates: Bool {
    guard let currentSessionID, isCurrentSessionStreaming else {
      return true
    }

    return streamingDisplayController(for: currentSessionID).isPinnedToBottom
  }

  var composerPhaseLabel: String? {
    if let visibleProgress {
      return visibleProgress.kind.label
    }

    if let visibleCurrentPhase {
      return visibleCurrentPhase.composerLabel
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
    liveNarrationByActivityIDBySession.removeValue(forKey: sessionID)
    liveActivityOrderBySession.removeValue(forKey: sessionID)
    retiredLiveToolGroupsBySession.removeValue(forKey: sessionID)
    retiredNarrationByActivityIDBySession.removeValue(forKey: sessionID)
    anonymousToolCallCountersBySession.removeValue(forKey: sessionID)
    draftsBySession.removeValue(forKey: draftStorageKey(for: sessionID))
    pendingAttachmentsBySession.removeValue(forKey: draftStorageKey(for: sessionID))
    steeringDraftsBySession.removeValue(forKey: draftStorageKey(for: sessionID))
    queuedDraftsBySession.removeValue(forKey: sessionID)
    errorMessagesBySession.removeValue(forKey: errorStorageKey(for: sessionID))
    retryRequestsBySession.removeValue(forKey: sessionID)
    clearCompactionBanner(for: sessionID)

    if currentSessionID == sessionID {
      transcriptItems = []
      pendingTranscriptScrollBehavior = .snap
    }

    syncThreadRuntimeActivity()
  }

  func prepareToDisplaySession(_ sessionID: String?) {
    pendingTranscriptScrollBehavior = .snap
    currentSessionID = sessionID
    if sessionID != nil {
      draftThreadModelID = nil
      draftThreadThinkingLevel = nil
    }
    appState.selectContextSession(sessionID)

    guard let sessionID else {
      transcriptItems = []
      isLoadingHistory = false
      return
    }

    if let cachedMessages = cachedMessages(for: sessionID) {
      transcriptItems = makeTranscriptItems(for: sessionID, messages: cachedMessages)
      isLoadingHistory = false
    } else {
      transcriptItems = transcriptItemsWithLiveToolActivity(for: sessionID)
      isLoadingHistory = isSessionStreaming(sessionID) ? false : true
    }

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
    appState.selectContextSession(sessionID)
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
      appState.selectContextSession(sessionID)
      pendingTranscriptScrollBehavior = .snap
    } else {
      cleanup()
      currentSessionID = sessionID
      appState.selectContextSession(sessionID)
      if let sessionID {
        retryRequestsBySession.removeValue(forKey: sessionID)
        acceptedSteeringRecordsBySession.removeValue(forKey: sessionID)
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

    if let preferredFetchedMessages = replaceOptimisticAssistantTailIfServerCompletedTurn(
      localMessages: existingMessages,
      fetchedMessages: fetchedMessages
    ) {
      return preferredFetchedMessages
    }

    return mergeFetchedMessagesPreservingRelativeOrder(
      localMessages: existingMessages,
      fetchedMessages: fetchedMessages
    )
  }

  private func replaceOptimisticAssistantTailIfServerCompletedTurn(
    localMessages: [SessionMessage],
    fetchedMessages: [SessionMessage]
  ) -> [SessionMessage]? {
    guard
      let localLastUserIndex = localMessages.lastIndex(where: { $0.role == .user }),
      let fetchedLastUserIndex = fetchedMessages.lastIndex(where: { $0.role == .user })
    else {
      return nil
    }

    let localPrefix = Array(localMessages.prefix(through: localLastUserIndex))
    let fetchedPrefix = Array(fetchedMessages.prefix(through: fetchedLastUserIndex))
    guard areEquivalentMessageSequences(localPrefix, fetchedPrefix) else {
      return nil
    }

    let localTail = Array(
      localMessages.suffix(from: localMessages.index(after: localLastUserIndex)))
    let fetchedTail = Array(
      fetchedMessages.suffix(from: fetchedMessages.index(after: fetchedLastUserIndex)))
    guard localTail.count == 1, !fetchedTail.isEmpty else {
      return nil
    }

    guard localTail.allSatisfy(isPlainAssistantTailMessage) else {
      return nil
    }

    guard fetchedTailRepresentsCompletedAssistantTurn(fetchedTail) else {
      return nil
    }

    return fetchedMessages
  }

  private func fetchedTailRepresentsCompletedAssistantTurn(_ messages: [SessionMessage]) -> Bool {
    guard let finalMessage = messages.last, isPlainAssistantTailMessage(finalMessage) else {
      return false
    }

    return messages.allSatisfy { message in
      switch message.role {
      case .assistant:
        return true
      case .tool:
        return message.contentBlocks.contains(where: \.containsToolResult)
      case .user, .system:
        return false
      }
    }
  }

  private func isPlainAssistantTailMessage(_ message: SessionMessage) -> Bool {
    guard message.role == .assistant else {
      return false
    }

    let displayText = message.transcriptDisplayText.trimmingCharacters(in: .whitespacesAndNewlines)
    return !displayText.isEmpty && toolCalls(from: message).isEmpty
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
      if let invertedBoundary = invertedFetchedTurnBoundary(
        localMessages: localMessages,
        startingAt: nextLocalIndex,
        fetchedMessages: fetchedMessages,
        startingAt: nextFetchedIndex
      ) {
        if nextFetchedIndex < invertedBoundary.fetchedQueuedUserIndex {
          mergedMessages.append(
            contentsOf: fetchedMessages[nextFetchedIndex..<invertedBoundary.fetchedQueuedUserIndex]
          )
        }

        mergedMessages.append(fetchedMessages[invertedBoundary.fetchedAssistantIndex])

        let middleStartIndex = fetchedMessages.index(after: invertedBoundary.fetchedQueuedUserIndex)
        if middleStartIndex < invertedBoundary.fetchedAssistantIndex {
          mergedMessages.append(
            contentsOf: fetchedMessages[middleStartIndex..<invertedBoundary.fetchedAssistantIndex]
          )
        }

        mergedMessages.append(fetchedMessages[invertedBoundary.fetchedQueuedUserIndex])
        nextLocalIndex = localMessages.index(after: invertedBoundary.localQueuedUserIndex)
        nextFetchedIndex = fetchedMessages.index(after: invertedBoundary.fetchedAssistantIndex)
        continue
      }

      if messagesAreEquivalentForTranscriptMerge(
        localMessages[nextLocalIndex],
        fetchedMessages[nextFetchedIndex]
      ) {
        mergedMessages.append(fetchedMessages[nextFetchedIndex])
        nextLocalIndex += 1
        nextFetchedIndex += 1
        continue
      }

      guard
        let alignment = nextEquivalentMessageAlignment(
          localMessages: localMessages,
          startingAt: nextLocalIndex,
          fetchedMessages: fetchedMessages,
          startingAt: nextFetchedIndex
        )
      else {
        mergedMessages.append(contentsOf: fetchedMessages[nextFetchedIndex...])
        mergedMessages.append(contentsOf: localMessages[nextLocalIndex...])
        return mergedMessages
      }

      if nextFetchedIndex < alignment.fetchedIndex {
        mergedMessages.append(
          contentsOf: fetchedMessages[nextFetchedIndex..<alignment.fetchedIndex])
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

  private func invertedFetchedTurnBoundary(
    localMessages: [SessionMessage],
    startingAt localStartIndex: Int,
    fetchedMessages: [SessionMessage],
    startingAt fetchedStartIndex: Int
  ) -> (localQueuedUserIndex: Int, fetchedQueuedUserIndex: Int, fetchedAssistantIndex: Int)? {
    let localQueuedUserIndex = localMessages.index(after: localStartIndex)
    guard
      localQueuedUserIndex < localMessages.endIndex,
      localMessages[localStartIndex].role == .assistant,
      localMessages[localQueuedUserIndex].role == .user,
      let fetchedQueuedUserIndex = indexOfEquivalentMessage(
        localMessages[localQueuedUserIndex],
        in: fetchedMessages,
        startingAt: fetchedStartIndex
      ),
      let fetchedAssistantIndex = indexOfEquivalentMessage(
        localMessages[localStartIndex],
        in: fetchedMessages,
        startingAt: fetchedStartIndex
      ),
      fetchedQueuedUserIndex < fetchedAssistantIndex
    else {
      return nil
    }

    return (localQueuedUserIndex, fetchedQueuedUserIndex, fetchedAssistantIndex)
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
      guard
        let fetchedIndex = indexOfEquivalentMessage(
          localMessages[localIndex],
          in: fetchedMessages,
          startingAt: fetchedStartIndex
        )
      else {
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

    for index in startIndex..<messages.count
    where messagesAreEquivalentForTranscriptMerge(message, messages[index]) {
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

  func selectModelForCurrentThread(_ modelID: String) async {
    let normalizedModelID = modelID.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedModelID.isEmpty else {
      assertionFailure("Attempted to select an empty thread model ID.")
      return
    }

    guard let currentSessionID else {
      draftThreadModelID = normalizedModelID
      return
    }

    guard !activeStreamSessionIDs.contains(currentSessionID) else {
      appState.showToast(
        message: "Stop this response before changing the thread model.",
        style: .warning
      )
      return
    }

    isUpdatingThreadRuntimeSettings = true
    defer { isUpdatingThreadRuntimeSettings = false }

    if await sessionViewModel.updateModel(for: currentSessionID, modelID: normalizedModelID) {
      clearErrorMessage(for: currentSessionID)
    } else {
      setErrorMessage("Failed to update thread model.", for: currentSessionID)
    }
  }

  func selectThinkingForCurrentThread(_ level: ThinkingLevel) async {
    guard selectedThreadThinkingLevels.contains(level) else {
      assertionFailure("Attempted to select a thinking level unsupported by the selected thread model.")
      return
    }

    guard let currentSessionID else {
      draftThreadThinkingLevel = level
      return
    }

    guard !activeStreamSessionIDs.contains(currentSessionID) else {
      appState.showToast(
        message: "Stop this response before changing the thread thinking level.",
        style: .warning
      )
      return
    }

    isUpdatingThreadRuntimeSettings = true
    defer { isUpdatingThreadRuntimeSettings = false }

    if await sessionViewModel.updateThinking(for: currentSessionID, level: level) {
      clearErrorMessage(for: currentSessionID)
    } else {
      setErrorMessage("Failed to update thread thinking level.", for: currentSessionID)
    }
  }

  private func appendPendingAttachment(_ attachment: PendingAttachment) throws {
    pendingAttachments = try AttachmentComposer.append([attachment], to: pendingAttachments)
  }

  func sendDraft() {
    let trimmed = draftMessage.trimmingCharacters(in: .whitespacesAndNewlines)
    let attachments = pendingAttachments
    let steering = normalizedTurnSteering(draftSteering)
    guard !trimmed.isEmpty || !attachments.isEmpty else {
      return
    }

    guard appState.connectionStatus == .connected else {
      errorMessage = "Reconnecting to Fawx. Try sending again once the connection is restored."
      return
    }

    draftMessage = ""
    pendingAttachments = []
    draftSteering = ""

    if isCurrentSessionStreaming, let currentSessionID {
      queuedDraftsBySession[currentSessionID] = QueuedDraft(
        text: trimmed,
        attachments: attachments,
        steering: steering,
        disposition: .message
      )
      return
    }

    Task {
      await send(trimmed, attachments: attachments, steering: steering)
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
        steering: retryRequest.steering,
        forceSessionID: retryRequest.sessionID
      )
    }
  }

  func dismissQueuedMessage() {
    clearQueuedMessage(for: currentSessionID)
  }

  func toggleQueuedMessageSteering() {
    guard let currentSessionID, let queuedDraft = queuedDraftsBySession[currentSessionID] else {
      return
    }
    guard !queuedDraft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
          queuedDraft.attachments.isEmpty else {
      return
    }

    switch queuedDraft.disposition {
    case .message:
      queuedDraftsBySession[currentSessionID] = QueuedDraft(
        text: queuedDraft.text,
        attachments: queuedDraft.attachments,
        steering: queuedDraft.steering,
        disposition: .steering
      )
      Task {
        await sendQueuedSteering(queuedDraft.text, sessionID: currentSessionID)
      }
    case .steering:
      queuedDraftsBySession[currentSessionID] = QueuedDraft(
        text: queuedDraft.text,
        attachments: queuedDraft.attachments,
        steering: queuedDraft.steering,
        disposition: .message
      )
    }
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
    requestServerStop(for: sessionID)
    streamTasks[sessionID]?.cancel()
    Task { [weak self] in
      await self?.finalizeCancellation(
        timestamp: Int(Date().timeIntervalSince1970),
        sessionID: sessionID
      )
    }
  }

  private func requestServerStop(for sessionID: String) {
    let client = appState.client
    Task { [weak self, client] in
      do {
        _ = try await client.stopSession(id: sessionID)
      } catch is CancellationError {
        return
      } catch {
        await self?.showServerStopFailure(error)
      }
    }
  }

  private func showServerStopFailure(_ error: Error) async {
    appState.showToast(
      message: "Couldn't stop the response on the server. The local stream was closed.",
      style: .error
    )
    await appState.noteRecoverableRequestFailure(error)
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
    steering: String? = nil,
    forceSessionID: String? = nil
  ) async {
    await appState.synchronizeLocalConnectionIfNeeded()
    var targetSessionID = forceSessionID ?? currentSessionID
    let requestedModelID = selectedThreadModelID
    setErrorMessage(nil, for: targetSessionID)
    let payload = AttachmentComposer.prepareMessage(message: text, attachments: attachments)

    if targetSessionID == nil {
      let requestedThinkingLevel = draftThreadThinkingLevel
      if let createdSessionID = await sessionViewModel.createNewThread(
        in: sessionViewModel.selectedWorkspaceID,
        modelID: requestedModelID,
        thinkingLevel: requestedThinkingLevel)
      {
        currentSessionID = createdSessionID
        appState.selectContextSession(createdSessionID)
        targetSessionID = createdSessionID
        draftThreadModelID = nil
        draftThreadThinkingLevel = nil
      } else {
        draftMessage = text
        pendingAttachments = attachments
        draftSteering = steering ?? ""
        setErrorMessage("Failed to create thread.", for: nil)
        return
      }
    }

    guard let sessionID = targetSessionID else {
      setErrorMessage("No session available.", for: forceSessionID ?? currentSessionID)
      return
    }
    let sessionModelID = sessionViewModel.modelID(for: sessionID) ?? requestedModelID

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
      model: sessionModelID
    )
    retryRequestsBySession[sessionID] = RetryRequest(
      text: text,
      attachments: attachments,
      steering: steering,
      sessionID: sessionID
    )

    do {
      let stream = try await appState.client.sendMessageStream(
        sessionID: sessionID,
        message: payload.message,
        images: payload.images,
        documents: payload.documents,
        steering: steering
      )
      startStreaming(
        stream,
        sessionID: sessionID,
        modelID: sessionModelID,
        retryText: text,
        retryAttachments: attachments,
        retrySteering: steering
      )
    } catch {
      setErrorMessage("Failed to send message. \(error.localizedDescription)", for: sessionID)
    }
  }

  private func startStreaming(
    _ stream: AsyncThrowingStream<SSEEvent, Error>,
    sessionID: String,
    modelID: String?,
    retryText: String,
    retryAttachments: [PendingAttachment],
    retrySteering: String?
  ) {
    // sendMessageStream has already opened the server-side run by the time we get here.
    // Only explicit user/cleanup stop paths should call /stop; startup must only bind local UI state.
    streamStates[sessionID] = SessionStreamingState(
      text: "",
      phase: nil,
      progress: nil,
      startedAt: Date(),
      modelID: modelID
    )
    syncThreadRuntimeActivity()
    streamingDisplayController(for: sessionID).reset(repinToBottom: true)
    liveNarrationByActivityIDBySession[sessionID] = nil
    liveActivityOrderBySession[sessionID] = nil

    let task = Task {
      var finalResponse: String?
      var streamFailed = false

      do {
        streamLoop: for try await event in stream {
          switch await reduceStreamEvent(event, sessionID: sessionID) {
          case .continue:
            continue
          case .finish(let response):
            finalResponse = response
          case .fail:
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
              steering: retrySteering,
              sessionID: sessionID
            )
            await finalizeCancellation(
              timestamp: Int(Date().timeIntervalSince1970),
              sessionID: sessionID
            )
          }
        } else {
          await finalizeStream(
            timestamp: Int(Date().timeIntervalSince1970),
            finalResponse: finalResponse,
            sessionID: sessionID
          )
        }
      } catch is CancellationError {
        await finalizeCancellation(
          timestamp: Int(Date().timeIntervalSince1970),
          sessionID: sessionID
        )
      } catch {
        handleStreamError(
          "Response interrupted. \(error.localizedDescription)", sessionID: sessionID)
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
            steering: retrySteering,
            sessionID: sessionID
          )
          await finalizeCancellation(
            timestamp: Int(Date().timeIntervalSince1970),
            sessionID: sessionID
          )
        }
      }
    }

    streamTasks[sessionID] = task
  }

  private func assistantMessageTimestamp(startedAt: Date?, fallbackUnixTimestamp: Int) -> Int {
    guard let startedAt else {
      return fallbackUnixTimestamp
    }

    let startedTimestamp = Int(startedAt.timeIntervalSince1970.rounded(.down))
    return startedTimestamp > 0 ? startedTimestamp : fallbackUnixTimestamp
  }

  private func finalizeStream(timestamp: Int, finalResponse: String?, sessionID: String) async {
    streamingDisplayController(for: sessionID).streamDidEnd()
    let startedAt = streamStates[sessionID]?.startedAt
    let modelID = streamStates[sessionID]?.modelID ?? sessionViewModel.modelID(for: sessionID)
    let endedAt = Date(timeIntervalSince1970: TimeInterval(timestamp))
    let content = finalResponse ?? streamingText(for: sessionID)
    if !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
      removePromotedPreviewNarration(content, for: sessionID)
      let assistantMessage = SessionMessage(
        role: .assistant,
        content: content,
        timestamp: assistantMessageTimestamp(
          startedAt: startedAt,
          fallbackUnixTimestamp: timestamp
        )
      )
      recordCompletedStreamingFootnote(
        startedAt: startedAt,
        endedAt: endedAt,
        for: assistantMessage,
        sessionID: sessionID
      )
      appendMessage(assistantMessage, for: sessionID)
      sessionViewModel.updatePreview(
        for: sessionID, text: content, model: modelID)
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
    guard streamStates[sessionID] != nil || streamTasks[sessionID] != nil else {
      return
    }

    streamingDisplayController(for: sessionID).streamDidEnd()
    let startedAt = streamStates[sessionID]?.startedAt
    let modelID = streamStates[sessionID]?.modelID ?? sessionViewModel.modelID(for: sessionID)
    let endedAt = Date(timeIntervalSince1970: TimeInterval(timestamp))
    let currentStreamingText = streamingText(for: sessionID)
    if !currentStreamingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
      let interrupted = currentStreamingText + "\n\n(interrupted)"
      let assistantMessage = SessionMessage(
        role: .assistant,
        content: interrupted,
        timestamp: assistantMessageTimestamp(
          startedAt: startedAt,
          fallbackUnixTimestamp: timestamp
        )
      )
      recordCompletedStreamingFootnote(
        startedAt: startedAt,
        endedAt: endedAt,
        for: assistantMessage,
        sessionID: sessionID
      )
      appendMessage(assistantMessage, for: sessionID)
      sessionViewModel.updatePreview(
        for: sessionID, text: interrupted, model: modelID)
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

  private func normalizedTurnSteering(_ value: String) -> String? {
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
  }

  private func sendQueuedSteering(_ text: String, sessionID: String) async {
    do {
      let response = try await appState.client.steerSession(id: sessionID, text: text)
      guard response.steered else {
        guard isQueuedSteeringDraft(text, sessionID: sessionID) else {
          return
        }
        appState.showToast(
          message: steerRejectedToastMessage(reason: response.reason),
          style: .warning
        )
        restoreQueuedMessageDisposition(
          .message,
          sessionID: sessionID,
          ifCurrentDisposition: .steering,
          matchingText: text
        )
        return
      }
      clearErrorMessage(for: sessionID)
      recordAcceptedSteering(text, sessionID: sessionID)
      clearAcceptedQueuedSteering(text, sessionID: sessionID)
    } catch {
      guard isQueuedSteeringDraft(text, sessionID: sessionID) else {
        return
      }
      await appState.noteRecoverableRequestFailure(error)
      appState.showToast(message: "Failed to steer the active turn.", style: .warning)
      restoreQueuedMessageDisposition(
        .message,
        sessionID: sessionID,
        ifCurrentDisposition: .steering,
        matchingText: text
      )
    }
  }

  private func steerRejectedToastMessage(reason: String?) -> String {
    guard let reason, !reason.isEmpty else {
      return "No active turn accepted steering. It will remain queued."
    }
    if reason == "no_active_run" {
      return "That turn already finished. Your note will remain queued."
    }

    return "No active turn accepted steering (\(reason)). It will remain queued."
  }

  private func recordAcceptedSteering(_ text: String, sessionID: String) {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
      return
    }

    let timestamp = Int(Date().timeIntervalSince1970)
    acceptedSteeringRecordSequence += 1
    let id = [
      String(timestamp),
      String(acceptedSteeringRecordSequence),
      Self.stableDigest(for: trimmed),
    ].joined(separator: ":")
    let acceptedRecord = AcceptedSteeringRecord(
      record: TurnSteeringRecord(
        id: id,
        text: trimmed,
        timestamp: timestamp
      ),
      anchor: acceptedSteeringAnchor(for: sessionID)
    )

    var records = acceptedSteeringRecordsBySession[sessionID, default: []]
    records.append(acceptedRecord)
    acceptedSteeringRecordsBySession[sessionID] = Array(records.suffix(16))

    guard currentSessionID == sessionID else {
      return
    }
    let messages = cachedMessages(for: sessionID) ?? []
    transcriptItems = makeTranscriptItems(for: sessionID, messages: messages)
  }

  private func clearAcceptedQueuedSteering(_ text: String, sessionID: String) {
    guard
      let queuedDraft = queuedDraftsBySession[sessionID],
      queuedDraft.disposition == .steering,
      queuedDraft.text.trimmingCharacters(in: .whitespacesAndNewlines)
        == text.trimmingCharacters(in: .whitespacesAndNewlines)
    else {
      return
    }

    queuedDraftsBySession.removeValue(forKey: sessionID)
  }

  private func acceptedSteeringAnchor(for sessionID: String) -> AcceptedSteeringAnchor {
    if let liveItemAnchor = liveTurnSteeringAnchorItemID(for: sessionID) {
      return .afterItem(liveItemAnchor)
    }

    if let messages = cachedMessages(for: sessionID), !messages.isEmpty {
      var occurrenceCounts: [String: Int] = [:]
      var lastAnchorID: String?
      for message in messages {
        lastAnchorID = messageAnchorID(for: message, occurrenceCounts: &occurrenceCounts)
      }

      return lastAnchorID.map(AcceptedSteeringAnchor.afterMessage) ?? .sessionStart
    }

    if currentSessionID == sessionID {
      // If the history cache was evicted but the visible transcript is still
      // present, anchor steering after the last rendered message instead of
      // jumping it to the top of the transcript.
      for item in transcriptItems.reversed() {
        if let message = item.transcriptMessage {
          return .afterMessage(message.id)
        }
      }
    }

    return .sessionStart
  }

  private func liveTurnSteeringAnchorItemID(for sessionID: String) -> String? {
    guard currentSessionID == sessionID,
          isSessionStreaming(sessionID) || !(liveToolGroupsBySession[sessionID]?.isEmpty ?? true)
    else {
      return nil
    }

    for item in transcriptItems.reversed() {
      if let message = item.sessionMessage,
         message.role == .user || message.role == .system {
        return nil
      }

      switch item.phase {
      case .workingNarration, .toolGroup, .turnSteering:
        return item.id
      case .message, .completedSummary, .finalAnswer:
        continue
      }
    }

    return nil
  }

  private func restoreQueuedMessageDisposition(
    _ disposition: QueuedDraftDisposition,
    sessionID: String,
    ifCurrentDisposition currentDisposition: QueuedDraftDisposition? = nil,
    matchingText text: String? = nil
  ) {
    guard let queuedDraft = queuedDraftsBySession[sessionID] else {
      return
    }
    if let currentDisposition, queuedDraft.disposition != currentDisposition {
      return
    }
    if let text,
       queuedDraft.text.trimmingCharacters(in: .whitespacesAndNewlines)
       != text.trimmingCharacters(in: .whitespacesAndNewlines) {
      return
    }
    queuedDraftsBySession[sessionID] = QueuedDraft(
      text: queuedDraft.text,
      attachments: queuedDraft.attachments,
      steering: queuedDraft.steering,
      disposition: disposition
    )
  }

  private func isQueuedSteeringDraft(_ text: String, sessionID: String) -> Bool {
    guard let queuedDraft = queuedDraftsBySession[sessionID],
          queuedDraft.disposition == .steering else {
      return false
    }

    return queuedDraft.text.trimmingCharacters(in: .whitespacesAndNewlines)
      == text.trimmingCharacters(in: .whitespacesAndNewlines)
  }

  private func sendQueuedMessageIfNeeded(finishedSessionID: String) async {
    guard let queuedDelivery = consumeQueuedMessageIfReady(finishedSessionID: finishedSessionID)
    else {
      return
    }

    await send(
      queuedDelivery.text,
      attachments: queuedDelivery.attachments,
      steering: queuedDelivery.steering,
      forceSessionID: queuedDelivery.sessionID
    )
  }

  private func resetStreamingState(for sessionID: String) {
    streamTasks.removeValue(forKey: sessionID)
    persistCurrentActivityNarration(for: sessionID)
    streamStates.removeValue(forKey: sessionID)
    streamingDisplayControllers[sessionID]?.reset(repinToBottom: false)
    streamingDisplayControllers.removeValue(forKey: sessionID)
    retireLiveToolActivity(for: sessionID)
    retiredLiveToolGroupsBySession.removeValue(forKey: sessionID)
    retiredNarrationByActivityIDBySession.removeValue(forKey: sessionID)
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: true
    )
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

    let updatedContext =
      (appState.currentContext
      ?? ContextInfo(
        usedTokens: tokensAfter,
        maxTokens: 0,
        percentage: usageRatio,
        compactionThreshold: 0
      )).applyingCompaction(usedTokens: tokensAfter, usageRatio: usageRatio)

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

    appState.setContext(updatedContext, for: sessionID)
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
    let compactedMessageCount =
      messagesRemoved == 1
      ? "1 message compacted"
      : "\(messagesRemoved) messages compacted"

    return CompactionBannerInfo(
      message:
        "\(prefix): \(compactedMessageCount), \(Int(beforePercentage.rounded()))% → \(Int(afterPercentage.rounded()))%",
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

  private func dismissCompactionBannerIfNeeded(
    expectedInfo: CompactionBannerInfo, sessionID: String
  ) {
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
  ) -> (text: String, attachments: [PendingAttachment], steering: String?, sessionID: String?)? {
    guard let queuedDraft = queuedDraftsBySession[finishedSessionID] else {
      return nil
    }
    guard appState.connectionStatus == .connected else {
      return nil
    }

    queuedDraftsBySession.removeValue(forKey: finishedSessionID)
    guard queuedDraft.disposition == .message else {
      return nil
    }
    return (queuedDraft.text, queuedDraft.attachments, queuedDraft.steering, finishedSessionID)
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
    } catch let apiError as APIError where apiError.statusCode == 404 || apiError.statusCode == 409
    {
      finishActivePermissionPrompt(id: prompt.id)

      if isAutomatic {
        appState.showToast(message: "Approval request expired.", style: .warning)
      }
    } catch {
      isRespondingToPermissionPrompt = false
      permissionPromptErrorMessage =
        "Couldn't send approval response. \(error.localizedDescription)"

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
    syncThreadRuntimeActivity()
  }

  private func setTranscriptPhaseBoundary(_ phase: TranscriptPhaseBoundary?, for sessionID: String) {
    guard streamStates[sessionID] != nil else {
      return
    }

    streamStates[sessionID]?.transcriptPhase = phase
    if phase?.allowsOpenEndedFinalAnswer == true {
      streamStates[sessionID]?.progress = nil
      refreshVisibleTranscriptForToolActivity(
        sessionID: sessionID,
        preferPreservePosition: shouldPreserveScrollPosition(for: sessionID),
        animatedWhenFollowing: false
      )
    }
    syncThreadRuntimeActivity()
  }

  private func setStreamingProgress(_ progress: StreamingProgress?, for sessionID: String) {
    guard streamStates[sessionID] != nil else {
      return
    }

    streamStates[sessionID]?.progress = progress
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func appendStreamingText(_ text: String, for sessionID: String) {
    guard !text.isEmpty, streamStates[sessionID] != nil else {
      return
    }

    streamStates[sessionID]?.text += text
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID),
      animatedWhenFollowing: false
    )
  }

  private func reduceStreamEvent(
    _ event: SSEEvent,
    sessionID: String
  ) async -> StreamReductionResult {
    switch event {
    case .workingNarrationDelta(let delta):
      let text = delta.text
      removeResponsePreviewIfPromotedToNarration(text, for: sessionID)
      let wasRenderedByLegacyPreview =
        streamStates[sessionID]?.hasTypedActivityEvents != true
        && streamStates[sessionID]?.activityNarration.hasSuffix(text) == true
      markTypedActivityEvents(for: sessionID)
      if !delta.voiceoverSuppressed && !wasRenderedByLegacyPreview {
        appendActivityNarration(text, for: sessionID)
      }
    case .textPreviewDelta(let text):
      appendResponsePreviewText(text, for: sessionID)
    case .textReset:
      resetResponsePreviewText(for: sessionID)
    case .finalAnswerDelta(let text):
      let controller = streamingDisplayController(for: sessionID)
      let isFirstTypedFinalAnswer = streamStates[sessionID]?.hasTypedFinalAnswerEvents != true
      let wasRenderedByLegacyTextDelta =
        isFirstTypedFinalAnswer
        && (streamStates[sessionID]?.text.hasSuffix(text) == true
          || controller.pendingTextHasSuffix(text))
      markTypedFinalAnswerEvents(for: sessionID)
      if isFirstTypedFinalAnswer {
        removePromotedPreviewNarration(text, for: sessionID)
      }
      if !wasRenderedByLegacyTextDelta {
        controller.appendToken(text)
      }
    case .completedSummary(let text):
      setBackendCompletedSummaryText(text, for: sessionID)
    case .textDelta(let text):
      if streamStates[sessionID]?.hasTypedFinalAnswerEvents != true {
        if shouldRouteLegacyTextDeltaAsActivityNarration(for: sessionID) {
          if normalizedActivityNarration(text) != currentActivityNarration(for: sessionID) {
            appendActivityNarration(text, for: sessionID)
          }
        } else {
          streamingDisplayController(for: sessionID).appendToken(text)
        }
      }
    case .progress(let kind, let message):
      setStreamingProgress(
        StreamingProgress(
          kind: StreamingProgressKind(rawValue: kind),
          message: message
        ),
        for: sessionID
      )
    case .notification(let title, let body):
      await NotificationService.shared.send(title: title, body: body)
    case .activityStart(let id, let title, _):
      markTypedActivityEvents(for: sessionID)
      beginActivityChunk(sessionID: sessionID, activityID: id, title: title)
    case .activityEnd(let id):
      markTypedActivityEvents(for: sessionID)
      endActivityChunk(sessionID: sessionID, activityID: id)
    case .activityToolCallStart(let activityID, let id, let name):
      markTypedActivityEvents(for: sessionID)
      beginToolCall(sessionID: sessionID, activityID: activityID, id: id, name: name)
    case .activityToolCallComplete(let activityID, let id, let name, let arguments):
      markTypedActivityEvents(for: sessionID)
      completeToolCall(
        sessionID: sessionID,
        activityID: activityID,
        id: id,
        name: name,
        arguments: arguments
      )
    case .activityToolResult(let activityID, let id, let toolName, let output, let isError):
      markTypedActivityEvents(for: sessionID)
      finishToolCall(
        sessionID: sessionID,
        activityID: activityID,
        id: id,
        name: toolName,
        output: output,
        isError: isError
      )
    case .toolCallStart(let id, let name):
      if streamStates[sessionID]?.hasTypedActivityEvents != true {
        beginToolCall(sessionID: sessionID, id: id, name: name)
      }
    case .toolCallDelta(let id, let argumentsDelta):
      if streamStates[sessionID]?.hasTypedActivityEvents != true {
        updateToolCall(sessionID: sessionID, id: id) { toolCall in
          toolCall.arguments += argumentsDelta
        }
      }
    case .toolCallComplete(let id, let name, let arguments):
      if streamStates[sessionID]?.hasTypedActivityEvents != true {
        completeToolCall(sessionID: sessionID, id: id, name: name, arguments: arguments)
      }
    case .toolResult(let id, let output, let isError):
      if streamStates[sessionID]?.hasTypedActivityEvents != true {
        finishToolCall(sessionID: sessionID, id: id, output: output, isError: isError)
      }
    case .toolProgress(
      let activityID,
      let id,
      let toolName,
      let category,
      let target,
      let advancesSlot,
      let outcome
    ):
      applyToolProgress(
        sessionID: sessionID,
        activityID: activityID,
        id: id,
        toolName: toolName,
        category: category,
        target: target,
        advancesSlot: advancesSlot,
        outcome: outcome
      )
    case .permissionPrompt(let prompt):
      enqueuePermissionPrompt(prompt)
    case .phase(let phase):
      setStreamingPhase(StreamingPhase(rawValue: phase), for: sessionID)
    case .transcriptPhaseBoundary(let phase):
      setTranscriptPhaseBoundary(TranscriptPhaseBoundary(rawValue: phase), for: sessionID)
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
      return .finish(response)
    case .engineError(_, let message, let recoverable):
      if !recoverable {
        handleStreamError(message, sessionID: sessionID)
        return .fail
      }
    case .error(let message):
      handleStreamError(message, sessionID: sessionID)
      return .fail
    }

    return .continue
  }

  private func markTypedActivityEvents(for sessionID: String) {
    streamStates[sessionID]?.hasTypedActivityEvents = true
  }

  private func markTypedFinalAnswerEvents(for sessionID: String) {
    streamStates[sessionID]?.hasTypedFinalAnswerEvents = true
    streamStates[sessionID]?.responsePreviewText = ""
  }

  private func setBackendCompletedSummaryText(_ text: String, for sessionID: String) {
    guard streamStates[sessionID] != nil,
          let summaryText = normalizedBackendCompletedSummaryText(text)
    else {
      return
    }

    streamStates[sessionID]?.completedSummaryText = summaryText
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func resetStreamingText(for sessionID: String) {
    guard streamStates[sessionID] != nil else {
      return
    }

    streamingDisplayController(for: sessionID).reset(repinToBottom: false)
    streamStates[sessionID]?.text = ""
  }

  private func appendResponsePreviewText(_ text: String, for sessionID: String) {
    guard !text.isEmpty, streamStates[sessionID] != nil else {
      return
    }

    // `text_preview_delta` is speculative answer text from a model turn that may
    // still resolve to tool use. Render it as live answer text now; if the
    // backend later sends `text_reset`, demote the exact preview into working
    // narration so tool-driving voiceover is preserved with the activity chunk.
    streamStates[sessionID]?.responsePreviewText += text
    streamingDisplayController(for: sessionID).appendToken(text)
  }

  private func resetResponsePreviewText(for sessionID: String) {
    guard streamStates[sessionID] != nil else {
      return
    }

    let previewText = streamStates[sessionID]?.responsePreviewText ?? ""
    streamStates[sessionID]?.responsePreviewText = ""
    resetStreamingText(for: sessionID)

    if let demotedPreview = normalizedActivityNarration(previewText) {
      appendActivityNarration(demotedPreview, for: sessionID)
    }
    markActivityNarrationBoundary(for: sessionID)
  }

  private func removeResponsePreviewIfPromotedToNarration(_ text: String, for sessionID: String) {
    guard let previewText = streamStates[sessionID]?.responsePreviewText,
          !previewText.isEmpty
    else {
      return
    }

    let normalizedPreview = normalizedActivityNarration(previewText)
    let normalizedNarration = normalizedActivityNarration(text)
    let isSameNarration =
      normalizedPreview != nil && normalizedPreview == normalizedNarration
    guard isSameNarration || previewText.hasSuffix(text) else {
      return
    }

    streamStates[sessionID]?.responsePreviewText = ""
    resetStreamingText(for: sessionID)
  }

  private func appendActivityNarration(_ text: String, for sessionID: String) {
    guard !text.isEmpty, streamStates[sessionID] != nil else {
      return
    }

    streamStates[sessionID]?.activityNarration += text
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func shouldRouteLegacyTextDeltaAsActivityNarration(for sessionID: String) -> Bool {
    guard let streamState = streamStates[sessionID] else {
      return false
    }

    let hasLegacyToolActivity =
      liveToolGroupsBySession[sessionID]?.contains { !$0.toolCalls.isEmpty } == true

    return !streamState.hasTypedFinalAnswerEvents
      && (streamState.hasTypedActivityEvents || hasLegacyToolActivity)
  }

  private func markActivityNarrationBoundary(for sessionID: String) {
    guard streamStates[sessionID] != nil else {
      return
    }

    // Preview resets mean the text should not be treated as final assistant
    // output. The activity transcript intentionally keeps it as work narration.
    persistCurrentActivityNarration(for: sessionID)
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
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
    var messageAnchorCounts: [String: Int] = [:]
    var items: [ChatTranscriptItem] = []
    var deferredLiveFinalAnswerItems: [ChatTranscriptItem] = []
    var pendingHistoricalGroupIndices: [Int] = []
    let acceptedSteeringRecords = acceptedSteeringRecords(for: sessionID)
    var appendedSteeringRecordIDs: Set<String> = []
    let assistantTurnToolActivityFlags = assistantTurnToolActivityFlags(in: messages)
    let hasLiveActivity = sessionID.map { !(liveToolGroupsBySession[$0]?.isEmpty ?? true) } ?? false

    func appendAcceptedSteeringRecords(anchor: AcceptedSteeringAnchor) {
      for steering in acceptedSteeringRecords where steering.anchor == anchor {
        guard appendedSteeringRecordIDs.insert(steering.record.id).inserted else {
          continue
        }
        items.append(.turnSteering(steering.record))
      }
    }

    appendAcceptedSteeringRecords(anchor: .sessionStart)
    for (messageIndex, message) in messages.enumerated() {
      let anchorID = messageAnchorID(
        for: message,
        occurrenceCounts: &messageAnchorCounts
      )
      switch message.role {
      case .tool:
        let toolResults = toolResults(from: message)
        if !toolResults.isEmpty, !pendingHistoricalGroupIndices.isEmpty {
          applyToolResults(toolResults, to: &items, groupIndices: pendingHistoricalGroupIndices)
          appendAcceptedSteeringRecords(anchor: .afterMessage(anchorID))
          continue
        }

        let displayText = message.transcriptDisplayText
        if !displayText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
          items.append(
            messageTranscriptItem(
              message,
              displayText: displayText,
              isFinalAnswer: shouldRenderMessageAsFinalAnswer(
                message,
                at: messageIndex,
                in: messages,
                sessionID: sessionID
              ),
              duplicateCounts: &duplicateCounts
            )
          )
        }
        pendingHistoricalGroupIndices = []
      case .user, .assistant, .system:
        let parts = historicalTranscriptParts(from: message)
        var messageGroupIndices: [Int] = []

        for part in parts {
          switch part {
          case .text(let textPart):
            let isFinalAnswer = shouldRenderMessageAsFinalAnswer(
              message,
              at: messageIndex,
              in: messages,
              sessionID: sessionID,
              isFinalAnswerCandidate: textPart.isFinalAnswerCandidate
            )
            let isWorkingNarration = shouldRenderMessageAsWorkingNarration(
              message,
              turnContainsToolActivity: assistantTurnToolActivityFlags[messageIndex]
                || (hasLiveActivity && isMessageInOpenTurn(at: messageIndex, in: messages)),
              isFinalAnswer: isFinalAnswer
            )
            let item = messageTranscriptItem(
              message,
              displayText: textPart.text,
              isFinalAnswer: isFinalAnswer,
              isWorkingNarration: isWorkingNarration,
              duplicateCounts: &duplicateCounts
            )
            if shouldDeferLiveFinalAnswer(
              item,
              message: message,
              at: messageIndex,
              in: messages,
              sessionID: sessionID
            ) {
              deferredLiveFinalAnswerItems.append(item)
            } else {
              items.append(item)
            }
          case .activity(let activity):
            items.append(
              .toolActivityGroup(
                ToolActivityGroupRecord(
                  id: historicalToolGroupID(for: message, toolCalls: activity.toolCalls),
                  toolCalls: activity.toolCalls,
                  isLive: false
                )
              )
            )
            messageGroupIndices.append(items.count - 1)
          }
        }
        pendingHistoricalGroupIndices = messageGroupIndices
      }
      appendAcceptedSteeringRecords(anchor: .afterMessage(anchorID))
    }
    for steering in acceptedSteeringRecords
      where !appendedSteeringRecordIDs.contains(steering.record.id)
        && !steering.anchor.isLiveItemAnchor {
      appendedSteeringRecordIDs.insert(steering.record.id)
      items.append(.turnSteering(steering.record))
    }

    if let sessionID {
      appendLiveActivityGroups(
        for: sessionID,
        messages: messages,
        to: &items
      )
      items.append(contentsOf: deferredLiveFinalAnswerItems)
      appendLiveCompletedSummary(for: sessionID, to: &items)
      appendLiveFinalAnswer(for: sessionID, to: &items)
    }

    appendAcceptedSteeringRecordsAfterItems(
      to: &items,
      records: acceptedSteeringRecords,
      appendedRecordIDs: &appendedSteeringRecordIDs
    )
    for steering in acceptedSteeringRecords
      where !appendedSteeringRecordIDs.contains(steering.record.id)
        && !isAcceptedSteeringRepresentedInCompletedSummary(steering.record, sessionID: sessionID) {
      appendedSteeringRecordIDs.insert(steering.record.id)
      items.append(.turnSteering(steering.record))
    }

    return validateTranscriptPhaseOrder(
      transcriptItemsWithCompletedWorkSummaries(items, sessionID: sessionID)
    )
  }

  private func shouldDeferLiveFinalAnswer(
    _ item: ChatTranscriptItem,
    message: SessionMessage,
    at index: Int,
    in messages: [SessionMessage],
    sessionID: String?
  ) -> Bool {
    guard case .finalAnswer = item,
          message.role == .assistant,
          let sessionID,
          isSessionStreaming(sessionID),
          !(liveToolGroupsBySession[sessionID]?.isEmpty ?? true),
          isMessageInOpenTurn(at: index, in: messages),
          streamStateAllowsOpenEndedFinalAnswer(for: sessionID)
    else {
      return false
    }

    return true
  }

  private func appendAcceptedSteeringRecordsAfterItems(
    to items: inout [ChatTranscriptItem],
    records: [AcceptedSteeringRecord],
    appendedRecordIDs: inout Set<String>
  ) {
    var rebuiltItems: [ChatTranscriptItem] = []
    rebuiltItems.reserveCapacity(items.count + records.count)

    for item in items {
      rebuiltItems.append(item)
      let matchingRecords = records.filter { steering in
        guard !appendedRecordIDs.contains(steering.record.id),
              case .afterItem(let itemID) = steering.anchor
        else {
          return false
        }
        return itemID == item.id
      }
      guard !matchingRecords.isEmpty else {
        continue
      }

      rebuiltItems.append(
        contentsOf: matchingRecords.map { steering in
          appendedRecordIDs.insert(steering.record.id)
          return .turnSteering(steering.record)
        }
      )
    }

    items = rebuiltItems
  }

  private func acceptedSteeringRecords(for sessionID: String?) -> [AcceptedSteeringRecord] {
    guard let sessionID else {
      return []
    }
    return acceptedSteeringRecordsBySession[sessionID] ?? []
  }

  private func messageTranscriptItem(
    _ message: SessionMessage,
    displayText: String,
    isFinalAnswer: Bool,
    isWorkingNarration: Bool = false,
    duplicateCounts: inout [String: Int]
  ) -> ChatTranscriptItem {
    let baseID = messageStableIDBase(for: message)
    let occurrence = duplicateCounts[baseID, default: 0]
    duplicateCounts[baseID] = occurrence + 1
    let id = occurrence == 0 ? baseID : "\(baseID)#\(occurrence)"
    let transcriptMessage = TranscriptMessage(
      id: id,
      message: message,
      displayText: displayText,
      footnoteText: nil,
      isWorkingNarration: isWorkingNarration
    )
    if isFinalAnswer {
      return .finalAnswer(transcriptMessage)
    }

    return .message(transcriptMessage)
  }

  private func workingNarrationTranscriptItem(
    _ narration: WorkingNarrationRecord,
    timestamp: Int
  ) -> ChatTranscriptItem {
    .message(
      TranscriptMessage(
        id: narration.id,
        message: SessionMessage(role: .assistant, content: narration.text, timestamp: timestamp),
        displayText: narration.text,
        footnoteText: nil,
        isWorkingNarration: true,
        isStreaming: narration.isLive
      )
    )
  }

  private func shouldRenderMessageAsFinalAnswer(
    _ message: SessionMessage,
    at index: Int,
    in messages: [SessionMessage],
    sessionID: String?,
    isFinalAnswerCandidate: Bool = true
  ) -> Bool {
    guard message.role == .assistant else {
      return false
    }

    guard isFinalAnswerCandidate else {
      return false
    }

    guard let sessionID else {
      return isTerminalAssistantMessageInTurn(at: index, in: messages)
    }

    if completedStreamingSummary(
      for: message,
      sessionID: sessionID,
      allowContentDigestFallback: true
    ) != nil {
      return isTerminalAssistantMessageInTurn(at: index, in: messages)
    }

    let hasLiveActivity = !(liveToolGroupsBySession[sessionID]?.isEmpty ?? true)
    guard isSessionStreaming(sessionID) || hasLiveActivity else {
      return isTerminalAssistantMessageInTurn(at: index, in: messages)
    }

    return isTerminalAssistantMessageInTurn(
      at: index,
      in: messages,
      openEndedIsTerminal: streamStateAllowsOpenEndedFinalAnswer(for: sessionID)
    )
  }

  private func streamStateAllowsOpenEndedFinalAnswer(for sessionID: String) -> Bool {
    guard let streamState = streamStates[sessionID] else {
      return false
    }

    return streamState.hasTypedFinalAnswerEvents
      || !streamState.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      || streamState.transcriptPhase?.allowsOpenEndedFinalAnswer == true
  }

  private func isTerminalAssistantMessageInTurn(
    at index: Int,
    in messages: [SessionMessage],
    openEndedIsTerminal: Bool = true
  ) -> Bool {
    let nextIndex = messages.index(after: index)
    guard nextIndex < messages.endIndex else {
      return openEndedIsTerminal
    }

    for laterMessage in messages[nextIndex...] {
      switch laterMessage.role {
      case .user, .system:
        return true
      case .assistant:
        return false
      case .tool:
        continue
      }
    }

    return openEndedIsTerminal
  }

  private func isMessageInOpenTurn(
    at index: Int,
    in messages: [SessionMessage]
  ) -> Bool {
    let nextIndex = messages.index(after: index)
    guard nextIndex < messages.endIndex else {
      return true
    }

    for laterMessage in messages[nextIndex...] {
      if laterMessage.role == .user || laterMessage.role == .system {
        return false
      }
    }

    return true
  }

  private func shouldRenderMessageAsWorkingNarration(
    _ message: SessionMessage,
    turnContainsToolActivity: Bool,
    isFinalAnswer: Bool
  ) -> Bool {
    guard message.role == .assistant, !isFinalAnswer else {
      return false
    }

    return turnContainsToolActivity
  }

  private func assistantTurnToolActivityFlags(
    in messages: [SessionMessage]
  ) -> [Bool] {
    var flags = Array(repeating: false, count: messages.count)
    var laterToolActivityBeforeNextUserTurn = false

    for index in messages.indices.reversed() {
      let message = messages[index]
      if message.role == .user || message.role == .system {
        laterToolActivityBeforeNextUserTurn = false
        continue
      }

      if message.contentBlocks.contains(where: { toolCall(from: $0) != nil || $0.containsToolResult }) {
        laterToolActivityBeforeNextUserTurn = true
      }
      flags[index] = laterToolActivityBeforeNextUserTurn
    }

    return flags
  }

  private func transcriptItemsWithCompletedWorkSummaries(
    _ items: [ChatTranscriptItem],
    sessionID: String?
  ) -> [ChatTranscriptItem] {
    var transformed: [ChatTranscriptItem] = []
    var index = items.startIndex

    while index < items.endIndex {
      let item = items[index]
      guard
        let message = item.transcriptMessage,
        message.message.role == .assistant,
        !message.isWorkingNarration,
        let summary = completedStreamingSummary(
          for: message.message,
          sessionID: sessionID,
          allowContentDigestFallback: isLastAssistantMessageWithSameContent(
            message.message,
            at: index,
            in: items
          )
        )
      else {
        transformed.append(item)
        index = items.index(after: index)
        continue
      }

      let precedingEntries = popCurrentTurnCompletedWorkEntries(from: &transformed)

      var nextIndex = items.index(after: index)
      var followingItems: [ChatTranscriptItem] = []
      if !summary.activityGroups.isEmpty {
        let summaryGroupIDs = Set(summary.activityGroups.map(\.id))
        while
          nextIndex < items.endIndex,
          case .toolActivityGroup(let group) = items[nextIndex],
          summaryGroupIDs.contains(group.id)
        {
          followingItems.append(items[nextIndex])
          nextIndex = items.index(after: nextIndex)
        }
      }

      let historicalEntries = precedingEntries + completedWorkEntries(from: followingItems)
      let storedEntries = summary.entries.isEmpty
        ? completedWorkEntries(from: summary.activityGroups)
        : summary.entries
      let completedEntries = mergeCompletedWorkEntries(
        storedEntries: storedEntries,
        orderedHistoricalEntries: historicalEntries
      )

      transformed.append(
        .completedWorkSummary(
          CompletedWorkSummaryRecord(
            id: message.id,
            elapsedText: summary.elapsedText,
            summaryText: summary.summaryText,
            entries: completedEntries
          )
        )
      )
      transformed.append(
        .finalAnswer(
          TranscriptMessage(
            id: message.id,
            message: message.message,
            displayText: message.displayText,
            footnoteText: nil
          )
        )
      )
      index = nextIndex
    }

    return transformed
  }

  private func popCurrentTurnCompletedWorkEntries(
    from transformed: inout [ChatTranscriptItem]
  ) -> [CompletedWorkEntry] {
    var workingItems: [ChatTranscriptItem] = []

    while let item = transformed.last, isCurrentTurnCompletedWorkSource(item) {
      workingItems.insert(item, at: 0)
      transformed.removeLast()
    }

    return completedWorkEntries(from: workingItems)
  }

  private func isCurrentTurnCompletedWorkSource(_ item: ChatTranscriptItem) -> Bool {
    switch item {
    case .toolActivityGroup:
      return true
    case .message(let message):
      return message.message.role == .assistant && message.isWorkingNarration
    case .turnSteering:
      return true
    case .completedWorkSummary, .finalAnswer:
      return false
    }
  }

  private func completedWorkEntries(from items: [ChatTranscriptItem]) -> [CompletedWorkEntry] {
    var entries: [CompletedWorkEntry] = []

    for item in items {
      switch item {
      case .toolActivityGroup(let group):
        entries.append(contentsOf: completedWorkEntries(from: group))
      case .message(let message)
        where message.message.role == .assistant && message.isWorkingNarration:
        guard message.displayText.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty != nil
        else {
          continue
        }
        entries.append(
          .narration(
            CompletedWorkNarrationRecord(
              id: "\(message.id)-working",
              text: message.displayText
            )
          )
        )
      case .turnSteering(let steering):
        entries.append(.turnSteering(steering))
      case .message, .completedWorkSummary, .finalAnswer:
        continue
      }
    }

    return entries
  }

  private func completedWorkEntries(from group: ToolActivityGroupRecord) -> [CompletedWorkEntry] {
    CompletedWorkSummaryRecord.entries(from: group)
  }

  private func completedWorkEntries(
    from groups: [ToolActivityGroupRecord]
  ) -> [CompletedWorkEntry] {
    completedWorkEntries(from: groups.map(ChatTranscriptItem.toolActivityGroup))
  }

  private func mergeCompletedWorkEntries(
    storedEntries: [CompletedWorkEntry],
    orderedHistoricalEntries: [CompletedWorkEntry]
  ) -> [CompletedWorkEntry] {
    guard !storedEntries.isEmpty else {
      return orderedHistoricalEntries
    }
    guard !orderedHistoricalEntries.isEmpty else {
      return storedEntries
    }

    var merged: [CompletedWorkEntry] = []
    var consumedStoredEntryIDs: Set<String> = []
    var consumedToolCallIDs: Set<String> = []
    var emittedNarrationKeys: Set<String> = []

    func appendIfUseful(_ entry: CompletedWorkEntry) {
      guard entry.hasVisibleActivity else {
        return
      }

      switch entry {
      case .narration(let narration):
        guard let key = normalizedCompletedWorkNarration(narration.text),
              emittedNarrationKeys.insert(key).inserted
        else {
          return
        }
        merged.append(entry)
      case .toolActivityGroup(let group):
        let ids = toolCallIDs(in: group)
        guard !ids.isEmpty else {
          merged.append(entry)
          return
        }
        let newIDs = ids.filter { !consumedToolCallIDs.contains($0) }
        guard !newIDs.isEmpty else {
          return
        }
        consumedToolCallIDs.formUnion(newIDs)
        merged.append(entry)
      case .turnSteering:
        merged.append(entry)
      }
    }

    var historicalIndex = orderedHistoricalEntries.startIndex
    while historicalIndex < orderedHistoricalEntries.endIndex {
      let historicalEntry = orderedHistoricalEntries[historicalIndex]
      let nextHistoricalIndex = orderedHistoricalEntries.index(after: historicalIndex)

      if case .narration = historicalEntry,
         nextHistoricalIndex < orderedHistoricalEntries.endIndex,
         case .toolActivityGroup(let nextHistoricalGroup) = orderedHistoricalEntries[nextHistoricalIndex],
         let storedToolIndex = storedToolEntryIndex(
          matching: nextHistoricalGroup,
          in: storedEntries,
          consumedEntryIDs: consumedStoredEntryIDs
         ),
         storedNarrationForToolEntry(
          before: storedToolIndex,
          in: storedEntries,
          consumedEntryIDs: consumedStoredEntryIDs
         ) != nil
      {
        // Historical assistant messages often reconstruct "narration before tool" from the same
        // provider payload that already produced typed live narration. Prefer the typed record,
        // but keep non-adjacent historical narration that occurred after a completed tool.
        historicalIndex = nextHistoricalIndex
        continue
      }

      guard case .toolActivityGroup(let historicalGroup) = historicalEntry,
            let storedToolIndex = storedToolEntryIndex(
              matching: historicalGroup,
              in: storedEntries,
              consumedEntryIDs: consumedStoredEntryIDs
            )
      else {
        appendIfUseful(historicalEntry)
        historicalIndex = nextHistoricalIndex
        continue
      }

      if let storedNarration = storedNarrationForToolEntry(
        before: storedToolIndex,
        in: storedEntries,
        consumedEntryIDs: consumedStoredEntryIDs
      ) {
        consumedStoredEntryIDs.insert(storedNarration.id)
        appendIfUseful(storedNarration)
      }

      let storedEntry = storedEntries[storedToolIndex]
      consumedStoredEntryIDs.insert(storedEntry.id)
      appendIfUseful(storedEntry)
      historicalIndex = nextHistoricalIndex
    }

    for storedEntry in storedEntries where !consumedStoredEntryIDs.contains(storedEntry.id) {
      appendIfUseful(storedEntry)
    }

    return merged
  }

  private func storedToolEntryIndex(
    matching historicalGroup: ToolActivityGroupRecord,
    in storedEntries: [CompletedWorkEntry],
    consumedEntryIDs: Set<String>
  ) -> [CompletedWorkEntry].Index? {
    storedEntries.firstIndex { storedEntry in
      guard !consumedEntryIDs.contains(storedEntry.id),
            case .toolActivityGroup(let storedGroup) = storedEntry
      else {
        return false
      }
      return !toolCallIDs(in: historicalGroup).isDisjoint(with: toolCallIDs(in: storedGroup))
    }
  }

  private func storedNarrationForToolEntry(
    before toolEntryIndex: [CompletedWorkEntry].Index,
    in storedEntries: [CompletedWorkEntry],
    consumedEntryIDs: Set<String>
  ) -> CompletedWorkEntry? {
    guard toolEntryIndex > storedEntries.startIndex else {
      return nil
    }

    var index = storedEntries.index(before: toolEntryIndex)
    while true {
      let entry = storedEntries[index]
      switch entry {
      case .narration:
        if !consumedEntryIDs.contains(entry.id) {
          return entry
        }
      case .toolActivityGroup:
        return nil
      case .turnSteering:
        break
      }

      guard index > storedEntries.startIndex else {
        return nil
      }
      index = storedEntries.index(before: index)
    }
  }

  private func toolCallIDs(in group: ToolActivityGroupRecord) -> Set<String> {
    Set(group.toolCalls.map(\.id))
  }

  private func normalizedCompletedWorkNarration(_ text: String) -> String? {
    text
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
      .nonEmpty
  }

  private func normalizedBackendCompletedSummaryText(_ text: String?) -> String? {
    text?
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .nonEmpty
  }

  private func validateTranscriptPhaseOrder(_ items: [ChatTranscriptItem]) -> [ChatTranscriptItem] {
    if let violation = items.firstTranscriptPhaseOrderViolation() {
      assertionFailure(
        "Transcript phase order violation: working phase \(violation.laterWorkingPhase) "
          + "with id \(violation.laterWorkingItemID) appeared after terminal item "
          + violation.terminalItemID
      )
    }

    return items
  }

  private static func uniquedActivityGroups(
    _ groups: [ToolActivityGroupRecord]
  ) -> [ToolActivityGroupRecord] {
    var seen: Set<String> = []
    var uniqueGroups: [ToolActivityGroupRecord] = []
    for group in groups where !seen.contains(group.id) {
      seen.insert(group.id)
      uniqueGroups.append(group)
    }
    return uniqueGroups
  }

  private func messageStableIDBase(for message: SessionMessage) -> String {
    [
      message.role.rawValue,
      String(message.timestamp),
      Self.stableDigest(for: message.content),
    ].joined(separator: ":")
  }

  private func messageAnchorID(
    for message: SessionMessage,
    occurrenceCounts: inout [String: Int]
  ) -> String {
    let baseID = messageStableIDBase(for: message)
    let occurrence = occurrenceCounts[baseID, default: 0]
    occurrenceCounts[baseID] = occurrence + 1
    return occurrence == 0 ? baseID : "\(baseID)#\(occurrence)"
  }

  private func recordCompletedStreamingFootnote(
    startedAt: Date?,
    endedAt: Date,
    for message: SessionMessage,
    sessionID: String
  ) {
    guard
      let startedAt,
      let footnote = streamingElapsedFootnoteText(
        startedAt: startedAt,
        endedAt: endedAt,
        minimumSeconds: 1
      )
    else {
      return
    }

    persistCurrentActivityNarration(for: sessionID)
    let activityGroups = completedActivityGroups(for: sessionID)
    let summary = CompletedStreamSummary(
      messageID: messageStableIDBase(for: message),
      contentDigest: messageContentDigest(for: message),
      terminalTextDigest: messageTerminalTextDigest(for: message),
      elapsedText: footnote,
      summaryText: normalizedBackendCompletedSummaryText(
        streamStates[sessionID]?.completedSummaryText
      ),
      activityGroups: activityGroups,
      entries: completedStreamingEntries(
        for: sessionID,
        fallbackActivityGroups: activityGroups
      )
    )
    var summaries = completedStreamSummariesBySession[sessionID, default: []]
    summaries.removeAll {
      $0.messageID == summary.messageID || $0.contentDigest == summary.contentDigest
    }
    summaries.append(summary)
    completedStreamSummariesBySession[sessionID] = Array(summaries.suffix(8))
  }

  private func completedStreamingSummary(
    for message: SessionMessage,
    sessionID: String?,
    allowContentDigestFallback: Bool
  ) -> CompletedStreamSummary? {
    guard let sessionID else {
      return nil
    }

    guard let summaries = completedStreamSummariesBySession[sessionID] else {
      return nil
    }

    let messageID = messageStableIDBase(for: message)
    if let exactMatch = summaries.last(where: { $0.messageID == messageID }) {
      return exactMatch
    }

    guard allowContentDigestFallback else {
      return nil
    }

    let contentDigest = messageContentDigest(for: message)
    if let digestMatch = summaries.last(where: { $0.contentDigest == contentDigest }) {
      return digestMatch
    }

    let terminalTextDigest = messageTerminalTextDigest(for: message)
    return summaries.last { $0.terminalTextDigest == terminalTextDigest }
  }

  private func isLastAssistantMessageWithSameContent(
    _ message: SessionMessage,
    at index: Int,
    in items: [ChatTranscriptItem]
  ) -> Bool {
    let contentDigest = messageContentDigest(for: message)
    let nextIndex = items.index(after: index)
    guard nextIndex < items.endIndex else {
      return true
    }

    return !items[nextIndex...].contains { item in
      guard
        let laterMessage = item.transcriptMessage,
        laterMessage.message.role == .assistant
      else {
        return false
      }
      return messageContentDigest(for: laterMessage.message) == contentDigest
    }
  }

  private func completedActivityGroups(for sessionID: String) -> [ToolActivityGroupRecord] {
    Self.uniquedActivityGroups(
      (retiredLiveToolGroupsBySession[sessionID] ?? [])
        + (liveToolGroupsBySession[sessionID] ?? [])
    )
      .map(completedActivityGroup)
      .filter(\.hasVisibleActivity)
  }

  private func completedStreamingEntries(
    for sessionID: String,
    fallbackActivityGroups: [ToolActivityGroupRecord]
  ) -> [CompletedWorkEntry] {
    let storedEntries = completedActivityEntries(
      for: sessionID,
      fallbackActivityGroups: fallbackActivityGroups
    )
    if currentSessionID == sessionID {
      let entries = currentTurnCompletedWorkEntries(from: transcriptItems)
      if !entries.isEmpty {
        return mergeCompletedWorkEntries(
          storedEntries: storedEntries,
          orderedHistoricalEntries: entries
        )
      }
    }

    return storedEntries
  }

  private func completedActivityEntries(
    for sessionID: String,
    fallbackActivityGroups: [ToolActivityGroupRecord]
  ) -> [CompletedWorkEntry] {
    let groupsByID = Dictionary(uniqueKeysWithValues: fallbackActivityGroups.map { ($0.id, $0) })
    let narrationByActivityID = activeLiveNarrationByActivityID(for: sessionID)
    var entries: [CompletedWorkEntry] = []
    var emittedGroupIDs: Set<String> = []
    var emittedNarrationIDs: Set<String> = []

    for activityID in liveActivityOrderBySession[sessionID, default: []] {
      if let narration = narrationByActivityID[activityID],
         emittedNarrationIDs.insert(narration.id).inserted {
        entries.append(
          .narration(
            CompletedWorkNarrationRecord(id: narration.id, text: narration.text)
          )
        )
      }

      if let group = groupsByID[activityID], emittedGroupIDs.insert(group.id).inserted {
        entries.append(contentsOf: completedWorkEntries(from: group))
      }
    }

    for group in fallbackActivityGroups where emittedGroupIDs.insert(group.id).inserted {
      entries.append(contentsOf: completedWorkEntries(from: group))
    }

    return entries
  }

  private func currentTurnCompletedWorkEntries(
    from items: [ChatTranscriptItem]
  ) -> [CompletedWorkEntry] {
    var currentTurnItems: [ChatTranscriptItem] = []

    for item in items.reversed() {
      if let message = item.sessionMessage,
         message.role == .user || message.role == .system {
        break
      }

      guard isCurrentTurnCompletedWorkSource(item) else {
        continue
      }

      currentTurnItems.insert(item, at: 0)
    }

    return completedWorkEntries(from: currentTurnItems)
  }

  private func isAcceptedSteeringRepresentedInCompletedSummary(
    _ record: TurnSteeringRecord,
    sessionID: String?
  ) -> Bool {
    guard let sessionID,
          let summaries = completedStreamSummariesBySession[sessionID]
    else {
      return false
    }

    return summaries.contains { summary in
      summary.entries.contains { entry in
        guard case .turnSteering(let steering) = entry else {
          return false
        }
        return steering.id == record.id
      }
    }
  }

  private func completedActivityGroup(_ group: ToolActivityGroupRecord) -> ToolActivityGroupRecord {
    ToolActivityGroupRecord(
      id: group.id,
      toolCalls: group.toolCalls.map { toolCall in
        ToolCallRecord(
          id: toolCall.id,
          name: toolCall.name,
          arguments: toolCall.arguments,
          result: toolCall.result,
          isRunning: false,
          isError: toolCall.isError
        )
      },
      isLive: false
    )
  }

  private func messageContentDigest(for message: SessionMessage) -> String {
    Self.stableDigest(for: message.content)
  }

  private func messageTerminalTextDigest(for message: SessionMessage) -> String {
    Self.stableDigest(for: Self.terminalDisplayText(for: message))
  }

  private static func terminalDisplayText(for message: SessionMessage) -> String {
    message.contentBlocks.reversed().compactMap(\.transcriptDisplayText).first
      ?? message.transcriptDisplayText
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
    completedStreamSummariesBySession.removeValue(forKey: sessionID)
    acceptedSteeringRecordsBySession.removeValue(forKey: sessionID)
    retiredLiveToolGroupsBySession.removeValue(forKey: sessionID)
    retiredNarrationByActivityIDBySession.removeValue(forKey: sessionID)
  }

  private func touchCachedSession(_ sessionID: String) {
    transcriptCacheAccessOrder.removeAll { $0 == sessionID }
    transcriptCacheAccessOrder.append(sessionID)
  }

  private func trimTranscriptCacheIfNeeded() {
    let protectedSessions = activeStreamSessionIDs.union(Set([currentSessionID].compactMap { $0 }))
    var scannedEntries = 0
    var removedLiveToolActivity = false

    while transcriptCache.count > Self.maxCachedSessions,
      scannedEntries < transcriptCacheAccessOrder.count
    {
      let leastRecentSessionID = transcriptCacheAccessOrder.removeFirst()
      if protectedSessions.contains(leastRecentSessionID) {
        transcriptCacheAccessOrder.append(leastRecentSessionID)
        scannedEntries += 1
        continue
      }

      transcriptCache.removeValue(forKey: leastRecentSessionID)
      acceptedSteeringRecordsBySession.removeValue(forKey: leastRecentSessionID)
      retiredLiveToolGroupsBySession.removeValue(forKey: leastRecentSessionID)
      retiredNarrationByActivityIDBySession.removeValue(forKey: leastRecentSessionID)
      liveNarrationByActivityIDBySession.removeValue(forKey: leastRecentSessionID)
      liveActivityOrderBySession.removeValue(forKey: leastRecentSessionID)
      if liveToolGroupsBySession.removeValue(forKey: leastRecentSessionID) != nil {
        removedLiveToolActivity = true
      }
      anonymousToolCallCountersBySession.removeValue(forKey: leastRecentSessionID)
      scannedEntries = 0
    }

    while transcriptCache.count > Self.maxCachedSessions,
      let leastRecentSessionID = transcriptCacheAccessOrder.first
    {
      transcriptCacheAccessOrder.removeFirst()
      transcriptCache.removeValue(forKey: leastRecentSessionID)
      acceptedSteeringRecordsBySession.removeValue(forKey: leastRecentSessionID)
      retiredLiveToolGroupsBySession.removeValue(forKey: leastRecentSessionID)
      retiredNarrationByActivityIDBySession.removeValue(forKey: leastRecentSessionID)
      liveNarrationByActivityIDBySession.removeValue(forKey: leastRecentSessionID)
      liveActivityOrderBySession.removeValue(forKey: leastRecentSessionID)
      if liveToolGroupsBySession.removeValue(forKey: leastRecentSessionID) != nil {
        removedLiveToolActivity = true
      }
      anonymousToolCallCountersBySession.removeValue(forKey: leastRecentSessionID)
    }

    if removedLiveToolActivity {
      syncThreadRuntimeActivity()
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
    var items = liveActivityItems(for: sessionID, messages: [])
    appendLiveCompletedSummary(for: sessionID, to: &items)
    appendLiveFinalAnswer(for: sessionID, to: &items)
    return items
  }

  private func appendLiveActivityGroups(
    for sessionID: String,
    messages: [SessionMessage],
    to items: inout [ChatTranscriptItem]
  ) {
    guard !currentTurnAlreadyHasTerminalSummary(in: items, sessionID: sessionID) else {
      return
    }
    guard isSessionStreaming(sessionID)
      || liveToolGroupsBySession[sessionID]?.contains(where: \.isLive) == true
    else {
      return
    }

    let liveItems = liveActivityItems(for: sessionID, messages: messages)
    guard !liveItems.isEmpty else {
      return
    }

    if let lastAssistantIndex = liveActivityInsertionIndex(in: items) {
      items.insert(
        contentsOf: liveItems,
        at: lastAssistantIndex
      )
    } else {
      items.append(contentsOf: liveItems)
    }
  }

  private func liveActivityInsertionIndex(in items: [ChatTranscriptItem]) -> Int? {
    let latestRequestBoundary = items.lastIndex { item in
      guard let message = item.sessionMessage else {
        return false
      }
      return message.role == .user || message.role == .system
    }
    let searchStart = latestRequestBoundary.map { items.index(after: $0) } ?? items.startIndex
    guard searchStart < items.endIndex else {
      return nil
    }

    return items[searchStart...].lastIndex { item in
      switch item {
      case .completedWorkSummary, .finalAnswer:
        return true
      case .message(let message):
        return message.message.role == .assistant && message.footnoteText != nil
      case .toolActivityGroup, .turnSteering:
        return false
      }
    }
  }

  private func liveActivityGroups(
    for sessionID: String,
    messages: [SessionMessage]
  ) -> [ToolActivityGroupRecord] {
    let summarizedGroupIDSet = summarizedActivityGroupIDs(for: sessionID)
    let groups = (liveToolGroupsBySession[sessionID] ?? []).map { group in
      let toolCallsAreRepresented = isLiveToolGroupRepresentedInHistory(group, messages: messages)
      let visibleToolCalls = toolCallsAreRepresented ? [] : group.toolCalls
      return ToolActivityGroupRecord(
        id: group.id,
        toolCalls: visibleToolCalls,
        isLive: group.isLive
      )
    }
    .filter { group in
      !summarizedGroupIDSet.contains(group.id)
    }
    .filter(\.hasVisibleActivity)

    return groups
  }

  private func liveActivityItems(
    for sessionID: String,
    messages: [SessionMessage]
  ) -> [ChatTranscriptItem] {
    let groups = liveActivityGroups(for: sessionID, messages: messages)
    let groupsByID = Dictionary(uniqueKeysWithValues: groups.map { ($0.id, $0) })
    let narrationByActivityID = activeLiveNarrationByActivityID(for: sessionID)
    let timestamp = liveNarrationTimestamp(for: sessionID)
    var items: [ChatTranscriptItem] = []
    var emittedNarrationIDs: Set<String> = []
    var emittedGroupIDs: Set<String> = []

    for activityID in liveActivityOrderBySession[sessionID, default: []] {
      if let narration = narrationByActivityID[activityID],
         emittedNarrationIDs.insert(narration.id).inserted {
        items.append(workingNarrationTranscriptItem(narration, timestamp: timestamp))
      }
      if let group = groupsByID[activityID], emittedGroupIDs.insert(group.id).inserted {
        items.append(.toolActivityGroup(group))
      }
    }

    for group in groups where emittedGroupIDs.insert(group.id).inserted {
      if let narration = narrationByActivityID[group.id],
         emittedNarrationIDs.insert(narration.id).inserted {
        items.append(workingNarrationTranscriptItem(narration, timestamp: timestamp))
      }
      items.append(.toolActivityGroup(group))
    }

    if isSessionStreaming(sessionID),
       let narration = currentActivityNarrationRecord(for: sessionID),
       !emittedNarrationIDs.contains(narration.id) {
      items.append(workingNarrationTranscriptItem(narration, timestamp: timestamp))
    }

    return items
  }

  private func summarizedActivityGroupIDs(for sessionID: String) -> Set<String> {
    // Completed stream summaries are capped to the most recent turns, so this
    // remains bounded and does not need a separate invalidation cache.
    Set(
      completedStreamSummariesBySession[sessionID, default: []]
        .flatMap(\.activityGroups)
        .map(\.id)
    )
  }

  private func normalizedActivityNarration(_ narration: String?) -> String? {
    narration?
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .nonEmpty
  }

  private func currentActivityNarration(for sessionID: String) -> String? {
    normalizedActivityNarration(streamStates[sessionID]?.activityNarration)
  }

  private func activeLiveNarrationByActivityID(for sessionID: String)
    -> [String: WorkingNarrationRecord]
  {
    (retiredNarrationByActivityIDBySession[sessionID] ?? [:])
      .merging(liveNarrationByActivityIDBySession[sessionID] ?? [:]) { _, live in live }
  }

  private func liveNarrationTimestamp(for sessionID: String) -> Int {
    // This timestamp is display metadata only. Relative transcript ordering is
    // driven by liveActivityOrderBySession so async delivery or wall-clock
    // changes cannot reorder narration against related tool activity.
    Int((streamStates[sessionID]?.startedAt ?? Date()).timeIntervalSince1970)
  }

  private func currentActivityNarrationRecord(for sessionID: String) -> WorkingNarrationRecord? {
    guard let narration = currentActivityNarration(for: sessionID) else {
      return nil
    }
    let activityID = Self.liveActivityID(sessionID: sessionID, suffix: "narration")
    return WorkingNarrationRecord(
      id: "\(activityID):narration",
      text: narration,
      isLive: true
    )
  }

  private func persistCurrentActivityNarration(for sessionID: String) {
    guard let narration = currentActivityNarration(for: sessionID) else {
      return
    }

    let activityID = Self.liveActivityID(
      sessionID: sessionID,
      suffix: "narration-\(liveNarrationByActivityIDBySession[sessionID, default: [:]].count + 1)"
    )
    storeLiveNarration(narration, for: sessionID, activityID: activityID, isLive: false)
    streamStates[sessionID]?.activityNarration = ""
  }

  private func storeLiveNarration(
    _ narration: String?,
    for sessionID: String,
    activityID: String,
    isLive: Bool = true
  ) {
    guard let narration = normalizedActivityNarration(narration) else {
      return
    }

    // One narration record belongs to one activity ID. Multiple deltas before
    // an activity boundary are coalesced in streamStates.activityNarration;
    // distinct narration chunks must receive distinct activity IDs so the
    // ordering list can interleave them with tool groups deterministically.
    liveNarrationByActivityIDBySession[sessionID, default: [:]][activityID] =
      WorkingNarrationRecord(
        id: "\(activityID):narration",
        text: narration,
        isLive: isLive
      )
    recordLiveActivityOrder(activityID, for: sessionID)
  }

  private func recordLiveActivityOrder(_ activityID: String, for sessionID: String) {
    guard !liveActivityOrderBySession[sessionID, default: []].contains(activityID) else {
      return
    }
    liveActivityOrderBySession[sessionID, default: []].append(activityID)
  }

  private static func liveActivityID(sessionID: String, suffix: String) -> String {
    "\(liveActivityIDPrefix):\(sessionID):\(suffix)"
  }

  private static func liveCompletedSummaryID(sessionID: String) -> String {
    "\(liveCompletedSummaryIDPrefix):\(sessionID)"
  }

  private static func liveFinalAnswerID(sessionID: String) -> String {
    "\(liveFinalAnswerIDPrefix):\(sessionID)"
  }

  private func appendLiveCompletedSummary(
    for sessionID: String,
    to items: inout [ChatTranscriptItem]
  ) {
    guard isSessionStreaming(sessionID),
          !currentTurnAlreadyHasTerminalSummary(in: items, sessionID: sessionID),
          let summaryText = normalizedBackendCompletedSummaryText(
            streamStates[sessionID]?.completedSummaryText
          )
    else {
      return
    }

    let startedAt = streamStates[sessionID]?.startedAt
    let elapsedText = startedAt.flatMap {
      streamingElapsedFootnoteText(
        startedAt: $0,
        endedAt: Date(),
        minimumSeconds: 1
      )
    } ?? "Worked this turn"
    items.append(
      .completedWorkSummary(
        CompletedWorkSummaryRecord(
          id: Self.liveCompletedSummaryID(sessionID: sessionID),
          elapsedText: elapsedText,
          summaryText: summaryText,
          entries: []
        )
      )
    )
  }

  private func currentTurnAlreadyHasTerminalSummary(
    in items: [ChatTranscriptItem],
    sessionID: String
  ) -> Bool {
    for item in items.reversed() {
      if let message = item.sessionMessage,
         message.role == .user || message.role == .system {
        return false
      }

      switch item {
      case .completedWorkSummary, .finalAnswer:
        return true
      case .message(let message)
        where message.message.role == .assistant
          && (!message.isWorkingNarration
            || completedStreamingSummary(
              for: message.message,
              sessionID: sessionID,
              allowContentDigestFallback: true
            ) != nil):
        return true
      case .message, .toolActivityGroup, .turnSteering:
        continue
      }
    }

    return false
  }

  private func appendLiveFinalAnswer(
    for sessionID: String,
    to items: inout [ChatTranscriptItem]
  ) {
    guard isSessionStreaming(sessionID) else {
      return
    }

    let text = streamingText(for: sessionID)
    guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      return
    }

    let fallbackTimestamp = Int(Date().timeIntervalSince1970)
    let timestamp = assistantMessageTimestamp(
      startedAt: streamStates[sessionID]?.startedAt,
      fallbackUnixTimestamp: fallbackTimestamp
    )
    let message = SessionMessage(
      role: .assistant,
      content: text,
      timestamp: timestamp
    )
    items.append(
      .finalAnswer(
        TranscriptMessage(
          id: Self.liveFinalAnswerID(sessionID: sessionID),
          message: message,
          displayText: text,
          footnoteText: nil,
          isStreaming: true
        )
      )
    )
  }

  private func removePromotedPreviewNarration(_ text: String, for sessionID: String) {
    guard let promotedText = normalizedActivityNarration(text) else {
      return
    }

    var didRemoveNarration = false
    if normalizedActivityNarration(streamStates[sessionID]?.activityNarration) == promotedText {
      streamStates[sessionID]?.activityNarration = ""
      didRemoveNarration = true
    }

    let originalNarrationCount = liveNarrationByActivityIDBySession[sessionID]?.count ?? 0
    if var narrationByActivityID = liveNarrationByActivityIDBySession[sessionID] {
      narrationByActivityID = narrationByActivityID.filter { _, narration in
        normalizedActivityNarration(narration.text) != promotedText
      }
      liveNarrationByActivityIDBySession[sessionID] = narrationByActivityID
    }
    let removedPersistedNarration =
      (liveNarrationByActivityIDBySession[sessionID]?.count ?? 0) != originalNarrationCount

    guard didRemoveNarration || removedPersistedNarration else {
      return
    }

    if liveNarrationByActivityIDBySession[sessionID]?.isEmpty == true {
      liveNarrationByActivityIDBySession.removeValue(forKey: sessionID)
    }

    if !removedPersistedNarration {
      if didRemoveNarration {
        syncThreadRuntimeActivity()
        refreshVisibleTranscriptForToolActivity(
          sessionID: sessionID,
          preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
        )
      }
      return
    }

    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func reconcileLiveToolGroupWithHistory(for sessionID: String, messages: [SessionMessage])
  {
    guard let liveToolGroups = liveToolGroupsBySession[sessionID] else {
      return
    }

    let representedGroups = liveToolGroups.filter {
      isLiveToolGroupRepresentedInHistory($0, messages: messages)
    }
    let remainingGroups = liveToolGroups.filter { group in
      !representedGroups.contains(where: { $0.id == group.id })
    }
    if remainingGroups.count != liveToolGroups.count {
      retireLiveToolGroups(representedGroups, for: sessionID)
      liveToolGroupsBySession[sessionID] = remainingGroups.isEmpty ? nil : remainingGroups
      syncThreadRuntimeActivity()
    }
  }

  private func retireLiveToolGroups(
    _ groups: [ToolActivityGroupRecord],
    for sessionID: String
  ) {
    // History reconciliation hides live overlays once the server echoes the
    // same tool use/result, but completed-work summaries still need the live
    // narration and typed payloads that historical messages do not persist.
    let completedGroups = groups
      .map(completedActivityGroup)
      .filter(\.hasVisibleActivity)
    let completedNarration = liveNarrationByActivityIDBySession[sessionID, default: [:]]
      .filter { activityID, _ in
        groups.contains(where: { $0.id == activityID })
      }
    guard !completedGroups.isEmpty else {
      return
    }

    retiredLiveToolGroupsBySession[sessionID] = Array(Self.uniquedActivityGroups(
      retiredLiveToolGroupsBySession[sessionID, default: []] + completedGroups
    ).suffix(Self.maxRetiredLiveToolGroupsPerSession))
    retiredNarrationByActivityIDBySession[sessionID, default: [:]]
      .merge(completedNarration) { _, live in live }
  }

  private func storeLiveToolGroup(_ toolGroup: ToolActivityGroupRecord, for sessionID: String) {
    recordLiveActivityOrder(toolGroup.id, for: sessionID)
    var groups = liveToolGroupsBySession[sessionID] ?? []
    if let index = groups.firstIndex(where: { $0.id == toolGroup.id }) {
      groups[index] = toolGroup
    } else {
      groups.append(toolGroup)
    }

    liveToolGroupsBySession[sessionID] = groups
  }

  private func updateLastLiveToolGroup(
    for sessionID: String,
    update: (inout ToolActivityGroupRecord) -> Void
  ) {
    var groups = liveToolGroupsBySession[sessionID] ?? []
    guard var toolGroup = groups.popLast() else {
      return
    }

    update(&toolGroup)
    groups.append(toolGroup)
    liveToolGroupsBySession[sessionID] = groups
  }

  private func clearEmptyLiveToolGroups(for sessionID: String) {
    guard let groups = liveToolGroupsBySession[sessionID] else {
      return
    }

    let visibleGroups = groups.filter(\.hasVisibleActivity)
    if visibleGroups.isEmpty {
      liveToolGroupsBySession.removeValue(forKey: sessionID)
    } else {
      liveToolGroupsBySession[sessionID] = visibleGroups
    }
  }

  private func refreshVisibleTranscriptForToolActivity(
    sessionID: String,
    preferPreservePosition: Bool,
    animatedWhenFollowing: Bool = true
  ) {
    guard currentSessionID == sessionID else {
      return
    }

    let cachedMessages = transcriptCache[sessionID] ?? []
    pendingTranscriptScrollBehavior = preferPreservePosition
      ? .preservePosition
      : (animatedWhenFollowing ? .animated : .snap)
    transcriptItems = makeTranscriptItems(for: sessionID, messages: cachedMessages)
  }

  private func beginActivityChunk(sessionID: String, activityID: String, title _: String?) {
    let existingGroups = liveToolGroupsBySession[sessionID] ?? []
    if existingGroups.contains(where: { $0.id == activityID }) {
      if activeLiveNarrationByActivityID(for: sessionID)[activityID] == nil,
         let narration = consumePendingActivityNarration(for: sessionID) {
        storeLiveNarration(narration, for: sessionID, activityID: activityID)
        syncThreadRuntimeActivity()
        refreshVisibleTranscriptForToolActivity(
          sessionID: sessionID,
          preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
        )
      }
      return
    }

    let narration = consumePendingActivityNarration(for: sessionID)
    storeLiveNarration(narration, for: sessionID, activityID: activityID)
    storeLiveToolGroup(
      ToolActivityGroupRecord(
        id: activityID,
        toolCalls: [],
        isLive: true
      ),
      for: sessionID
    )
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func endActivityChunk(sessionID: String, activityID: String) {
    updateLiveToolGroup(sessionID: sessionID, activityID: activityID) { group in
      group.isLive = false
    }
    clearEmptyLiveToolGroups(for: sessionID)
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
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

    storeLiveToolGroup(toolGroup, for: sessionID)
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func beginToolCall(
    sessionID: String,
    activityID: String,
    id: String?,
    name: String?
  ) {
    let toolCallID = stableToolCallID(for: sessionID, rawID: id)
    var toolGroup = liveToolGroup(sessionID: sessionID, activityID: activityID)

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

    storeLiveToolGroup(toolGroup, for: sessionID)
    syncThreadRuntimeActivity()
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

  private func completeToolCall(
    sessionID: String,
    activityID: String,
    id: String?,
    name: String?,
    arguments: String
  ) {
    updateToolCall(sessionID: sessionID, activityID: activityID, id: id) { toolCall in
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

  private func finishToolCall(
    sessionID: String,
    activityID: String,
    id: String?,
    name: String?,
    output: String,
    isError: Bool
  ) {
    updateToolCall(sessionID: sessionID, activityID: activityID, id: id) { toolCall in
      if let name {
        toolCall.name = name
      }
      toolCall.result = output
      toolCall.isRunning = false
      toolCall.isError = isError
    }
  }

  private func applyToolProgress(
    sessionID: String,
    activityID: String?,
    id: String?,
    toolName: String?,
    category: String,
    target: String?,
    advancesSlot: String?,
    outcome: String
  ) {
    let progress = ToolProgressRecord(
      category: category,
      target: target,
      advancesSlot: advancesSlot,
      outcome: outcome
    )
    if let activityID = activityID?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty {
      updateToolCall(sessionID: sessionID, activityID: activityID, id: id) { toolCall in
        if let toolName {
          toolCall.name = toolName
        }
        toolCall.progress = progress
      }
    } else {
      updateToolCall(sessionID: sessionID, id: id) { toolCall in
        if let toolName {
          toolCall.name = toolName
        }
        toolCall.progress = progress
      }
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

    storeLiveToolGroup(toolGroup, for: sessionID)
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func updateToolCall(
    sessionID: String,
    activityID: String,
    id: String?,
    update: (inout ToolCallRecord) -> Void
  ) {
    let toolCallID = stableToolCallID(for: sessionID, rawID: id)
    var toolGroup = liveToolGroup(sessionID: sessionID, activityID: activityID)

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

    storeLiveToolGroup(toolGroup, for: sessionID)
    syncThreadRuntimeActivity()
    refreshVisibleTranscriptForToolActivity(
      sessionID: sessionID,
      preferPreservePosition: shouldPreserveScrollPosition(for: sessionID)
    )
  }

  private func liveToolGroup(
    sessionID: String,
    activityID: String
  ) -> ToolActivityGroupRecord {
    if let existingGroup = liveToolGroupsBySession[sessionID]?.first(where: { $0.id == activityID })
    {
      return existingGroup
    }

    return ToolActivityGroupRecord(
      id: activityID,
      toolCalls: [],
      isLive: true
    )
  }

  private func consumePendingActivityNarration(for sessionID: String) -> String? {
    let narration = currentActivityNarration(for: sessionID)
    streamStates[sessionID]?.activityNarration = ""
    return narration
  }

  private func updateLiveToolGroup(
    sessionID: String,
    activityID: String,
    update: (inout ToolActivityGroupRecord) -> Void
  ) {
    var groups = liveToolGroupsBySession[sessionID] ?? []
    guard let index = groups.firstIndex(where: { $0.id == activityID }) else {
      return
    }

    update(&groups[index])
    liveToolGroupsBySession[sessionID] = groups
  }

  private func activeOrFreshLiveToolGroup(for sessionID: String) -> ToolActivityGroupRecord {
    if var existingGroup = liveToolGroupsBySession[sessionID]?.last,
       existingGroup.isLive,
       !lastLiveActivityOrderEntryIsStandaloneNarration(for: sessionID) {
      existingGroup.isLive = true
      return existingGroup
    }

    let activityID = Self.liveActivityID(
        sessionID: sessionID,
        suffix: "\(liveToolGroupsBySession[sessionID, default: []].count + 1)"
      )
    storeLiveNarration(currentActivityNarration(for: sessionID), for: sessionID, activityID: activityID)
    return ToolActivityGroupRecord(
      id: activityID,
      toolCalls: [],
      isLive: true
    )
  }

  private func lastLiveActivityOrderEntryIsStandaloneNarration(for sessionID: String) -> Bool {
    guard let activityID = liveActivityOrderBySession[sessionID]?.last,
          activeLiveNarrationByActivityID(for: sessionID)[activityID] != nil
    else {
      return false
    }

    return liveToolGroupsBySession[sessionID]?.contains(where: { $0.id == activityID }) != true
  }

  private func retireLiveToolActivity(for sessionID: String) {
    guard var liveToolGroups = liveToolGroupsBySession[sessionID] else {
      return
    }

    guard liveToolGroups.contains(where: \.isLive) else {
      return
    }

    for index in liveToolGroups.indices {
      liveToolGroups[index].isLive = false
    }

    liveToolGroupsBySession[sessionID] = liveToolGroups
    clearEmptyLiveToolGroups(for: sessionID)
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

    let historicalToolUses = Set(
      messages.flatMap { message in
        message.contentBlocks.compactMap { block -> String? in
          guard case .toolUse(let id, _, _) = block else {
            return nil
          }
          return id
        }
      })
    let historicalToolResults = Set(
      messages.flatMap { message in
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

  private func historicalTranscriptParts(from message: SessionMessage)
    -> [HistoricalMessageTranscriptPart]
  {
    var parts: [HistoricalMessageTranscriptPart] = []
    var narrationParts: [String] = []
    var currentToolCalls: [ToolCallRecord] = []
    var emittedToolActivity = false
    let messageContainsToolUse = message.contentBlocks.contains { block in
      toolCall(from: block) != nil
    }

    func joinedNarration(_ values: [String]) -> String? {
      values
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
        .joined(separator: "\n\n")
        .nonEmpty
    }

    func flushCurrentToolGroup() {
      guard !currentToolCalls.isEmpty else {
        return
      }

      parts.append(
        .activity(
          HistoricalToolActivityDraft(
            toolCalls: currentToolCalls
          )
        )
      )
      emittedToolActivity = true
      currentToolCalls = []
    }

    for block in message.contentBlocks {
      if let toolCall = toolCall(from: block) {
        if currentToolCalls.isEmpty {
          if let narration = joinedNarration(narrationParts) {
            parts.append(
              .text(
                HistoricalTextTranscriptDraft(
                  text: narration,
                  isFinalAnswerCandidate: false
                )
              )
            )
          }
          narrationParts = []
        }
        currentToolCalls.append(toolCall)
        continue
      }

      guard
        let displayText = block.transcriptDisplayText?
          .trimmingCharacters(in: .whitespacesAndNewlines)
          .nonEmpty
      else {
        continue
      }

      if !currentToolCalls.isEmpty {
        flushCurrentToolGroup()
      }
      narrationParts.append(displayText)
    }

    flushCurrentToolGroup()

    if let trailingDisplayText = joinedNarration(narrationParts) {
      parts.append(
        .text(
          HistoricalTextTranscriptDraft(
            text: trailingDisplayText,
            isFinalAnswerCandidate: message.role == .assistant
              && (emittedToolActivity || !messageContainsToolUse)
          )
        )
      )
    }

    return parts
  }

  private func toolCalls(from message: SessionMessage) -> [ToolCallRecord] {
    message.contentBlocks.compactMap(toolCall(from:))
  }

  private func toolCall(from block: SessionContentBlock) -> ToolCallRecord? {
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

  private func toolResults(from message: SessionMessage) -> [(
    id: String, output: String, isError: Bool
  )] {
    message.contentBlocks.compactMap { block in
      guard case .toolResult(let toolUseID, let content, let storedIsError) = block else {
        return nil
      }

      let renderedOutput = renderedJSONValue(content)
      let legacyPrefixedError = renderedOutput.hasPrefix("[ERROR]")
      let isError = storedIsError ?? legacyPrefixedError
      let cleanedOutput =
        legacyPrefixedError
        ? renderedOutput.replacingOccurrences(of: "[ERROR] ", with: "")
        : renderedOutput
      return (toolUseID, cleanedOutput, isError)
    }
  }

  private func applyToolResults(
    _ toolResults: [(id: String, output: String, isError: Bool)],
    to items: inout [ChatTranscriptItem],
    groupIndices: [Int]
  ) {
    let toolCallLocations = toolCallLocationsByID(in: items, groupIndices: groupIndices)
    for toolResult in toolResults {
      if let location = toolCallLocations[toolResult.id],
        applyToolResult(toolResult, to: &items, location: location)
      {
        continue
      }

      guard let fallbackGroupIndex = groupIndices.last else {
        continue
      }
      appendUnknownToolResult(toolResult, to: &items, groupIndex: fallbackGroupIndex)
    }
  }

  private struct ToolCallLocation {
    let groupIndex: Int
    let toolIndex: Int
  }

  private func toolCallLocationsByID(
    in items: [ChatTranscriptItem],
    groupIndices: [Int]
  ) -> [String: ToolCallLocation] {
    groupIndices.reduce(into: [:]) { partialResult, groupIndex in
      guard items.indices.contains(groupIndex),
        case .toolActivityGroup(let group) = items[groupIndex]
      else {
        return
      }

      for toolIndex in group.toolCalls.indices {
        partialResult[group.toolCalls[toolIndex].id] = ToolCallLocation(
          groupIndex: groupIndex,
          toolIndex: toolIndex
        )
      }
    }
  }

  private func applyToolResult(
    _ toolResult: (id: String, output: String, isError: Bool),
    to items: inout [ChatTranscriptItem],
    location: ToolCallLocation
  ) -> Bool {
    guard items.indices.contains(location.groupIndex),
      case .toolActivityGroup(var group) = items[location.groupIndex],
      group.toolCalls.indices.contains(location.toolIndex),
      group.toolCalls[location.toolIndex].id == toolResult.id
    else {
      return false
    }

    group.toolCalls[location.toolIndex].result = toolResult.output
    group.toolCalls[location.toolIndex].isRunning = false
    group.toolCalls[location.toolIndex].isError = toolResult.isError
    items[location.groupIndex] = .toolActivityGroup(group)
    return true
  }

  private func appendUnknownToolResult(
    _ toolResult: (id: String, output: String, isError: Bool),
    to items: inout [ChatTranscriptItem],
    groupIndex: Int
  ) {
    guard case .toolActivityGroup(var group) = items[groupIndex] else {
      return
    }

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
    items[groupIndex] = .toolActivityGroup(group)
  }

  private func historicalToolGroupID(
    for message: SessionMessage,
    toolCalls: [ToolCallRecord]
  ) -> String {
    let component =
      toolCalls
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

  private func syncThreadRuntimeActivity() {
    sessionViewModel.syncRuntimeActivity(threadRuntimeActivityBySessionID())
  }

  private func threadRuntimeActivityBySessionID() -> [String: ThreadRuntimeActivity] {
    let sessionIDs = Set(streamStates.keys).union(liveToolGroupsBySession.keys)
    return sessionIDs.reduce(into: [:]) { partialResult, sessionID in
      let streamState = streamStates[sessionID]
      let liveToolGroups = liveToolGroupsBySession[sessionID, default: []].filter(\.isLive)
      let liveToolCallCount = liveToolGroups.reduce(0) { $0 + $1.toolCount }
      let runningToolCallCount = liveToolGroups.reduce(0) { $0 + $1.runningCount }
      let completedToolCallCount = liveToolGroups.reduce(0) { $0 + $1.completedCount }
      let erroredToolCallCount = liveToolGroups.reduce(0) { $0 + $1.errorCount }
      let progressLabel =
        streamState?.progress?.kind.label
        ?? streamState?.transcriptPhase?.composerLabel
        ?? streamState?.phase?.composerLabel
      let progressMessage = streamState?.progress?.message

      let runtimeActivity = ThreadRuntimeActivity(
        isStreaming: streamState != nil,
        liveToolCallCount: liveToolCallCount,
        runningToolCallCount: runningToolCallCount,
        completedToolCallCount: completedToolCallCount,
        erroredToolCallCount: erroredToolCallCount,
        progressLabel: progressLabel,
        progressMessage: progressMessage,
        startedAt: streamState?.startedAt
      )

      guard streamState != nil || runningToolCallCount > 0 else {
        return
      }

      partialResult[sessionID] = runtimeActivity
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
  let steering: String?
  let sessionID: String
}

#if DEBUG
  extension ChatViewModel {
    func makeTranscriptItems(from messages: [SessionMessage]) -> [ChatTranscriptItem] {
      makeTranscriptItems(for: currentSessionID, messages: messages)
    }

    func makeTranscriptItemsForTesting(
      sessionID: String?,
      messages: [SessionMessage]
    ) -> [ChatTranscriptItem] {
      makeTranscriptItems(for: sessionID, messages: messages)
    }

    func appendMessageForTesting(_ message: SessionMessage, sessionID: String) {
      appendMessage(message, for: sessionID)
    }

    func recordCompletedStreamingFootnoteForTesting(
      _ message: SessionMessage,
      sessionID: String,
      startedAt: Date,
      endedAt: Date
    ) {
      recordCompletedStreamingFootnote(
        startedAt: startedAt,
        endedAt: endedAt,
        for: message,
        sessionID: sessionID
      )
    }

    func assistantMessageTimestampForTesting(
      startedAt: Date?,
      fallbackUnixTimestamp: Int
    ) -> Int {
      assistantMessageTimestamp(
        startedAt: startedAt,
        fallbackUnixTimestamp: fallbackUnixTimestamp
      )
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
      hasTypedFinalAnswerEvents: Bool = false,
      phase: StreamingPhase? = nil,
      transcriptPhase: TranscriptPhaseBoundary? = nil,
      progress: StreamingProgress? = nil,
      startedAt: Date = Date()
    ) {
      self.currentSessionID = currentSessionID
      appState.selectContextSession(currentSessionID)
      streamStates.removeAll()
      streamTasks.removeAll()
      streamingDisplayControllers.removeAll()

      if isStreaming, let streamingSessionID {
        var streamState = SessionStreamingState(
          text: streamingText,
          phase: phase,
          transcriptPhase: transcriptPhase,
          progress: progress,
          startedAt: startedAt
        )
        streamState.hasTypedFinalAnswerEvents = hasTypedFinalAnswerEvents
        streamStates[streamingSessionID] = streamState
        _ = streamingDisplayController(for: streamingSessionID)
      }

      syncThreadRuntimeActivity()
    }

    func setStreamingSessionsForTesting(
      _ sessions: [String: (text: String, phase: StreamingPhase?)],
      currentSessionID: String?
    ) {
      setStreamingSessionsForTesting(
        sessions.mapValues { (text: $0.text, phase: $0.phase, progress: nil) },
        currentSessionID: currentSessionID
      )
    }

    func setStreamingSessionsForTesting(
      _ sessions: [String: (text: String, phase: StreamingPhase?, progress: StreamingProgress?)],
      currentSessionID: String?
    ) {
      self.currentSessionID = currentSessionID
      appState.selectContextSession(currentSessionID)
      streamStates = sessions.reduce(into: [:]) { partialResult, entry in
        partialResult[entry.key] = SessionStreamingState(
          text: entry.value.text,
          phase: entry.value.phase,
          progress: entry.value.progress,
          startedAt: Date()
        )
      }
      streamTasks.removeAll()
      streamingDisplayControllers.removeAll()
      for sessionID in sessions.keys {
        _ = streamingDisplayController(for: sessionID)
      }
      syncThreadRuntimeActivity()
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

    func setLiveToolGroupForTesting(
      sessionID: String,
      narration: String? = nil,
      toolCalls: [ToolCallRecord],
      isLive: Bool = true
    ) {
      liveToolGroupsBySession[sessionID] = [
        ToolActivityGroupRecord(
          id: Self.liveActivityID(sessionID: sessionID, suffix: "test"),
          toolCalls: toolCalls,
          isLive: isLive
        )
      ]
      storeLiveNarration(
        narration,
        for: sessionID,
        activityID: Self.liveActivityID(sessionID: sessionID, suffix: "test"),
        isLive: isLive
      )
      syncThreadRuntimeActivity()
    }

    func runtimeActivityForTesting(sessionID: String) -> ThreadRuntimeActivity? {
      threadRuntimeActivityBySessionID()[sessionID]
    }

    func appendStreamingTokenForTesting(_ token: String) {
      guard let currentSessionID else {
        return
      }

      streamingDisplayController(for: currentSessionID).appendToken(token)
    }

    func appendActivityNarrationForTesting(_ text: String, sessionID: String) {
      appendActivityNarration(text, for: sessionID)
    }

    func reduceStreamEventForTesting(_ event: SSEEvent, sessionID: String) async {
      _ = await reduceStreamEvent(event, sessionID: sessionID)
    }

    func markActivityNarrationBoundaryForTesting(sessionID: String) {
      markActivityNarrationBoundary(for: sessionID)
    }

    func removePromotedPreviewNarrationForTesting(_ text: String, sessionID: String) {
      removePromotedPreviewNarration(text, for: sessionID)
    }

    func resetStreamingTextForTesting(sessionID: String) {
      resetStreamingText(for: sessionID)
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
    ) -> (text: String, attachments: [PendingAttachment], steering: String?, sessionID: String?)? {
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

    func completeToolCallForTesting(
      sessionID: String, id: String?, name: String?, arguments: String
    ) {
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
      guard let context else {
        appState.clearContext()
        return
      }

      appState.selectContextSession(currentSessionID)
      appState.setContext(context, for: currentSessionID)
    }

    var currentContextForTesting: ContextInfo? {
      appState.currentContext
    }
  }
#endif
