import Observation
import SwiftUI
import UserNotifications

enum ConnectionStatus: String, Sendable {
  case disconnected
  case connecting
  case connected
  case reconnecting
}

enum AppTheme: String, CaseIterable, Sendable {
  case system
  case light
  case dark

  var displayName: String {
    rawValue.capitalized
  }

  var colorScheme: ColorScheme? {
    switch self {
    case .system:
      nil
    case .light:
      .light
    case .dark:
      .dark
    }
  }
}

enum AppFontSize: String, CaseIterable, Sendable {
  case small
  case medium
  case large

  var displayName: String {
    rawValue.capitalized
  }

  var scale: CGFloat {
    switch self {
    case .small:
      0.92
    case .medium:
      1
    case .large:
      1.12
    }
  }

  var sliderValue: Double {
    switch self {
    case .small:
      0
    case .medium:
      1
    case .large:
      2
    }
  }

  static func fromSliderValue(_ value: Double) -> AppFontSize {
    switch Int(value.rounded()) {
    case ...0:
      .small
    case 2...:
      .large
    default:
      .medium
    }
  }
}

enum AppToastStyle: Sendable, Equatable {
  case info
  case success
  case warning
  case error
}

struct AppToast: Identifiable, Equatable, Sendable {
  let id = UUID()
  let message: String
  let style: AppToastStyle
}

enum ConnectionBannerTone: Sendable, Equatable {
  case warning
  case error
}

struct ConnectionBannerState: Sendable, Equatable {
  let message: String
  let tone: ConnectionBannerTone
  let showsRetry: Bool
}

enum AppConnectionMode: String, Sendable {
  case local
  case remote
}

enum AppRootDestination: Sendable, Equatable {
  case main
  case setupWizard
  case remoteOnboarding
}

enum RefreshCadence {
  static let dashboardPanels: Duration = .seconds(30)
}

@MainActor
@Observable
final class AppState {
  private struct StartupHydrationOverride: OptionSet {
    let rawValue: Int

    static let navigation = StartupHydrationOverride(rawValue: 1 << 0)
    static let theme = StartupHydrationOverride(rawValue: 1 << 1)
    static let fontSize = StartupHydrationOverride(rawValue: 1 << 2)
    static let modelFavorites = StartupHydrationOverride(rawValue: 1 << 3)
    static let accentColor = StartupHydrationOverride(rawValue: 1 << 4)
  }

  private struct ContextSessionID: Equatable {
    let rawValue: String

    init?(_ sessionID: String?) {
      guard let rawValue = sessionID?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty else {
        return nil
      }

      self.rawValue = rawValue
    }
  }

  private struct LaunchState {
    var connectionMode: AppConnectionMode
    var rootDestination: AppRootDestination
    var serverURLString: String
    var pairedDeviceName: String?
    var theme: AppTheme
    var fontSize: AppFontSize
    var accentColor: AppAccentColor
    var favoriteModelIDs: Set<String>
    var authToken: String?
    var localInstallConfiguration: LocalInstallConfiguration?
    var isSetupComplete: Bool

    var baseURL: URL? {
      URL(string: serverURLString)
    }

    var isConfigured: Bool {
      !serverURLString.isEmpty && authToken?.isEmpty == false
    }

    @MainActor
    static func resolved(from snapshot: AppStatePersistence.LaunchSnapshot) -> LaunchState {
      let resolvedPairing = resolvePairing(from: snapshot)
      let isSetupComplete =
        snapshot.setupComplete || resolvedPairing.localInstallConfiguration != nil

      return LaunchState(
        connectionMode: resolvedPairing.connectionMode,
        rootDestination: AppState.resolveInitialDestination(
          isConfigured: !resolvedPairing.serverURLString.isEmpty
            && resolvedPairing.authToken?.isEmpty == false,
          setupComplete: isSetupComplete,
          connectionMode: resolvedPairing.connectionMode,
          hasStoredServerURL: !snapshot.storedServerURL.isEmpty,
          hasLocalInstall: resolvedPairing.localInstallConfiguration != nil
        ),
        serverURLString: resolvedPairing.serverURLString,
        pairedDeviceName: AppState.resolvedDeviceName(
          storedDeviceName: snapshot.pairedDeviceName,
          authToken: resolvedPairing.authToken
        ),
        theme: snapshot.theme,
        fontSize: snapshot.fontSize,
        accentColor: snapshot.accentColor,
        favoriteModelIDs: snapshot.favoriteModelIDs,
        authToken: resolvedPairing.authToken,
        localInstallConfiguration: resolvedPairing.localInstallConfiguration,
        isSetupComplete: isSetupComplete
      )
    }

    @MainActor
    private static func resolvePairing(
      from snapshot: AppStatePersistence.LaunchSnapshot
    ) -> (
      serverURLString: String,
      authToken: String?,
      connectionMode: AppConnectionMode,
      localInstallConfiguration: LocalInstallConfiguration?
    ) {
      let localInstallConfiguration = snapshot.localInstallConfiguration
      var serverURLString = UITestLaunchOptions.serverURLOverride ?? snapshot.storedServerURL
      var authToken = UITestLaunchOptions.bearerTokenOverride ?? snapshot.authToken
      var connectionMode = AppState.resolveLaunchConnectionMode(
        storedConnectionModeRawValue: snapshot.connectionModeRawValue,
        storedServerURL: snapshot.storedServerURL,
        effectiveServerURLString: serverURLString
      )

      #if os(macOS)
        if let localInstallConfiguration,
          connectionMode == .local || serverURLString.isEmpty || authToken?.isEmpty != false
        {
          serverURLString = localInstallConfiguration.baseURLString
          authToken = localInstallConfiguration.bearerToken
          connectionMode = .local
        } else if serverURLString.isEmpty && connectionMode == .local {
          serverURLString = AppState.defaultLocalSetupServerURLString
        }
      #endif

      return (serverURLString, authToken, connectionMode, localInstallConfiguration)
    }
  }

  private static let defaultLocalSetupServerURLString = "http://127.0.0.1:8400"

