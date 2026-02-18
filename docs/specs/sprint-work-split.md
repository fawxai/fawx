# Sprint Work Split — Clawdio + Jarvis

*Created 2026-02-16. Update as tasks move.*

## Philosophy

Two agents, two tracks, minimal merge conflicts. Clawdio owns architecture and cross-cutting infrastructure. Jarvis owns UI features and isolated user-facing work. Both branch from `feat/android-mvp`, both follow the git flow in AGENTS.md.

---

## Track 1: Clawdio (Architecture)

### Now
| Task | Phase | Issues | Status |
|------|-------|--------|--------|
| Tier 2.5: Streaming for chat path | Phase 5 | #494 | ⬜ |
| Tier 2.5: Deduplicate OpenRouter/OpenAI clients | Phase 5 | #495 | ⬜ |

### Next
| Task | Phase | Issues | Status |
|------|-------|--------|--------|
| Tier 3: Key/Auth Management audit | Phase 6 | — | ⬜ |
| transformContext hook | Phase 7 | #483 | ⬜ |
| Token usage tracking | Phase 7 | #490 | ⬜ |

### Clawdio Owns (do not modify without coordinating)
- `AgentExecutor.kt` — execution loop, boundary checks
- `BoundaryCheck.kt` — check framework, CheckResult, SteerCheck
- `PhoneAgentApi.kt` — API client, tool execution, model tier gating
- `PhoneAgentLocal.kt` — local LLM client
- `WebSearchClient.kt`, `WebFetchClient.kt` — API tools
- `OutputClassifier.kt` — output visibility classification
- `ModelClassifier.kt`, `ModelConfig.kt` — model tier infrastructure
- `StuckDetector.kt` — stuck detection logic
- `Message.kt` — message data class, contentBlocks, serialization
- `ToolUse.kt`, `ChatResponse.kt` — API data models
- All files in `core/src/test/`

---

## Track 2: Jarvis (UI Features)

### Now
| Task | Phase | Issues | Status |
|------|-------|--------|--------|
| Steer UI | Phase 4 | — | ⬜ |

### Next
| Task | Phase | Issues | Status |
|------|-------|--------|--------|
| On-Device Memory (SQLite + keyword search) | Phase 7 | — | ⬜ |
| Progressive Status Updates (tool name surfacing) | Phase 7 | — | ⬜ |
| Conversation Lifecycle (idle timeout, daily reset) | Phase 7 | — | ⬜ |

### Jarvis Owns (do not modify without coordinating)
- `ChatActivity.kt` — main UI, Compose screens
- `OverlayController.kt` — overlay bubble, mini-view
- `OverlayService.kt` — overlay service lifecycle
- Overlay layout files and composables
- `SettingsActivity.kt` — settings screens
- `res/` — layouts, strings, drawables, themes

---

## Shared Files (coordinate before editing)

These files are touched by both tracks. Whoever needs to edit them should check with the other first (or at minimum, check `git log` for recent changes).

| File | Why shared |
|------|------------|
| `ChatViewModel.kt` | Clawdio: executor wiring, agent state. Jarvis: UI state, steer queue, input handling |
| `PhoneTools.kt` | Clawdio: tool definitions, gating. Jarvis: tool display names, categories |
| `build.gradle.kts` (both modules) | Dependencies |
| `AndroidManifest.xml` | Permissions, services |

### Conflict Resolution
1. If both agents have PRs touching the same file, the one closer to merge goes first
2. The other rebases after merge
3. When in doubt, ask Joe

---

## Branch Naming
- Clawdio: `refactor/*` or `fix/*` (architecture work)
- Jarvis: `feat/*` or `ui/*` (feature work)
- Both: branch from `feat/android-mvp`, PR back to `feat/android-mvp`

## Review
- All PRs get `@claude review this PR` per AGENTS.md
- Cross-track PRs that touch shared files: tag the other agent for awareness
- Joe is final merge authority

---

## Sync Points
When a Telegram group is set up, use it for:
- "I\'m about to edit ChatViewModel.kt" heads-up
- PR ready notifications
- Design questions that cross track boundaries
- Merge conflict coordination

Until then: Joe relays, or check `git log --oneline -10 feat/android-mvp` before branching.
