# Fawx Debug Handoff - 2026-03-21

## Current State

- Repo: `fawx/`
- Branch: `dev`
- Rebase status: local `dev` is `0 ahead / 0 behind` `origin/dev`; no rebase needed before resuming.
- Worktree: dirty, with both Swift app and Rust engine changes in progress.
- Current LaunchAgent server process:
  - binary: `target/release/fawx`
  - args: `serve --http --port 8400 --data-dir ~/.fawx`
  - started: `Sat Mar 21 01:52:52 2026`
- Current LaunchAgent plist:
  - `~/Library/LaunchAgents/ai.fawx.server.plist`
  - now correctly includes `--http`
- Current active LaunchAgent log:
  - `~/Library/Logs/Fawx/server.log`
- Older/stale runtime log from earlier manual runs:
  - `~/.fawx/server.log`
  - useful historically, but not the first place to look for current LaunchAgent issues

## What We Learned

### 1. Parallel sessions exposed multiple bugs, but did not create the main engine bug

The strongest current read is:

- parallel sessions increased the frequency of multi-round tool continuation paths
- the core `call_*` vs `fc_*` provider-id bug already lived in the engine
- the new app behavior surfaced it more reliably

This distinction matters because some early Swift-side mitigations helped stability, but the real blocker lived in the Rust tool continuation path.

### 2. There were several independent issues, not one bug

The debugging thread mixed together at least these separate problems:

- shared stream/session state problems in the Swift app when multiple sessions were active
- overly aggressive refresh/polling causing timeout storms
- a false Ripcord popup on fresh sessions
- a stale LaunchAgent missing `--http`, causing headless restart loops every ~10 seconds
- a Rustls startup crash in the server because no crypto provider was installed
- OpenAI Responses tool-call identifier bookkeeping bugs in the engine
- old poisoned session history replaying orphan `tool_use` blocks into new provider requests
- a separate scrolling / blank-on-open issue in the app that is still not root-caused

## Confirmed Root Causes and Fixes

### A. Multi-session Swift state collision

Root cause:

- the app originally tracked only one global live stream, so concurrent session activity could stomp other sessions

Fix direction taken:

- moved live stream state to be session-scoped in `ChatViewModel`
- updated UI plumbing so multiple sessions can show active independently

Key files touched:

- `app/Fawx/ViewModels/ChatViewModel.swift`
- `app/Fawx/Views/macOS/Sidebar.swift`
- `app/Fawx/Views/macOS/ContentView.swift`
- `app/Fawx/Views/iOS/SessionListView.swift`
- `app/Fawx/Views/macOS/FawxMacCommands.swift`
- `app/FawxTests/ViewModels/ChatViewModelTests.swift`

Status:

- implemented and tested

### B. Timeout storms from broad background refresh work

Root cause:

- multiple background refresh paths were still firing while live chat work was happening
- iOS hidden tabs were still polling
- shared server load showed up as many `-1001` timeout errors in the iOS console while using the macOS app

Fix direction taken:

- coalesced refresh work in `AppState`
- skipped broad macOS background refresh cycles while live streams are active
- stopped inactive iOS tabs from continuing background work

Key files touched:

- `app/Fawx/ViewModels/AppState.swift`
- `app/Fawx/FawxApp.swift`
- `app/Fawx/Views/iOS/TabRootView.swift`
- `app/Fawx/Views/Shared/FleetView.swift`
- `app/Fawx/Views/Shared/ExperimentsView.swift`
- `app/Fawx/Views/Shared/SkillsView.swift`
- `app/Fawx/Views/Shared/GitView.swift`
- `app/Fawx/Views/iOS/iOSSettingsView.swift`
- `app/FawxTests/ViewModels/AppStateTests.swift`

Status:

- implemented and tested

### C. Ripcord popup on fresh session

Root cause:

- the app could show the Ripcord bubble even when there were `0 actions journaled`

Fix direction taken:

- only show the Ripcord notification when there are actual journaled actions

Key files touched:

- `app/Fawx/Models/Ripcord.swift`
- `app/Fawx/ViewModels/AppState.swift`

Status:

- implemented

### D. Stale LaunchAgent missing `--http`

Root cause:

- the installed LaunchAgent on this machine was stale and launched:
  - `fawx serve --port 8400 --data-dir ~/.fawx`