  var connectionStatus: ConnectionStatus = .disconnected
  var connectionMode: AppConnectionMode
  var rootDestination: AppRootDestination
  var serverURLString: String {
    didSet {
      debugLogServerURLMutation(from: oldValue, to: serverURLString)
    }
  }
  var pairedDeviceName: String?
  var activeModel: ModelInfo?
  var thinkingLevel: ThinkingLevel?
  var availableThinkingLevels: [ThinkingLevel] = []
  var availableModels: [ModelInfo] = []
  var skills: [SkillSummary] = []
  var authProviders: [AuthProvider] = []
  var lastHealth: HealthResponse?
  var currentContext: ContextInfo?
  var permissionPresetName = "Power User"
  var permissionMode: PermissionMode = .prompt
  var ripcordStatus: RipcordStatusResponse? {
    didSet {
      guard let ripcordStatus, ripcordStatus.shouldSurfaceNotification else {
        ripcordNotificationDismissedID = nil
        return
      }

      if oldValue?.shouldSurfaceNotification != true
        || oldValue?.notificationID != ripcordStatus.notificationID
      {
        ripcordNotificationDismissedID = nil
      }
    }
  }
  var connectionError: String?
  var theme: AppTheme
  var fontSize: AppFontSize
  var accentColor: AppAccentColor
  var favoriteModelIDs: Set<String>
  var isUpdatingServerSettings = false
  var authProvidersError: String?
  var serverStatusError: String?
  var sidebarSelection: SidebarSelection?
  var toast: AppToast?
  var setupStatus: SetupStatusResponse?
  var localServerStatus: LocalServerRuntimeStatus?
  var launchAgentStatus: LaunchAgentStatusResponse?
  var qrPairingResponse: QrPairingResponse?
  var localInstallConfiguration: LocalInstallConfiguration?
  var setupActionError: String?

  let client: FawxClient
  @ObservationIgnored private let persistence: AppStatePersistence
  private var authToken: String?
  private var isSetupComplete: Bool
  @ObservationIgnored private var reconnectTask: Task<Void, Never>?
  @ObservationIgnored private var hasRequestedNotificationAuthorization = false
  @ObservationIgnored private var toastDismissTask: Task<Void, Never>?
  @ObservationIgnored private var reconnectAttempt = 0
  @ObservationIgnored private var startupHydrationOverrides: StartupHydrationOverride = []
  @ObservationIgnored private var refreshServerStateTask: Task<Void, Error>?
  @ObservationIgnored private var refreshPhase4StateTask: Task<Void, Never>?
  @ObservationIgnored private var refreshRipcordStateTask: Task<Void, Never>?
  @ObservationIgnored private var currentContextSessionID: ContextSessionID?
  @ObservationIgnored private var contextRefreshSequence: UInt64 = 0
  @ObservationIgnored private var hasAttemptedLaunchAgentRepair = false
  private var ripcordNotificationDismissedID: String?

  @ObservationIgnored private var initialPersistenceLoadTask: Task<Void, Never>?

  init(
    persistence: AppStatePersistence = AppStatePersistence.defaultStore(),
    client: FawxClient? = nil,
    startLoadingPersistedState: Bool = true
  ) {
    let initialState = Self.initialLaunchState()

    self.connectionMode = initialState.connectionMode
    self.rootDestination = initialState.rootDestination
    self.serverURLString = initialState.serverURLString
    self.pairedDeviceName = initialState.pairedDeviceName
    self.theme = initialState.theme
    self.fontSize = initialState.fontSize
    self.accentColor = initialState.accentColor
    FawxAccentPalette.update(initialState.accentColor)
    self.favoriteModelIDs = initialState.favoriteModelIDs
    self.authToken = initialState.authToken
    self.localInstallConfiguration = initialState.localInstallConfiguration
    self.isSetupComplete = initialState.isSetupComplete
    self.client =
      client ?? FawxClient(baseURL: initialState.baseURL, bearerToken: initialState.authToken)
    self.persistence = persistence

    if startLoadingPersistedState {
      startPersistedStateLoad(resetState: UITestLaunchOptions.shouldResetState)
    }
  }

  var isConfigured: Bool {
    guard !serverURLString.isEmpty else {
      return false
    }
    guard let authToken, !authToken.isEmpty else {
      return false
    }
    return true
  }

  var showsMainExperience: Bool {
    rootDestination == .main
  }

  var isRemoteClient: Bool {
    connectionMode == .remote
  }

  var canOpenRemoteOnboarding: Bool {
    #if os(macOS)
      true
    #else
      false
    #endif
  }

  var localLogFileURL: URL? {
    localInstallConfiguration?.logFileURL
  }

  var advertisedHost: String? {
    if let displayHost = qrPairingResponse?.displayHost, !displayHost.isEmpty {
      return displayHost
    }
    if let hostname = setupStatus?.tailscale.hostname, !hostname.isEmpty {
      return hostname
    }
    return nil
  }

  var displayedHost: String {
    if let host = URL(string: serverURLString)?.host, !host.isEmpty {
      return host
    }
    if let host = localServerStatus?.host, !host.isEmpty {
      return host
    }
    if let host = setupStatus?.localServer.host, !host.isEmpty {
      return host
    }
    return "Not connected"
  }

  var displayedPort: Int? {
    if let port = URL(string: serverURLString)?.port {
      return port
    }
    if let port = localServerStatus?.port {
      return port
    }
    if let port = setupStatus?.localServer.port {
      return port
    }
    return nil
  }

  var displayedServerURLString: String {
    if !serverURLString.isEmpty {
      return serverURLString
    }
    let host = localServerStatus?.host ?? setupStatus?.localServer.host
    let port = localServerStatus?.port ?? setupStatus?.localServer.port
    let prefersHTTPS =
      localServerStatus?.httpsEnabled ?? setupStatus?.localServer.httpsEnabled ?? false
    if let host, let port {
      return "\(prefersHTTPS ? "https" : "http")://\(host):\(port)"
    }
    return ""
  }

  var serverStatusLabel: String {
    switch connectionStatus {
    case .connected:
      return localServerStatus?.status.capitalized ?? "Connected"
    case .connecting:
      return "Connecting"
    case .reconnecting:
      return "Reconnecting"
    case .disconnected:
      if let localServerStatus, localServerStatus.status == "stopped" {
        return "Stopped"
      }
      return "Disconnected"
    }
  }

  var autoStartEnabled: Bool {
    launchAgentStatus?.installed
      ?? setupStatus?.launchagent.autoStartEnabled
      ?? false
  }

  var canManageServerLocally: Bool {
    #if os(macOS)
      connectionMode == .local
    #else
      false
    #endif
  }

  var configurationKey: String {
    [
      serverURLString,
      pairedDeviceName ?? "",
      isConfigured ? "paired" : "unpaired",
      connectionMode.rawValue,
      rootDestinationKey,
    ].joined(separator: "|")
  }

  var setupWizardKey: String {
    [
      localInstallConfiguration?.baseURLString ?? "",
      setupStatus?.mode ?? "",
      setupStatus?.launchagent.loaded == true ? "launchagent" : "no-launchagent",
      setupStatus?.tailscale.hostname ?? "",
      authProviders.map(\.provider).joined(separator: ","),
    ].joined(separator: "|")
  }

