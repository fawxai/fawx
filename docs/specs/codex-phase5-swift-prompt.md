# Codex Phase 5 Swift Implementation Prompt

**Target:** macOS + iOS Swift app in `app/`
**Visual reference:** `docs/design/cowork-mockups-p4p5.html` + `docs/design/screenshots/s*.png`
**Phase 5 spec:** `docs/specs/phase5-full-parity.md`
**Existing app:** `app/Fawx/` — Phase 1-3 built (chat, sessions, settings, skills, auth)

---

## What's New

Phase 5 backend endpoints are now live. You need to wire the Swift app to use them.

---

## New Files to Create

### Models
- `app/Fawx/Models/Permission.swift` — permission types
- `app/Fawx/Models/Synthesis.swift` — synthesis/custom instructions types
- `app/Fawx/Models/MarketplaceSkill.swift` — marketplace skill types
- `app/Fawx/Models/Usage.swift` — usage/cost types
- `app/Fawx/Models/OAuthFlow.swift` — OAuth flow types

### Views
- `app/Fawx/Views/Shared/PermissionsSettingsPanel.swift` — permissions & safety screen
- `app/Fawx/Views/Shared/SynthesisSettingsPanel.swift` — custom instructions editor
- `app/Fawx/Views/Shared/UsageSettingsPanel.swift` — cost summary panel
- `app/Fawx/Views/Shared/MarketplaceView.swift` — marketplace tab in Skills screen

### ViewModels
- `app/Fawx/ViewModels/PermissionsViewModel.swift` — permissions state
- `app/Fawx/ViewModels/SynthesisViewModel.swift` — synthesis state
- `app/Fawx/ViewModels/UsageViewModel.swift` — usage state

---

## Files to Modify

- `app/Fawx/Networking/FawxClient.swift` — add methods for all new endpoints
- `app/Fawx/ViewModels/SettingsViewModel.swift` — integrate new panels
- `app/Fawx/ViewModels/SkillsViewModel.swift` — add marketplace search/install/remove
- `app/Fawx/Views/macOS/SettingsView.swift` — add permissions, synthesis, usage sections
- `app/Fawx/Views/iOS/iOSSettingsView.swift` — add permissions, synthesis, usage sections
- `app/Fawx/Views/Shared/SkillsView.swift` — add marketplace tab/section
- `app/Fawx/Views/Shared/AuthStatusList.swift` — add "Sign in with ChatGPT" OAuth button

---

## 1. Permissions & Safety

### Model (`Permission.swift`)
```swift
struct PermissionEntry: Codable, Sendable, Hashable, Identifiable {
    let action: String
    let level: String    // "allow" | "propose" | "deny"
    let title: String
    var id: String { action }
}

struct PermissionsResponse: Codable, Sendable, Hashable {
    let preset: String
    let permissions: [PermissionEntry]
    let availablePresets: [String]
    
    enum CodingKeys: String, CodingKey {
        case preset, permissions
        case availablePresets = "available_presets"
    }
}

struct PermissionsPatchRequest: Encodable, Sendable {
    let preset: String?
    let changes: [PermissionChange]?
}

struct PermissionChange: Codable, Sendable {
    let action: String
    let level: String
}

struct PermissionsPatchResponse: Codable, Sendable {
    let updated: Bool
    let preset: String
    let changedActions: [String]
    
    enum CodingKeys: String, CodingKey {
        case updated, preset
        case changedActions = "changed_actions"
    }
}
```

### FawxClient Methods
```swift
func getPermissions() async throws -> PermissionsResponse {
    try await performRequest(path: "/v1/permissions", decodeAs: PermissionsResponse.self)
}

func patchPermissions(_ request: PermissionsPatchRequest) async throws -> PermissionsPatchResponse {
    let body = try encoder.encode(request)
    return try await performRequest(
        path: "/v1/permissions",
        method: "PATCH",
        bodyData: body,
        decodeAs: PermissionsPatchResponse.self
    )
}
```

### PermissionsViewModel
```swift
@MainActor @Observable
final class PermissionsViewModel {
    var permissions: [PermissionEntry] = []
    var activePreset: String = "power"
    var availablePresets: [String] = []
    var isLoading = false
    var errorMessage: String?
    
    private let appState: AppState
    
    init(appState: AppState) { self.appState = appState }
    
    func refresh() async { ... }  // GET /v1/permissions
    func applyPreset(_ name: String) async { ... }  // PATCH with preset
    func setActionLevel(action: String, level: String) async { ... }  // PATCH with changes
}
```

### PermissionsSettingsPanel
- Section header: "Permissions & Safety"
- Preset picker: segmented control or dropdown with available presets
- Per-action list: each row shows title + level picker (Allow / Ask / Deny)
- Changing any individual action switches preset to "custom" automatically
- Color coding: green=allow, amber=propose, red=deny

---

## 2. Synthesis / Custom Instructions