- because it was missing `--http`, `launchd` started a headless server that exited and restarted every ~10 seconds
- that produced repeated lock errors such as:
  - `Database already open. Cannot acquire lock.`
  - `fawx serve - headless mode`

What happened operationally:

- user ran `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/ai.fawx.server.plist`
- user then reinstalled the LaunchAgent from Fawx settings
- the LaunchAgent plist is now correct and includes `--http`

App-side support added:

- one-time self-heal for stale LaunchAgent shape in `AppState`

Key files touched:

- `app/Fawx/ViewModels/AppState.swift`
- `app/FawxTests/ViewModels/AppStateTests.swift`

Status:

- fixed on this machine

### E. Server crash loop from missing Rustls crypto provider

Root cause:

- the LaunchAgent server could crash on startup because no process-level Rustls `CryptoProvider` had been selected

Fix direction taken:

- switched `axum-server` usage to the no-provider Rustls feature in `fx-api`
- pinned the provider dependency in `fx-cli`
- installed the Ring provider once at process startup

Key files touched:

- `engine/crates/fx-api/Cargo.toml`
- `engine/crates/fx-cli/Cargo.toml`
- `engine/crates/fx-cli/src/main.rs`

Status:

- implemented, rebuilt, and the LaunchAgent server stayed up afterward

### F. OpenAI Responses tool-call id bookkeeping bug (`call_*` vs `fc_*`)

Root cause:

- provider item ids and tool call ids were being collapsed or lost across streaming / continuation paths
- OpenAI expects tool output items to reference ids that begin with `fc`
- some follow-up tool rounds still sent `call_*` ids, producing errors like:
  - `Invalid 'input[n].id': 'call_...'. Expected an ID that begins with 'fc'.`
  - `No tool output found for function call fc_...`

Fix direction taken:

- split provider item id from tool call id in `fx-llm`
- preserve provider ids through response streaming
- refresh provider ids on later tool rounds in the kernel
- avoid duplicate assistant `tool_use` recording when response content already contains it
- normalize old duplicated stored session history by preferring the `tool_use` that still has a provider id

Key files touched:

- `engine/crates/fx-llm/src/types.rs`
- `engine/crates/fx-llm/src/openai_responses.rs`
- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-cli/src/headless.rs`
- `engine/crates/fx-session/src/session.rs`

Status:

- partially fixed first, then extended with additional kernel/session fixes
- fresh-session tool paths were verified working after these changes

### G. Session-specific poisoned history from orphan `tool_use` blocks

Root cause:

- some old sessions ended with assistant `tool_use` blocks that never received matching `tool_result` messages
- when returning to those sessions later, Fawx replayed that broken history into the next provider request
- OpenAI rejected the request because the replay context still referenced unresolved tool calls

Concrete confirmed case:

- session id: `sess-b72b8ec7`
- title: `We're working on parameter golf. I asked you to log memory in another session. ...`
- confirmed orphan provider ids included:
  - `fc_07ae42746c9561a00169be004558e08197a7ec2527bc6c8b8d`
  - `fc_07ae42746c9561a00169be004558ec81979962b144dc6658c7`
  - `fc_07ae42746c9561a00169be004558f48197896208b4a8278890`

Fix direction taken:

- sanitize replay context at the API layer before it reaches the provider
- unresolved old `tool_use` blocks are now dropped from replay context
- resolved old tool rounds are kept intact

Key files touched:

- `engine/crates/fx-api/src/handlers/sessions.rs`
- `engine/crates/fx-api/src/tests.rs`

Status:

- implemented
- release binary rebuilt
- LaunchAgent server restarted on the rebuilt binary
- I did not send a test message into the real parameter-golf chat to avoid polluting user history

## Important Non-Fixes / Backlog

### 1. Scrolling / blank-session-on-open bug is still not root-caused

Observed behavior:

- opening a session can show a blank screen until the user scrolls
- scrolling during reasoning is still wrong

Important note:

- multiple scroll-related app changes were attempted and did not produce a visible improvement
- this should be treated as a separate backlog item, not mixed into the tool/server debugging thread

Current status:

- unresolved

### 2. Some app-side protections were stabilizers, not root-cause fixes

These are worth keeping conceptually separate from the real engine fixes:

- local URL resync / stale `127.0.0.1:18400` guardrails
- generic `5xx` no longer meaning "server offline"
- some defensive client/config refresh behavior

These were helpful containment steps while debugging, but they were not the final explanation for the main tool-call failures.

## Most Useful Evidence

### Relevant logs

- current LaunchAgent server log:
  - `~/Library/Logs/Fawx/server.log`
- old runtime log from earlier manual runs:
  - `~/.fawx/server.log`

### Useful log commands

```bash
tail -f ~/Library/Logs/Fawx/server.log
```

```bash
tail -f ~/.fawx/server.log
```

### Current LaunchAgent shape

```text
target/release/fawx serve --http --port 8400 --data-dir ~/.fawx
```

### Current server process check

```bash
ps -axo pid,lstart,command | rg 'target/release/fawx serve --http --port 8400 --data-dir ~/.fawx'
```

## Tests and Verification Run During This Debug Pass

Swift-side verification that passed at different points in this debugging thread:

- `xcodebuild test -project app/Fawx.xcodeproj -scheme Fawx-macOS -destination 'platform=macOS' -only-testing:FawxTests-macOS/ChatViewModelTests`
- `xcodebuild test -project app/Fawx.xcodeproj -scheme Fawx-macOS -destination 'platform=macOS' -only-testing:FawxTests-macOS/AppStateTests -only-testing:FawxTests-macOS/ChatViewModelTests`
- `xcodebuild build -project app/Fawx.xcodeproj -scheme Fawx-iOS -destination 'generic/platform=iOS Simulator'`
- `xcodebuild test -scheme Fawx-macOS -only-testing:FawxTests-macOS/ConnectionStateMachineTests -only-testing:FawxTests-macOS/AppStateTests`

Rust-side verification that passed at different points:

- `cargo test -p fx-llm openai_responses -- --nocapture`
- `cargo test -p fx-kernel consume_stream_with_events_assembles_tool_calls_from_deltas -- --nocapture`
- `cargo test -p fx-kernel finalize_stream_tool_calls_separates_multi_tool_arguments -- --nocapture`
- `cargo test -p fx-kernel act_with_tools_refreshes_provider_ids_between_rounds -- --nocapture`
- `cargo test -p fx-kernel consume_stream_with_events_preserves_provider_ids_in_content -- --nocapture`
- `cargo test -p fx-cli session_turn_collector_prefers_content_tool_use_blocks_over_tool_call_fallback -- --nocapture`
- `cargo test -p fx-session session_message_to_llm_message_deduplicates_tool_use_blocks_preferring_provider_id -- --nocapture`
- `cargo test -p fx-api session_messages_to_context_drops_unresolved_tool_use_messages -- --nocapture`
- `cargo test -p fx-api get_session_messages_returns_structured_blocks -- --nocapture`
- `cargo test -p fx-api session_message_ignores_unresolved_prior_tool_use_in_context -- --nocapture`
- `cargo build --release -p fx-cli`

## Resume Plan

When resuming this work later:

1. Do not rebase first. Local `dev` is already aligned with `origin/dev`, and the worktree is dirty.
2. Start with the real session-specific repro that last failed:
   - the parameter-golf chat (`sess-b72b8ec7`)
3. Watch only new lines in:
   - `~/Library/Logs/Fawx/server.log`
4. If the parameter-golf chat still fails:
   - confirm whether the post-restart server log shows a new unresolved tool context problem
   - confirm whether the failing request still includes orphan historical `tool_use` blocks
5. Keep the scrolling / blank-on-open issue separate unless it begins to share the same root cause.

## Practical Resume Notes

- The server should already be running; no default restart should be necessary unless the log shows a new crash.
- The app may still need relaunches when testing Swift-side changes.
- The current debugging priority should be:
  - real tool/session correctness first
  - scrolling backlog second

## Short Honest Summary

The main lesson from this debugging pass is that "parallel sessions broke Fawx" was only partly true. Parallel usage surfaced several latent issues at once. The real blocking failures were mostly engine and session-history correctness bugs, plus one bad local LaunchAgent install on this machine. Those engine-side issues now have real fixes in place. The remaining known unsolved issue from this thread is the scroll / blank-session rendering bug, which should be debugged independently.