  var preferredColorScheme: ColorScheme? {
    theme.colorScheme
  }

  var connectionBanner: ConnectionBannerState? {
    guard showsMainExperience, isConfigured, let connectionError, !connectionError.isEmpty else {
      if showsMainExperience, connectionStatus == .reconnecting {
        return ConnectionBannerState(
          message: "Reconnecting to Fawx server at \(serverURLString)...",
          tone: .warning,
          showsRetry: false
        )
      }
      return nil
    }

    switch connectionStatus {
    case .reconnecting:
      return ConnectionBannerState(message: connectionError, tone: .warning, showsRetry: false)
    case .disconnected:
      return ConnectionBannerState(message: connectionError, tone: .error, showsRetry: true)
    case .connecting, .connected:
      return nil
    }
  }

  var activeRipcordStatus: RipcordStatusResponse? {
    guard let ripcordStatus, ripcordStatus.shouldSurfaceNotification else {
      return nil
    }
    guard ripcordNotificationDismissedID != ripcordStatus.notificationID else {
      return nil
    }
    return ripcordStatus
  }

  func dismissRipcordNotification() {
    guard let ripcordStatus, ripcordStatus.shouldSurfaceNotification else {
      return
    }

    ripcordNotificationDismissedID = ripcordStatus.notificationID
  }

  func bootstrap() async {
    await awaitPersistedStateLoad()
    await synchronizeLocalConnectionIfNeeded()

    guard showsMainExperience, isConfigured else {
      reconnectTask?.cancel()
      reconnectTask = nil
      if showsMainExperience {
        connectionStatus = .disconnected
      }
      clearRipcordState()
      return
    }

    reconnectAttempt = 0
    await attemptConnection(initialStatus: .connecting, allowReconnect: true)
    await refreshPhase4State()
    await refreshRipcordState()
  }

  func beginRemoteOnboarding() {
    recordStartupHydrationOverride(.navigation)
    connectionMode = .remote
    rootDestination = .remoteOnboarding
    setupActionError = nil
    persistConnectionMode()
  }

  func returnToLocalSetup() {
    #if os(macOS)
      recordStartupHydrationOverride(.navigation)
      connectionMode = .local
      rootDestination = .setupWizard
      setupActionError = nil
      persistConnectionMode()
    #endif
  }

  /// Configure the HTTP client for a bootstrapped server without changing
  /// navigation state or persisting pairing. Used by the setup wizard to
  /// enable API calls on the provider step before setup is complete.
  func configureClientForBootstrap(serverURL: String, bearerToken: String) async throws {
    guard let url = URL(string: serverURL) else {
      throw APIError.invalidURL(serverURL)
    }
    await client.updateConfiguration(baseURL: url, bearerToken: bearerToken)
  }

  func completeLocalSetup(
    progress: @escaping @MainActor @Sendable (String) -> Void = { _ in }
  ) async throws {
    await awaitPersistedStateLoad()

    progress("Checking for an existing Fawx install...")
    if let existingConfig = await refreshLocalInstallConfiguration(),
      !existingConfig.bearerToken.isEmpty
    {
      try await adoptAndConnect(
        serverURL: existingConfig.baseURLString,
        bearerToken: existingConfig.bearerToken,
        progress: progress
      )
      return
    }

    let bootstrapService = LocalBootstrapService()
    let result = try await bootstrapService.performFullBootstrap(progress: progress)
    let installedConfig = await refreshLocalInstallConfiguration()
    let serverURL = installedConfig?.baseURLString ?? "http://\(result.host):\(result.port)"
    let bearerToken = installedConfig?.bearerToken ?? result.bearerToken
    try await adoptAndConnect(serverURL: serverURL, bearerToken: bearerToken, progress: progress)
  }

  func savePairing(
    serverURLString rawURL: String,
    token: String,
    deviceName: String,
    connectionMode: AppConnectionMode = .remote
  ) async throws {
    guard let canonicalURLString = canonicalizeServerURL(rawURL) else {
      throw APIError.invalidURL(rawURL)
    }
    guard let url = URL(string: canonicalURLString) else {
      throw APIError.invalidURL(canonicalURLString)
    }

    await awaitPersistedStateLoad()
    try await persistence.savePairing(
      serverURLString: canonicalURLString,
      token: token,
      deviceName: deviceName,
      connectionMode: connectionMode
    )

    serverURLString = canonicalURLString
    pairedDeviceName = deviceName
    authToken = token
    self.connectionMode = connectionMode
    rootDestination = .main

    await client.updateConfiguration(baseURL: url, bearerToken: token)
    connectionError = nil
    serverStatusError = nil
    connectionStatus = .connected
    setupActionError = nil
  }

  func unpair() async throws {
    await awaitPersistedStateLoad()
    await persistence.clearPairing(serverURLString: serverURLString)

    let hasLocalInstall = localInstallConfiguration != nil
    let setupComplete = isSetupComplete
    let previousConnectionMode = connectionMode

    authToken = nil
    pairedDeviceName = nil
    lastHealth = nil
    activeModel = nil
    thinkingLevel = nil
    availableThinkingLevels = []
    clearContext()
    availableModels = []
    skills = []
    authProviders = []
    permissionPresetName = "Power User"
    permissionMode = .prompt
    ripcordStatus = nil
    connectionError = nil
    connectionStatus = .disconnected
    serverStatusError = nil
    setupStatus = nil
    localServerStatus = nil
    launchAgentStatus = nil
    qrPairingResponse = nil
    clearRipcordState()

    await client.updateConfiguration(baseURL: nil, bearerToken: nil)

    rootDestination = Self.resolveInitialDestination(
      isConfigured: false,
      setupComplete: setupComplete,
      connectionMode: previousConnectionMode,
      hasStoredServerURL: false,
      hasLocalInstall: hasLocalInstall && previousConnectionMode == .local
    )
  }