### Model (`Synthesis.swift`)
```swift
struct SynthesisResponse: Codable, Sendable, Hashable {
    let synthesis: String?
    let updatedAt: Int?
    let source: String
    let version: Int
    let maxLength: Int
    
    enum CodingKeys: String, CodingKey {
        case synthesis, source, version
        case updatedAt = "updated_at"
        case maxLength = "max_length"
    }
}

struct SetSynthesisRequest: Encodable, Sendable {
    let synthesis: String
    let version: Int?
}

struct SetSynthesisResponse: Codable, Sendable {
    let updated: Bool
    let synthesis: String
    let updatedAt: Int
    let version: Int
    
    enum CodingKeys: String, CodingKey {
        case updated, synthesis, version
        case updatedAt = "updated_at"
    }
}

struct ClearSynthesisResponse: Codable, Sendable {
    let cleared: Bool
    let version: Int
}
```

### FawxClient Methods
```swift
func getSynthesis() async throws -> SynthesisResponse {
    try await performRequest(path: "/v1/synthesis", decodeAs: SynthesisResponse.self)
}

func setSynthesis(_ text: String, version: Int? = nil) async throws -> SetSynthesisResponse {
    let body = try encoder.encode(SetSynthesisRequest(synthesis: text, version: version))
    return try await performRequest(path: "/v1/synthesis", method: "PUT", bodyData: body, decodeAs: SetSynthesisResponse.self)
}

func clearSynthesis() async throws -> ClearSynthesisResponse {
    try await performRequest(path: "/v1/synthesis", method: "DELETE", decodeAs: ClearSynthesisResponse.self)
}
```

### SynthesisSettingsPanel
- Section header: "Custom Instructions"
- Helper text: "Set persistent guidance for how Fawx should behave. You can also change this by asking Fawx directly in chat."
- TextEditor field with character counter showing `currentLength / maxLength`
- Save button (disabled when unchanged or over limit)
- Clear button with confirmation
- Show 409 Conflict gracefully: "Instructions were updated elsewhere. Refreshing..."

---

## 3. Usage / Cost Tracking

### Model (`Usage.swift`)
```swift
struct UsageResponse: Codable, Sendable, Hashable {
    let session: SessionUsage
    let today: PeriodUsage
    let providers: [ProviderUsage]
}

struct SessionUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double
    
    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}

struct PeriodUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double
    
    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}

struct ProviderUsage: Codable, Sendable, Hashable {
    let provider: String
    let model: String
    let inputTokens: Int
    let outputTokens: Int
    let estimatedCostUsd: Double
    
    enum CodingKeys: String, CodingKey {
        case provider, model
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}
```

### FawxClient Method
```swift
func getUsage() async throws -> UsageResponse {
    try await performRequest(path: "/v1/usage", decodeAs: UsageResponse.self)
}
```

### UsageSettingsPanel
- Section header: "Usage"
- Show: Session tokens, Today's estimated cost, per-provider breakdown
- Format costs as "$X.XX" — use NumberFormatter with .currency style
- If all zeros (endpoint returns stubs), show "Usage tracking not yet available"

---

## 4. Skills Marketplace

### Model (`MarketplaceSkill.swift`)
```swift
struct MarketplaceSkillSummary: Codable, Sendable, Hashable, Identifiable {
    let name: String
    let title: String
    let description: String
    let publisher: String
    let signed: Bool
    var id: String { name }
}

struct SkillSearchResponse: Codable, Sendable, Hashable {
    let query: String
    let skills: [MarketplaceSkillSummary]
    let total: Int
    let marketplaceAvailable: Bool
    let message: String
    
    enum CodingKeys: String, CodingKey {
        case query, skills, total, message
        case marketplaceAvailable = "marketplace_available"
    }
}
```

### FawxClient Methods
```swift
func searchSkills(query: String) async throws -> SkillSearchResponse {
    let encoded = query.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? query
    return try await performRequest(path: "/v1/skills/search?q=\(encoded)", decodeAs: SkillSearchResponse.self)
}

func installSkill(name: String) async throws {
    struct InstallRequest: Encodable { let name: String }
    let body = try encoder.encode(InstallRequest(name: name))
    // This returns 503 (stub) — handle gracefully
    let _: JSONValue = try await performRequest(path: "/v1/skills/install", method: "POST", bodyData: body, decodeAs: JSONValue.self)
}

func removeSkill(name: String) async throws {
    let _: JSONValue = try await performRequest(path: "/v1/skills/\(name)", method: "DELETE", decodeAs: JSONValue.self)
}
```

### SkillsView Update
Add a "Marketplace" section/tab alongside the existing "Installed Skills":
- Search bar that queries `/v1/skills/search`
- When `marketplaceAvailable` is false, show: "Marketplace not yet connected"
- Install button on each marketplace result (will show 503 error gracefully)
- Delete/remove on installed skills

---

## 5. OAuth — "Sign in with ChatGPT"

