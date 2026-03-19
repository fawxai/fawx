# GitHub PAT Settings UI — Spec

Date: 2026-03-18
Status: Implementation-ready
Priority: Ship blocker

## Problem

There is no GUI path to set a GitHub Personal Access Token. The git_push
and github_pr_create tools require a PAT, but the only way to set one is
via TUI (`/auth github set-token`) or `fawx setup` (which has UX issues).
Users who only interact through the Swift GUI cannot configure GitHub auth.

## Solution

Add a GitHub PAT input section to `AuthStatusList.swift` with a SecureField
and Save button that calls the existing `FawxClient.storeAPIKey()` method.

## Existing infrastructure (no backend changes needed)

- **API endpoint**: `POST /v1/auth/github/api-key` with body `{"api_key": "ghp_..."}`
- **Client method**: `FawxClient.storeAPIKey(provider:apiKey:label:)` (line 140)
- **Auth store**: saves as `AuthMethod::ApiKey { provider: "github", key: "..." }`
- **Credential bridge**: `borrow_github_token()` falls back to AuthManager, which
  reads this stored key (fixed in commit `dbf53f18`)

## Implementation

### 1. Add state to `AuthStatusList.swift`

```swift
@State private var githubTokenInput = ""
@State private var isSavingGitHub = false
```

### 2. Add GitHub section to the body

After the existing `ForEach(appState.authProviders)` block, add:

```swift
// GitHub PAT (for git push / PR creation)
GitHubTokenSection(
    tokenInput: $githubTokenInput,
    isSaving: $isSavingGitHub,
    isConfigured: appState.authProviders.contains(where: {
        $0.provider.lowercased() == "github"
    }),
    onSave: { token in
        isSavingGitHub = true
        defer { isSavingGitHub = false }
        do {
            _ = try await appState.client.storeAPIKey(
                provider: "github",
                apiKey: token,
                label: "GitHub PAT"
            )
            githubTokenInput = ""
            await appState.refreshSettingsState()
            appState.showToast(message: "GitHub token saved.", style: .success)
        } catch {
            appState.showToast(message: error.localizedDescription, style: .error)
        }
    }
)
```

### 3. Create `GitHubTokenSection` view

Either inline or as a private struct in the same file:

```swift
private struct GitHubTokenSection: View {
    @Binding var tokenInput: String
    @Binding var isSaving: Bool
    let isConfigured: Bool
    let onSave: (String) async -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                Text("GitHub")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: FawxSpacing.paddingMD)

                Text(isConfigured ? "Configured" : "Not configured")
                    .font(FawxTypography.status)
                    .foregroundStyle(isConfigured ? Color.fawxSuccess : Color.fawxWarning)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, 4)
                    .background(
                        (isConfigured ? Color.fawxSuccess : Color.fawxWarning).opacity(0.12)
                    )
                    .clipShape(Capsule())
            }

            Text("Required for git push and pull request creation.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            HStack(spacing: FawxSpacing.paddingSM) {
                SecureField("Personal Access Token", text: $tokenInput)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel("GitHub personal access token")

                Button(isSaving ? "Saving..." : "Save") {
                    let token = tokenInput.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !token.isEmpty else { return }
                    Task { await onSave(token) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(tokenInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isSaving)
                .accessibilityLabel("Save GitHub token")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .accessibilityIdentifier("authProvider_github")
    }
}
```

### 4. iOS support

If `iOSSettingsView.swift` has a separate auth section, add the same
`GitHubTokenSection` there. Check if it reuses `AuthStatusList` — if so,
no extra work needed.

## File changes

| File | Change |
|------|--------|
| `app/Fawx/Views/Shared/AuthStatusList.swift` | Add `@State` vars, `GitHubTokenSection` struct, wire into body |

## No backend changes required

Everything already exists on the Rust side.

## Test plan

1. Open Settings → Auth Status
2. See "GitHub — Not configured" section with SecureField
3. Paste a PAT, click Save
4. Toast: "GitHub token saved."
5. Status changes to "Configured"
6. Ask Fawx to `git_push` — should authenticate successfully
7. Restart server — token persists (stored in AuthManager)

## Estimated scope

~60 lines of Swift. Single file change. No backend work.