  func refreshServerState() async throws {
    try await coalesceThrowingTask(\.refreshServerStateTask) { [self] in
      async let modelsTask = self.client.listModels()
      async let legacyStatusTask = self.client.serverStatus()
      async let permissionsTask = self.client.getPermissions()

      let models = try await modelsTask
      let legacyStatus: ServerStatusResponse?
      do {
        legacyStatus = try await legacyStatusTask
        self.serverStatusError = nil
      } catch {
        legacyStatus = nil
        self.serverStatusError = error.localizedDescription
      }

      let permissionsResponse: PermissionsResponse?
      do {
        permissionsResponse = try await permissionsTask
      } catch {
        permissionsResponse = nil
      }

      do {
        let thinking = try await self.client.thinking()
        self.thinkingLevel = thinking.level
        self.availableThinkingLevels = thinking.validLevels
      } catch {
        self.thinkingLevel = nil
        self.availableThinkingLevels = []
      }

      await self.refreshAuthProviders()

      self.availableModels = models.models
      let activeModelID = legacyStatus?.model ?? models.activeModel
      self.activeModel =
        models.models.first(where: { $0.modelID == activeModelID }) ?? models.models.first
      if let permissionsResponse {
        self.permissionPresetName = permissionPresetLabel(permissionsResponse.preset)
        self.permissionMode = permissionsResponse.mode
      } else {
        self.permissionPresetName = self.resolvePermissionPreset(from: legacyStatus?.config)
      }
      await self.refreshPhase4State()
    }
  }

  func refreshAuthProviders() async {
    do {
      let auth = try await client.authProviders()
      authProviders = auth.providers.sorted { lhs, rhs in
        if lhs.isConfigured != rhs.isConfigured {
          return lhs.isConfigured && !rhs.isConfigured
        }
        return lhs.displayName.localizedCaseInsensitiveCompare(rhs.displayName) == .orderedAscending
      }
      authProvidersError = nil
    } catch {
      authProviders = []
      authProvidersError = error.localizedDescription
    }
  }

  func refreshSettingsState() async {
    if isConfigured {
      do {
        try await refreshServerState()
        return
      } catch {
        await noteRecoverableRequestFailure(error)
      }
    }
    await refreshAuthProviders()
    await refreshPhase4State()
  }

  func refreshRipcordState() async {
    await coalesceTask(\.refreshRipcordStateTask) { [self] in
      guard self.isConfigured else {
        self.clearRipcordState()
        return
      }

      do {
        let status = try await self.client.ripcordStatus()
        let previousStatus = self.ripcordStatus
        self.ripcordStatus = status

        if self.shouldNotifyForRipcordActivation(from: previousStatus, to: status) {
          self.postRipcordNotification(for: status)
        }
      } catch let error as APIError where error.statusCode == 503 {
        self.clearRipcordState()
      } catch {
        if ConnectionStateMachine.shouldHandleAsConnectionIssue(error) {
          await self.noteRecoverableRequestFailure(error)
        }
      }
    }
  }

  func loadRipcordJournal() async throws -> [JournalEntry] {
    let response = try await client.ripcordJournal()
    return response.entries.sorted { lhs, rhs in
      lhs.id < rhs.id
    }
  }

  func pullRipcord() async throws -> RipcordReport {
    let report = try await client.pullRipcord()
    ripcordStatus = .inactive
    return report
  }

  func approveRipcord() async throws {
    try await client.approveRipcord()
    ripcordStatus = .inactive
  }

  func refreshPhase4State() async {
    await coalesceTask(\.refreshPhase4StateTask) { [self] in
      let canQueryLocalSetupServer: Bool
      #if os(macOS)
        canQueryLocalSetupServer =
          self.rootDestination == .setupWizard
          && self.connectionMode == .local
          && !self.serverURLString.isEmpty
      #else
        canQueryLocalSetupServer = false
      #endif

      guard self.isConfigured || canQueryLocalSetupServer else {
        self.setupStatus = nil
        self.localServerStatus = nil
        self.launchAgentStatus = nil
        self.qrPairingResponse = nil
        return
      }

      do {
        self.setupStatus = try await self.client.setupStatus()
        self.setupActionError = nil
      } catch {
        self.setupStatus = nil
      }

      do {
        self.localServerStatus = try await self.client.runtimeStatus()
      } catch {
        if self.localServerStatus?.status != "stopped" {
          self.localServerStatus = nil
        }
      }

      do {
        self.launchAgentStatus = try await self.client.launchAgentStatus()
      } catch {
        self.launchAgentStatus = nil
      }

      do {
        self.qrPairingResponse = try await self.client.qrPairing()
      } catch {
        self.qrPairingResponse = nil
      }

      await self.repairLaunchAgentIfNeeded()
    }
  }

  func setLaunchAgentEnabled(_ enabled: Bool) async throws -> String {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    if enabled {
      let response = try await client.installLaunchAgent(autoStart: true)
      showToast(message: response.message, style: .success)
      await refreshPhase4State()
      return response.message
    } else {
      let response = try await client.uninstallLaunchAgent()
      showToast(message: response.message, style: .info)
      await refreshPhase4State()
      return response.message
    }
  }

  func restartLocalServer() async throws -> String {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.restartServer()
    connectionStatus = .reconnecting
    if let localServerStatus {
      self.localServerStatus = LocalServerRuntimeStatus(
        status: "starting",
        version: localServerStatus.version,
        uptimeSeconds: 0,
        pid: localServerStatus.pid,
        host: localServerStatus.host,
        port: localServerStatus.port,
        httpsEnabled: localServerStatus.httpsEnabled
      )
    }
    showToast(message: response.message, style: .info)
    await waitForLocalServerReconnect()
    await revalidateConnection(allowReconnect: true)
    await refreshPhase4State()
    return response.message
  }

  func stopLocalServer() async throws -> String {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.stopServer()
    reconnectTask?.cancel()
    reconnectTask = nil
    reconnectAttempt = 0
    connectionStatus = .disconnected
    connectionError =
      connectionMode == .local ? nil : connectionMessage(for: APIError.invalidResponse)
    localServerStatus = LocalServerRuntimeStatus(
      status: "stopped",
      version: localServerStatus?.version ?? "",
      uptimeSeconds: 0,
      pid: 0,
      host: localServerStatus?.host ?? setupStatus?.localServer.host ?? displayedHost,
      port: localServerStatus?.port ?? setupStatus?.localServer.port ?? displayedPort ?? 8400,
      httpsEnabled: localServerStatus?.httpsEnabled ?? setupStatus?.localServer.httpsEnabled
        ?? false
    )
    showToast(message: response.message, style: .info)
    await refreshPhase4State()
    return response.message
  }

  func updateServerPort(_ port: Int) async throws -> ConfigPatchResponse {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.patchConfig(
      changes: .object([
        "http": .object([
          "port": .number(Double(port))
        ])
      ])
    )
    if response.restartRequired {
      showToast(message: "Port updated. Restart the server to apply it.", style: .info)
    } else {
      showToast(message: "Server port updated.", style: .success)
    }
    await refreshPhase4State()
    return response
  }

  func updateWorkspaceRoot(
    _ path: String,
    showToast: Bool = true
  ) async throws -> ConfigPatchResponse {
    try await patchWorkspaceRoot(.string(path), showToast: showToast)
  }