### Model (`OAuthFlow.swift`)
```swift
struct OAuthStartResponse: Codable, Sendable {
    let provider: String
    let authorizeUrl: String
    let flowToken: String
    let redirectUri: String
    
    enum CodingKeys: String, CodingKey {
        case provider
        case authorizeUrl = "authorize_url"
        case flowToken = "flow_token"
        case redirectUri = "redirect_uri"
    }
}

struct OAuthCallbackRequest: Encodable, Sendable {
    let code: String
    let flowToken: String
    
    enum CodingKeys: String, CodingKey {
        case code
        case flowToken = "flow_token"
    }
}

struct OAuthCallbackResponse: Codable, Sendable {
    let provider: String
    let status: String
    let authMethod: String
    let verified: Bool
    
    enum CodingKeys: String, CodingKey {
        case provider, status, verified
        case authMethod = "auth_method"
    }
}
```

### FawxClient Methods
```swift
func oauthStart(provider: String) async throws -> OAuthStartResponse {
    try await performRequest(path: "/v1/auth/\(provider)/oauth-start", decodeAs: OAuthStartResponse.self)
}

func oauthCallback(provider: String, code: String, flowToken: String) async throws -> OAuthCallbackResponse {
    let body = try encoder.encode(OAuthCallbackRequest(code: code, flowToken: flowToken))
    return try await performRequest(
        path: "/v1/auth/\(provider)/oauth-callback",
        method: "POST",
        bodyData: body,
        decodeAs: OAuthCallbackResponse.self
    )
}
```

### AuthStatusList Update
Add a "Sign in with ChatGPT" button alongside existing auth methods:
1. Button taps → call `oauthStart(provider: "openai")`
2. Open `authorizeUrl` in `ASWebAuthenticationSession` with callback scheme `fawx-auth`
3. Extract `code` from callback URL query parameter
4. Call `oauthCallback(provider: "openai", code: code, flowToken: flowToken)`
5. On success → refresh auth status list
6. On failure → show error toast

**Important:** Use `ASWebAuthenticationSession` (import `AuthenticationServices`), NOT a WKWebView or Safari open. The callback scheme is `fawx-auth` and the callback URL will be `fawx-auth://openai/callback?code=...&state=...`.

```swift
import AuthenticationServices

func startOAuthLogin(provider: String) async {
    do {
        let startResponse = try await appState.client.oauthStart(provider: provider)
        guard let url = URL(string: startResponse.authorizeUrl) else { return }
        
        let code = try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(
                url: url,
                callbackURLScheme: "fawx-auth"
            ) { callbackURL, error in
                if let error { continuation.resume(throwing: error); return }
                guard let callbackURL,
                      let components = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false),
                      let code = components.queryItems?.first(where: { $0.name == "code" })?.value
                else {
                    continuation.resume(throwing: APIError.unknown("Missing authorization code"))
                    return
                }
                continuation.resume(returning: code)
            }
            session.prefersEphemeralWebBrowserSession = true
            session.presentationContextProvider = ... // see below
            session.start()
        }
        
        let _ = try await appState.client.oauthCallback(
            provider: provider,
            code: code,
            flowToken: startResponse.flowToken
        )
        // Refresh auth status
        await appState.refreshAuthProviders()
    } catch {
        // Show error
    }
}
```

For `presentationContextProvider`, create a simple NSObject conforming to `ASWebAuthenticationPresentationContextProviding` that returns the key window.

---

## 6. Settings View Wiring

### macOS SettingsView — add new sections after existing ones:
```swift
var body: some View {
    ScrollView {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
            connectionSection
            modelThinkingSection
            authStatusSection      // ← add OAuth button here
            permissionsSection     // ← NEW
            synthesisSection       // ← NEW
            usageSection           // ← NEW
            appearanceSection
        }
        ...
    }
}

private var permissionsSection: some View {
    PermissionsSettingsPanel(viewModel: permissionsViewModel)
}

private var synthesisSection: some View {
    SynthesisSettingsPanel(viewModel: synthesisViewModel)
}

private var usageSection: some View {
    UsageSettingsPanel(viewModel: usageViewModel)
}
```

### iOS iOSSettingsView — same sections in the iOS layout

---

## Design Rules

1. Follow existing app patterns exactly — `@Observable`, `@Bindable`, `@MainActor`, `FawxSpacing`, `Color.fawx*`
2. All new types are `Codable, Sendable, Hashable`
3. Use `CodingKeys` for snake_case → camelCase mapping
4. Error handling: show errors in toast or inline, never crash
5. Loading states: show progress indicator during async calls
6. Accessibility: all interactive elements have accessibility labels
7. Dark mode: use `Color.fawx*` theme colors (already defined in `Theme/Colors.swift`)
8. Both macOS and iOS: use `Views/Shared/` for cross-platform, platform-specific in `Views/macOS/` or `Views/iOS/`

---

## Build

Build for macOS:
```bash
cd app
xcodebuild -scheme Fawx -destination 'platform=macOS' build
```

Verify no warnings. Fix any compiler errors before committing.