  func clearWorkspaceRoot(showToast: Bool = true) async throws -> ConfigPatchResponse {
    try await patchWorkspaceRoot(.null, showToast: showToast)
  }

  private func patchWorkspaceRoot(
    _ pathValue: JSONValue,
    showToast: Bool
  ) async throws -> ConfigPatchResponse {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.patchConfig(
      changes: .object([
        "workspace": .object([
          "root": pathValue
        ]),
        "tools": .object([
          "working_dir": pathValue
        ]),
      ])
    )

    if showToast {
      if response.restartRequired {
        self.showToast(message: "Workspace updated. Restart the server to apply it.", style: .info)
      } else {
        self.showToast(message: "Workspace updated.", style: .success)
      }
    }

    try await refreshServerState()
    return response
  }

  func fetchPairingQRCode() async throws -> QrPairingResponse {
    let response = try await client.qrPairing()
    qrPairingResponse = response
    return response
  }

  func generatePairingCode() async throws -> PairingCodeResponse {
    try await client.generatePairingCode()
  }

  func requestTailscaleCertificate(hostname: String) async throws -> TailscaleCertResponse {
    let response = try await client.tailscaleCert(hostname: hostname)
    await refreshPhase4State()
    return response
  }

  func storeAnthropicSetupToken(_ token: String) async throws -> ProviderAuthActionResponse {
    let response = try await client.exchangeAnthropicSetupToken(token)
    if isConfigured {
      try await refreshServerState()
    } else {
      await refreshPhase4State()
    }
    return response
  }

  func storeProviderAPIKey(provider: String, apiKey: String) async throws
    -> ProviderAuthActionResponse
  {
    let response = try await client.storeAPIKey(provider: provider, apiKey: apiKey)
    if isConfigured {
      try await refreshServerState()
    } else {
      await refreshPhase4State()
    }
    return response
  }

  func startOAuth(provider: String) async throws -> OAuthStartResponse {
    try await client.oauthStart(provider: provider)
  }

  func completeOAuth(
    provider: String,
    code: String,
    flowToken: String
  ) async throws -> OAuthCallbackResponse {
    let response = try await client.oauthCallback(
      provider: provider,
      code: code,
      flowToken: flowToken
    )
    if isConfigured {
      try await refreshServerState()
    } else {
      await refreshPhase4State()
    }
    return response
  }

  func verifyProvider(_ provider: String) async throws -> ProviderVerificationResponse {
    let response = try await client.verifyProvider(provider)
    if isConfigured {
      try await refreshServerState()
    } else {
      await refreshPhase4State()
    }
    return response
  }

  func deleteProvider(_ provider: String) async throws {
    _ = try await client.deleteProvider(provider)
    try await refreshServerState()
  }

  func retryConnection() async {
    reconnectTask?.cancel()
    reconnectTask = nil
    reconnectAttempt = 0
    await attemptConnection(initialStatus: .connecting, allowReconnect: true)
  }

  func revalidateConnection(allowReconnect: Bool = true) async {
    guard isConfigured else {
      reconnectTask?.cancel()
      reconnectTask = nil
      reconnectAttempt = 0
      lastHealth = nil
      connectionError = nil
      connectionStatus = .disconnected
      clearRipcordState()
      return
    }

    reconnectTask?.cancel()
    reconnectTask = nil
    reconnectAttempt = 0

    let initialStatus: ConnectionStatus = allowReconnect ? .reconnecting : .connecting
    await attemptConnection(initialStatus: initialStatus, allowReconnect: allowReconnect)
    await refreshPhase4State()
  }

  func markDisconnected(from error: Error) {
    reconnectTask?.cancel()
    reconnectTask = nil
    reconnectAttempt = 0
    lastHealth = nil
    connectionError = connectionMessage(for: error)
    connectionStatus = .disconnected
    clearRipcordState()
  }

  func userFacingConnectionMessage(for error: Error) -> String {
    connectionMessage(for: error)
  }

  func noteRecoverableRequestFailure(_ error: Error) async {
    guard ConnectionStateMachine.shouldHandleAsConnectionIssue(error) else {
      return
    }

    await handleConnectionFailure(error, allowReconnect: true)
  }

  func refreshContext(for sessionID: String?) async {
    let contextSessionID = ContextSessionID(sessionID)
    let refreshSequence = beginContextSelection(for: contextSessionID)

    guard let contextSessionID else {
      return
    }

    do {
      let context = try await client.sessionContext(id: contextSessionID.rawValue)
      guard ownsContextSelection(contextSessionID, refreshSequence: refreshSequence) else {
        return
      }
      currentContext = context
    } catch {
      guard ownsContextSelection(contextSessionID, refreshSequence: refreshSequence) else {
        return
      }
      currentContext = nil
    }
  }

  /// Selects the session allowed to own `currentContext`.
  ///
  /// Callers pass raw API session IDs; `AppState` normalizes them once at this boundary and stores
  /// only the private `ContextSessionID` wrapper so ownership comparisons cannot drift.
  func selectContextSession(_ sessionID: String?) {
    let contextSessionID = ContextSessionID(sessionID)
    guard contextSessionID != currentContextSessionID else {
      return
    }

    _ = beginContextSelection(for: contextSessionID)
  }

  /// Applies a live context update for the selected session and invalidates older refreshes.
  ///
  /// Streaming compaction events are fresher than any already in-flight context fetch, so accepting
  /// one intentionally prevents a late HTTP response from repainting stale token usage.
  func setContext(_ context: ContextInfo, for sessionID: String?) {
    guard ContextSessionID(sessionID) == currentContextSessionID else {
      return
    }

    advanceContextRefreshSequence()
    currentContext = context
  }

  func clearContext() {
    _ = beginContextSelection(for: nil)
  }

  private func beginContextSelection(for sessionID: ContextSessionID?) -> UInt64 {
    advanceContextRefreshSequence()
    let didChangeSession = currentContextSessionID != sessionID
    currentContextSessionID = sessionID
    if didChangeSession || sessionID == nil {
      currentContext = nil
    }
    return contextRefreshSequence
  }

  private func advanceContextRefreshSequence() {
    precondition(
      contextRefreshSequence < UInt64.max,
      "Context refresh sequence exhausted before process restart."
    )
    contextRefreshSequence += 1
  }

  private func ownsContextSelection(_ sessionID: ContextSessionID, refreshSequence: UInt64) -> Bool {
    contextRefreshSequence == refreshSequence && currentContextSessionID == sessionID
  }

  private func coalesceTask(
    _ keyPath: ReferenceWritableKeyPath<AppState, Task<Void, Never>?>,
    operation: @escaping @MainActor () async -> Void
  ) async {
    if let task = self[keyPath: keyPath] {
      await task.value
      return
    }

    let task = Task { @MainActor [self] in
      defer { self[keyPath: keyPath] = nil }
      await operation()
    }

    self[keyPath: keyPath] = task
    await task.value
  }

  private func coalesceThrowingTask(
    _ keyPath: ReferenceWritableKeyPath<AppState, Task<Void, Error>?>,
    operation: @escaping @MainActor () async throws -> Void
  ) async throws {
    if let task = self[keyPath: keyPath] {
      try await task.value
      return
    }

    let task = Task { @MainActor [self] in
      defer { self[keyPath: keyPath] = nil }
      try await operation()
    }

    self[keyPath: keyPath] = task
    try await task.value
  }

  func setModel(_ modelID: String) async throws {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.setModel(modelID)
    activeModel =
      availableModels.first(where: { $0.modelID == response.activeModel }) ?? activeModel

    if let thinkingAdjusted = response.thinkingAdjusted {
      thinkingLevel = thinkingAdjusted.to
      showToast(
        message:
          "Thinking adjusted to \(displayThinkingLevel(thinkingAdjusted.to, modelID: response.activeModel)).",
        style: .info
      )
    }

    try await refreshServerState()
  }

  func setThinking(_ level: ThinkingLevel) async throws {
    isUpdatingServerSettings = true
    defer { isUpdatingServerSettings = false }

    let response = try await client.setThinking(level)
    thinkingLevel = response.level
    availableThinkingLevels = response.validLevels
    try await refreshServerState()
  }

  func setTheme(_ theme: AppTheme) {
    recordStartupHydrationOverride(.theme)
    self.theme = theme
    persistTheme(theme)
  }

  func setFontSize(_ fontSize: AppFontSize) {
    recordStartupHydrationOverride(.fontSize)
    self.fontSize = fontSize
    persistFontSize(fontSize)
    FawxTypography.setScale(fontSize.scale)
  }

  func setAccentColor(_ accentColor: AppAccentColor) {
    recordStartupHydrationOverride(.accentColor)
    self.accentColor = accentColor
    FawxAccentPalette.update(accentColor)
    persistAccentColor(accentColor)
  }

  func toggleFavoriteModel(_ modelID: String) {
    let normalizedModelID = modelID.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedModelID.isEmpty else {
      return
    }

    recordStartupHydrationOverride(.modelFavorites)
    if favoriteModelIDs.contains(normalizedModelID) {
      favoriteModelIDs.remove(normalizedModelID)
    } else {
      favoriteModelIDs.insert(normalizedModelID)
    }
    persistFavoriteModelIDs(favoriteModelIDs)
  }

  func awaitPersistedStateLoad() async {
    let loadTask = initialPersistenceLoadTask
    await loadTask?.value
  }

  private func adoptAndConnect(
    serverURL: String,
    bearerToken: String? = nil,
    progress: @escaping @MainActor @Sendable (String) -> Void = { _ in }
  ) async throws {
    progress("Connecting this Mac to Fawx...")
    guard let canonicalURLString = canonicalizeServerURL(serverURL),
      let url = URL(string: canonicalURLString)
    else {
      throw APIError.invalidURL(serverURL)
    }

    let setupClient = FawxClient(baseURL: url, bearerToken: bearerToken)
    let requestedDeviceName = Self.defaultLocalDeviceName()
    let response = try await setupClient.adoptLocalDevice(deviceName: requestedDeviceName)
    let pairedDeviceName = response.deviceName?.trimmingCharacters(in: .whitespacesAndNewlines)
    let resolvedDeviceName = pairedDeviceName?.nonEmpty ?? requestedDeviceName

    try await savePairing(
      serverURLString: canonicalURLString,
      token: response.token,
      deviceName: resolvedDeviceName,
      connectionMode: .local
    )
    isSetupComplete = true
    await persistence.setSetupComplete(true)
    progress("Opening Fawx...")
    await bootstrap()
  }

  private func shouldNotifyForRipcordActivation(
    from previousStatus: RipcordStatusResponse?,
    to currentStatus: RipcordStatusResponse
  ) -> Bool {
    guard currentStatus.shouldSurfaceNotification else {
      return false
    }

    guard let previousStatus, previousStatus.shouldSurfaceNotification else {
      return true
    }

    return previousStatus.notificationID != currentStatus.notificationID
  }

  private func postRipcordNotification(for status: RipcordStatusResponse) {
    let title = "Fawx - Tripwire Crossed"
    let subtitle = "\"\(status.displayDescription)\""
    let body = "Actions are being journaled. Review when ready."

    Task { @MainActor [weak self] in
      guard let self else {
        return
      }

      let center = UNUserNotificationCenter.current()
      if !hasRequestedNotificationAuthorization {
        hasRequestedNotificationAuthorization = true
        _ = try? await center.requestAuthorization(options: [.alert, .sound])
      }

      let content = UNMutableNotificationContent()
      content.title = title
      content.subtitle = subtitle
      content.body = body
      content.sound = .default

      let identifier = "ripcord-\(status.tripwireId ?? UUID().uuidString)"
      let request = UNNotificationRequest(identifier: identifier, content: content, trigger: nil)
      try? await center.add(request)
    }
  }

  private func clearRipcordState() {
    ripcordStatus = nil
  }

  private func startPersistedStateLoad(resetState: Bool) {
    let persistence = persistence

    initialPersistenceLoadTask = Task(priority: .utility) { [weak self] in
      let snapshot = await persistence.loadLaunchSnapshot(resetState: resetState)
      guard !Task.isCancelled else {
        return
      }

      await self?.finishPersistedStateLoad(snapshot)
    }
  }

  private func finishPersistedStateLoad(_ snapshot: AppStatePersistence.LaunchSnapshot) async {
    await applyPersistedLaunchSnapshot(snapshot)
    initialPersistenceLoadTask = nil
  }

  private func applyPersistedLaunchSnapshot(_ snapshot: AppStatePersistence.LaunchSnapshot) async {
    var launchState = Self.resolvedLaunchState(from: snapshot)
    let startupOverrides = startupHydrationOverrides
    startupHydrationOverrides = []

    if startupOverrides.contains(.navigation) {
      launchState.connectionMode = connectionMode
      launchState.rootDestination = rootDestination
    }

    if startupOverrides.contains(.theme) {
      launchState.theme = theme
    }

    if startupOverrides.contains(.fontSize) {
      launchState.fontSize = fontSize
    }

    if startupOverrides.contains(.accentColor) {
      launchState.accentColor = accentColor
    }

    if startupOverrides.contains(.modelFavorites) {
      launchState.favoriteModelIDs = favoriteModelIDs
    }

    applyLaunchState(launchState)
    await client.updateConfiguration(
      baseURL: launchState.baseURL, bearerToken: launchState.authToken)
    await persistence.persistResolvedLaunchState(
      setupComplete: launchState.isSetupComplete,
      connectionMode: launchState.connectionMode
    )
  }

  private func recordStartupHydrationOverride(_ override: StartupHydrationOverride) {
    guard initialPersistenceLoadTask != nil else {
      return
    }

    startupHydrationOverrides.insert(override)
  }

  private func applyLaunchState(_ launchState: LaunchState) {
    connectionMode = launchState.connectionMode
    rootDestination = launchState.rootDestination
    serverURLString = launchState.serverURLString
    pairedDeviceName = launchState.pairedDeviceName
    theme = launchState.theme
    fontSize = launchState.fontSize
    accentColor = launchState.accentColor
    FawxAccentPalette.update(launchState.accentColor)
    favoriteModelIDs = launchState.favoriteModelIDs
    authToken = launchState.authToken
    localInstallConfiguration = launchState.localInstallConfiguration
    isSetupComplete = launchState.isSetupComplete
  }

  private var rootDestinationKey: String {
    switch rootDestination {
    case .main:
      "main"
    case .setupWizard:
      "setup"
    case .remoteOnboarding:
      "remote"
    }
  }

  private func resolvePermissionPreset(from config: JSONValue?) -> String {
    let rawPreset = config?
      .value(at: ["permissions", "preset"])?
      .stringValue

    return permissionPresetLabel(rawPreset)
  }

  private func waitForLocalServerReconnect() async {
    for _ in 0..<12 {
      if Task.isCancelled {
        return
      }

      do {
        _ = try await client.health()
        return
      } catch {
        try? await Task.sleep(for: .milliseconds(500))
      }
    }
  }

  private func attemptConnection(initialStatus: ConnectionStatus, allowReconnect: Bool) async {
    connectionStatus = initialStatus

    do {
      lastHealth = try await client.health()
      try await refreshServerState()
      handleConnectionRecovered(showsToast: initialStatus != .connecting)
    } catch {
      await handleConnectionFailure(error, allowReconnect: allowReconnect)
    }
  }

  private func handleConnectionRecovered(showsToast: Bool) {
    reconnectTask?.cancel()
    reconnectTask = nil
    reconnectAttempt = 0
    connectionStatus = .connected
    connectionError = nil

    if showsToast {
      showToast(message: "Connection restored", style: .success)
    }
  }

  private func handleConnectionFailure(_ error: Error, allowReconnect: Bool) async {
    connectionError = connectionMessage(for: error)
    clearRipcordState()

    switch ConnectionStateMachine.failureStatus(for: error, allowReconnect: allowReconnect) {
    case .reconnecting:
      connectionStatus = .reconnecting
      scheduleReconnectIfNeeded()
    case .disconnected, .connecting, .connected:
      reconnectTask?.cancel()
      reconnectTask = nil
      connectionStatus = .disconnected
    }
  }

  private func scheduleReconnectIfNeeded() {
    guard reconnectTask == nil, isConfigured else {
      return
    }

    reconnectTask = Task { @MainActor [weak self] in
      guard let self else {
        return
      }

      while !Task.isCancelled && self.isConfigured {
        let delaySeconds = min(pow(2, Double(self.reconnectAttempt)), 30)
        do {
          try await Task.sleep(for: .seconds(delaySeconds))
        } catch is CancellationError {
          return
        } catch {
          return
        }

        guard !Task.isCancelled else {
          return
        }

        do {
          self.lastHealth = try await self.client.health()
          guard !Task.isCancelled else {
            return
          }
          try await self.refreshServerState()
          guard !Task.isCancelled else {
            return
          }
          self.handleConnectionRecovered(showsToast: true)
          return
        } catch is CancellationError {
          return
        } catch {
          guard !Task.isCancelled else {
            return
          }
          self.reconnectAttempt = min(self.reconnectAttempt + 1, 5)
          self.connectionError = self.connectionMessage(for: error)
          let nextStatus = ConnectionStateMachine.retryFailureStatus(
            for: error,
            reconnectAttempt: self.reconnectAttempt
          )
          self.connectionStatus = nextStatus

          if nextStatus == .disconnected {
            self.reconnectTask = nil
            return
          }
        }
      }
    }
  }

  private func isAuthenticationFailure(_ error: Error) -> Bool {
    ConnectionStateMachine.issueKind(for: error) == .authentication
  }

  private func isConnectivityFailure(_ error: Error) -> Bool {
    ConnectionStateMachine.issueKind(for: error) == .connectivity
  }

  private func connectionMessage(for error: Error) -> String {
    let destination = serverURLString.isEmpty ? "your Fawx server" : serverURLString

    if isAuthenticationFailure(error) {
      return "Authentication failed. Check your pairing in Settings."
    }

    if isConnectivityFailure(error) {
      return "Fawx server at \(destination) is offline or unreachable."
    }

    return "Fawx server at \(destination) returned an unexpected response."
  }

  func showToast(message: String, style: AppToastStyle) {
    toastDismissTask?.cancel()
    toast = AppToast(message: message, style: style)

    toastDismissTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .seconds(3))
      self?.toast = nil
    }
  }

  private func refreshLocalInstallConfiguration() async -> LocalInstallConfiguration? {
    #if os(macOS)
      let configuration = await persistence.loadLocalInstallConfiguration()
      localInstallConfiguration = configuration
      return configuration
    #else
      return nil
    #endif
  }

  func synchronizeLocalConnectionIfNeeded() async {
    #if os(macOS)
      guard connectionMode == .local else {
        return
      }

      let configuration: LocalInstallConfiguration?
      if let localInstallConfiguration {
        configuration = localInstallConfiguration
      } else {
        configuration = await refreshLocalInstallConfiguration()
      }
      guard let configuration else {
        debugLogLocalConnectionSync("no local install configuration available")
        return
      }

      let resolvedServerURL = configuration.baseURLString
      let resolvedBearerToken = configuration.bearerToken

      guard serverURLString != resolvedServerURL || authToken != resolvedBearerToken else {
        debugLogLocalConnectionSync("already synchronized to \(resolvedServerURL)")
        return
      }

      debugLogLocalConnectionSync(
        "resyncing local connection from \(serverURLString.nonEmpty ?? "<empty>") to \(resolvedServerURL)"
      )
      serverURLString = resolvedServerURL
      authToken = resolvedBearerToken
      await client.updateConfiguration(
        baseURL: URL(string: resolvedServerURL),
        bearerToken: resolvedBearerToken
      )
    #endif
  }

  #if DEBUG
    private func debugLogServerURLMutation(from oldValue: String, to newValue: String) {
      guard oldValue != newValue || newValue.contains(":18400") else {
        return
      }

      NSLog(
        """
        [FawxDebug][AppState] serverURLString changed old=%@ new=%@ mode=%@ root=%@ stack=%@
        """,
        oldValue.nonEmpty ?? "<empty>",
        newValue.nonEmpty ?? "<empty>",
        connectionMode.rawValue,
        rootDestinationKey,
        Thread.callStackSymbols.prefix(8).joined(separator: " | ")
      )
    }

    private func debugLogLocalConnectionSync(_ message: String) {
      NSLog("[FawxDebug][AppState] local connection sync: %@", message)
    }
  #else
    private func debugLogServerURLMutation(from oldValue: String, to newValue: String) {}

    private func debugLogLocalConnectionSync(_ message: String) {}
  #endif

  private func persistConnectionMode() {
    let connectionMode = connectionMode
    let persistence = persistence
    Task(priority: .utility) {
      await persistence.setConnectionMode(connectionMode)
    }
  }

  private func repairLaunchAgentIfNeeded() async {
    #if os(macOS)
      guard !hasAttemptedLaunchAgentRepair else {
        return
      }
      guard connectionMode == .local else {
        return
      }
      guard launchAgentStatus?.installed == true else {
        return
      }
      guard let plistURL = Self.launchAgentPlistURL() else {
        return
      }
      guard
        let plistContents = try? String(contentsOf: plistURL, encoding: .utf8),
        Self.launchAgentNeedsRepair(plistContents: plistContents)
      else {
        return
      }

      hasAttemptedLaunchAgentRepair = true

      do {
        _ = try await client.installLaunchAgent(autoStart: true)
        launchAgentStatus = try await client.launchAgentStatus()
        showToast(message: "Repaired local server auto-start configuration.", style: .info)
      } catch {
        setupActionError = error.localizedDescription
      }
    #endif
  }

  nonisolated static func launchAgentNeedsRepair(plistContents: String) -> Bool {
    guard plistContents.contains("<string>serve</string>") else {
      return false
    }

    return !plistContents.contains("<string>--http</string>")
  }

  private nonisolated static func launchAgentPlistURL() -> URL? {
    #if os(macOS)
      FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library", isDirectory: true)
        .appendingPathComponent("LaunchAgents", isDirectory: true)
        .appendingPathComponent("ai.fawx.server.plist", isDirectory: false)
    #else
      nil
    #endif
  }

  private func persistTheme(_ theme: AppTheme) {
    let persistence = persistence
    Task(priority: .utility) {
      await persistence.setTheme(theme)
    }
  }

  private func persistFontSize(_ fontSize: AppFontSize) {
    let persistence = persistence
    Task(priority: .utility) {
      await persistence.setFontSize(fontSize)
    }
  }

  private func persistAccentColor(_ accentColor: AppAccentColor) {
    let persistence = persistence
    Task(priority: .utility) {
      await persistence.setAccentColor(accentColor)
    }
  }

  private func persistFavoriteModelIDs(_ modelIDs: Set<String>) {
    let persistence = persistence
    Task(priority: .utility) {
      await persistence.setFavoriteModelIDs(modelIDs)
    }
  }

  private static func initialLaunchState() -> LaunchState {
    let serverURLString = UITestLaunchOptions.serverURLOverride ?? ""
    let authToken = UITestLaunchOptions.bearerTokenOverride
    let connectionMode = defaultConnectionMode(forStoredServerURL: serverURLString)

    return LaunchState(
      connectionMode: connectionMode,
      rootDestination: resolveInitialDestination(
        isConfigured: !serverURLString.isEmpty && authToken?.isEmpty == false,
        setupComplete: false,
        connectionMode: connectionMode,
        hasStoredServerURL: !serverURLString.isEmpty,
        hasLocalInstall: false
      ),
      serverURLString: serverURLString,
      pairedDeviceName: resolvedDeviceName(storedDeviceName: nil, authToken: authToken),
      theme: .system,
      fontSize: .medium,
      accentColor: .default,
      favoriteModelIDs: [],
      authToken: authToken,
      localInstallConfiguration: nil,
      isSetupComplete: false
    )
  }

  private static func resolvedLaunchState(
    from snapshot: AppStatePersistence.LaunchSnapshot
  ) -> LaunchState {
    LaunchState.resolved(from: snapshot)
  }

  private static func resolvedDeviceName(
    storedDeviceName: String?,
    authToken: String?
  ) -> String? {
    UITestLaunchOptions.pairedDeviceNameOverride
      ?? storedDeviceName
      ?? (UITestLaunchOptions.isUITesting && authToken != nil ? "UI Test Device" : nil)
  }

  static func resolveInitialDestination(
    isConfigured: Bool,
    setupComplete: Bool,
    connectionMode: AppConnectionMode,
    hasStoredServerURL: Bool,
    hasLocalInstall: Bool
  ) -> AppRootDestination {
    #if os(iOS)
      return isConfigured ? .main : .remoteOnboarding
    #else
      if isConfigured && (setupComplete || hasLocalInstall || connectionMode == .remote) {
        return .main
      }
      if hasLocalInstall {
        return .main
      }
      if connectionMode == .remote || hasStoredServerURL {
        return .remoteOnboarding
      }
      return .setupWizard
    #endif
  }

  static func defaultConnectionMode(forStoredServerURL storedServerURL: String) -> AppConnectionMode
  {
    #if os(iOS)
      .remote
    #else
      storedServerURL.isEmpty ? .local : .remote
    #endif
  }

  static func resolveLaunchConnectionMode(
    storedConnectionModeRawValue: String?,
    storedServerURL: String,
    effectiveServerURLString: String
  ) -> AppConnectionMode {
    let defaultConnectionMode = defaultConnectionMode(
      forStoredServerURL: effectiveServerURLString.isEmpty
        ? storedServerURL : effectiveServerURLString
    )

    return AppConnectionMode(
      rawValue: storedConnectionModeRawValue ?? defaultConnectionMode.rawValue
    ) ?? defaultConnectionMode
  }

  private static func defaultLocalDeviceName() -> String {
    #if os(macOS)
      if let localizedName = Host.current().localizedName, !localizedName.isEmpty {
        return localizedName
      }
    #endif
    return "This Mac"
  }
}
