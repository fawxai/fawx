# Nova Pre-PoC: Mac Mini Agent Simulation
## Phase 0.5 Product Requirements Document

**Version**: 0.1 — February 7, 2026
**Author**: Joe + Claude
**Status**: Ready for execution
**Platform**: macOS (Apple Silicon Mac Mini)
**Duration**: 8 weeks (part-time) / 4 weeks (full-time)
**Hardware Required**: Mac Mini (M-series), no phone needed

---

## 1. Purpose

Validate the entire cognitive pipeline — intent classification, LLM routing, action planning, skill execution, storage, and security — without touching Android. The Mac Mini runs ~80% of the final codebase natively. Everything built here transfers directly to the phone (Horizon 1) and the OS (Horizon 2) with zero rewrite.

The pre-PoC answers one critical question: **Does the agent architecture actually work?** Can it understand commands, reason about multi-step plans, execute skills, and maintain context — before we spend a single day on Android integration?

### 1.1 Success Criteria

The pre-PoC is complete when a user can sit at a terminal and:

1. Type a natural language command
2. Watch it get classified by a local LLM (Gemma 3n via llama.cpp)
3. See it routed to local or cloud (Claude) based on complexity
4. Receive an action plan with concrete steps
5. See the plan validated against the action policy engine
6. Watch simulated execution with step-by-step output
7. Ask follow-up questions with conversational context retained
8. Install and invoke a WASM skill (e.g., weather) that the agent uses autonomously
9. Review an audit log of everything the agent did
10. See all sensitive data encrypted at rest

### 1.2 What This Is NOT

- Not a phone app. No Android, no touch injection, no screen capture.
- Not a voice assistant. Text input only (voice comes in Horizon 1).
- Not a product. No UI polish, no onboarding, no error recovery UX.
- It IS the real codebase. Not a prototype to throw away.

---

## 2. Architecture for Pre-PoC

```
┌──────────────────────────────────────────────────────────┐
│  Mac Mini (aarch64-apple-darwin)                         │
│                                                          │
│  ┌──────────┐                                            │
│  │ nv-cli   │ ← User types commands here                 │
│  │ terminal │                                            │
│  │ REPL     │                                            │
│  └────┬─────┘                                            │
│       │                                                  │
│  ┌────▼─────────────────────────────────────────────┐    │
│  │ nv-agent                                         │    │
│  │ orchestrator: receive → classify → route → plan  │    │
│  │               → policy check → execute → respond │    │
│  └──┬──────────┬──────────┬──────────┬──────────┬───┘    │
│     │          │          │          │          │         │
│  ┌──▼───┐ ┌───▼───┐ ┌───▼────┐ ┌───▼────┐ ┌──▼─────┐  │
│  │nv-llm│ │pc-    │ │pc-     │ │pc-     │ │pc-     │  │
│  │local │ │secur- │ │skills  │ │storage │ │sync    │  │
│  │+cloud│ │ity    │ │WASM    │ │encrypt │ │cloud   │  │
│  └──────┘ └───────┘ └────────┘ └────────┘ └────────┘  │
│                                                          │
│  Simulated (mock) layer:                                 │
│  ┌──────────────────────────────────────────────────┐    │
│  │ nv-phone-sim: Fake phone state, fake apps,       │    │
│  │ fake notifications, fake screen content.          │    │
│  │ Agent plans actions against this simulated phone. │    │
│  └──────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────┘
```

### 2.1 The Simulator Layer

Instead of nv-phone (which needs Android), the pre-PoC includes a `nv-phone-sim` module — a mock phone environment that lets the agent plan and "execute" actions against a virtual device state.

```rust
// nv-phone-sim provides:
pub struct SimulatedPhone {
    screen: SimScreen,           // Current "app" and UI elements
    apps: Vec<SimApp>,           // Installed apps with mock behaviors
    notifications: Vec<SimNotif>,// Pending notifications
    contacts: Vec<SimContact>,   // Fake contacts
    calendar: Vec<SimEvent>,     // Fake calendar events
    clipboard: String,
    settings: SimSettings,       // WiFi, Bluetooth, volume, etc.
}

impl PhoneActions for SimulatedPhone {
    fn tap(&mut self, target: &str) -> Result<ActionResult>;
    fn launch_app(&mut self, app: &str) -> Result<ActionResult>;
    fn type_text(&mut self, text: &str) -> Result<ActionResult>;
    fn read_screen(&self) -> ScreenState;
    fn get_notifications(&self) -> Vec<Notification>;
    // ... same trait the real nv-phone will implement
}
```

The agent code doesn't know or care whether it's talking to a simulated phone or a real one. This is the key architectural win: the `PhoneActions` trait is the abstraction boundary.

---

## 3. Epics

### Epic 1: Project Skeleton & Build System
Establish the Cargo workspace, CI, and all crate stubs so every subsequent sprint starts from a compiling codebase.

### Epic 2: Local LLM Integration
Get llama.cpp running on Apple Silicon, load Gemma 3n, and classify intents from natural language input.

### Epic 3: Cloud LLM Integration
Connect to Claude API with streaming, tool use, and confidence-based routing between local and cloud.

### Epic 4: Agent Reasoning Core
Build the orchestrator loop: perception → intent → routing → planning → execution → response.

### Epic 5: Action Policy Engine
Implement the security boundary that validates every action plan before execution.

### Epic 6: Encrypted Storage
Build the encrypted key-value store for credentials, conversation history, and preferences.

### Epic 7: Phone Simulator
Create the mock phone environment so the agent can generate and "execute" realistic action plans.

### Epic 8: WASM Skill Runtime
Load and execute signed WASM skills with capability enforcement.

### Epic 9: Interactive CLI & Integration Testing
Build the REPL interface, wire everything together, and validate end-to-end flows.

---

## 4. Sprint Plan

### Sprint 1: Foundation (Week 1-2)
**Goal**: Compiling workspace with all crates stubbed, local LLM producing output.

#### Epic 1: Project Skeleton

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 1.1 | Initialize Cargo workspace with `nova/Cargo.toml` listing all member crates | `cargo check` passes with empty crates | 1h |
| 1.2 | Create nv-core crate with `lib.rs`, `config.rs`, `message.rs`, `event.rs`, `error.rs`, `types.rs` — all with placeholder types and doc comments | Each module has at least one public type defined | 2h |
| 1.3 | Define `Config` struct in nv-core with JSON5 deserialization (model paths, API keys path, log level, storage path, policy path) | `Config::load("config.json5")` works with a sample config file | 1h |
| 1.4 | Define core message types: `UserInput`, `Intent`, `ActionPlan`, `ActionStep`, `ActionResult`, `AgentResponse` | Types compile with serde Serialize/Deserialize | 1h |
| 1.5 | Implement event bus using `tokio::sync::broadcast` — `EventBus::new()`, `subscribe()`, `publish()` | Unit test: publish event, subscriber receives it | 1h |
| 1.6 | Define error taxonomy using `thiserror`: `CoreError`, `LlmError`, `StorageError`, `SecurityError`, `SkillError`, `PhoneError` | Each error variant has a human-readable message | 30m |
| 1.7 | Create all remaining crate stubs (nv-agent, nv-llm, nv-phone, nv-voice, nv-security, nv-skills, nv-sync, nv-storage, nv-sensors, nv-cli) with empty `lib.rs` importing nv-core | `cargo check --workspace` passes | 1h |
| 1.8 | Create `nv-phone-sim` crate stub alongside nv-phone (feature-gated: sim vs real) | Crate compiles, feature flag documented | 30m |
| 1.9 | Set up `.cargo/config.toml` with default target, release profile (LTO, strip), and Android cross-compilation target (commented out for now) | `cargo build --release` produces optimized binary | 30m |
| 1.10 | Create `justfile` or `Makefile` with common commands: `build`, `test`, `run`, `lint`, `fmt`, `cross-android` | `just build` and `just test` work | 30m |
| 1.11 | Set up GitHub repo with CI: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check` on push | CI passes on first push | 1h |
| 1.12 | Add `ARCHITECTURE.md` in repo root linking to SPEC.md decisions and crate responsibilities | Document exists and is accurate | 30m |

#### Epic 2: Local LLM Integration (Part 1)

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 2.1 | Create `ffi/llama-cpp-sys/` with build.rs that compiles vendored llama.cpp for macOS aarch64 with Metal acceleration | `cargo build -p llama-cpp-sys` succeeds, produces static lib | 3h |
| 2.2 | Pin specific llama.cpp commit (latest stable tag) and document in `ffi/llama-cpp-sys/VENDOR.md` | Commit hash recorded, build is reproducible | 30m |
| 2.3 | Write safe Rust wrapper in nv-llm: `LocalModel::load(path, params) -> Result<Self>` | Loads a GGUF file without crashing | 2h |
| 2.4 | Implement `LocalModel::generate(prompt, max_tokens) -> Result<String>` with basic sampling | Given a prompt, produces coherent text | 2h |
| 2.5 | Download Gemma 3n 1.7B Q4_K_M GGUF, store in `~/.nova/models/` | Model file exists, path configurable | 30m |
| 2.6 | Write integration test: load model, send "What is 2+2?", verify non-empty response | Test passes, response is reasonable | 1h |
| 2.7 | Add Metal GPU acceleration flag to llama-cpp-sys build.rs (macOS only, feature-gated) | Inference runs on GPU, measurably faster than CPU-only | 2h |
| 2.8 | Benchmark: measure tokens/sec on Mac Mini for Gemma 3n Q4_K_M, log to console | Benchmark runs, tok/sec printed (target: >30 tok/sec on M-series) | 1h |

**Sprint 1 deliverable**: `cargo run -p nv-cli` loads a local LLM and responds to a hardcoded prompt. All crates exist and compile.

---

### Sprint 2: Thinking Locally & Remotely (Week 3-4)
**Goal**: Agent classifies intents locally, routes complex queries to Claude, returns structured plans.

#### Epic 2: Local LLM Integration (Part 2 — Intent Classification)

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 2.9 | Create intent classification system prompt in `nv-llm/src/prompts/intent.txt` — defines categories: LAUNCH_APP, SEARCH, NAVIGATE, MESSAGE, CALENDAR, SETTINGS, QUESTION, COMPLEX_TASK, CONVERSATION | Prompt file exists with clear category definitions and examples | 1h |
| 2.10 | Implement `IntentClassifier::classify(input: &str) -> Result<Intent>` using local model with constrained output parsing | Returns structured Intent with category, confidence, extracted entities | 3h |
| 2.11 | Define `Intent` struct: `category: IntentCategory`, `confidence: f32`, `entities: HashMap<String, String>`, `raw_input: String` | Struct defined in nv-core/types.rs | 30m |
| 2.12 | Build intent test suite: 50 example inputs with expected categories, run classifier against all, measure accuracy | Test suite exists, accuracy logged (target: >85% on test set) | 2h |
| 2.13 | Implement confidence threshold: if local confidence < 0.7, flag for cloud routing | Unit test: ambiguous input gets low confidence | 1h |

#### Epic 3: Cloud LLM Integration

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 3.1 | Implement Claude API client in nv-llm/cloud.rs: `CloudModel::new(api_key, model)` | Compiles with reqwest, API key loaded from encrypted storage or env var | 1h |
| 3.2 | Implement `CloudModel::generate(messages, system_prompt) -> Result<String>` with streaming via SSE | Streams response tokens, returns complete response | 3h |
| 3.3 | Implement `CloudModel::generate_with_tools(messages, tools) -> Result<ToolResponse>` for structured action plan output | Returns parsed tool calls matching defined schema | 3h |
| 3.4 | Define tool schema for action planning: `plan_actions(steps: Vec<ActionStep>)` where each step has `action`, `target`, `parameters`, `confirmation_required` | Schema defined, Claude returns valid tool calls | 2h |
| 3.5 | Implement streaming callback: print tokens as they arrive to terminal (visual feedback during generation) | User sees response building character by character | 1h |
| 3.6 | Error handling: retry with exponential backoff (3 attempts), timeout (30s), rate limit detection, API key validation on startup | Each error case has a test | 2h |
| 3.7 | Implement conversation history: `ChatHistory` struct that maintains message list, truncates to context window limit | History grows, truncates oldest messages when exceeding token limit | 1h |

#### Epic 3 (continued): LLM Router

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 3.8 | Implement `LlmRouter` in nv-llm/router.rs: takes `UserInput`, runs local intent classification, decides local vs cloud | Router returns `RoutingDecision::Local` or `RoutingDecision::Cloud` with rationale | 2h |
| 3.9 | Define routing rules: LAUNCH_APP/SETTINGS/simple QUESTION → local. COMPLEX_TASK/PLAN/multi-step → cloud. MESSAGE → cloud (needs careful wording). Low confidence → cloud. | Rules documented and tested | 1h |
| 3.10 | Implement fallback: if local model fails (OOM, timeout), automatically route to cloud with warning log | Fallback triggers on simulated local failure | 1h |
| 3.11 | Implement `LlmProvider` trait that abstracts over local and cloud: `async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>` | Both LocalModel and CloudModel implement the trait | 1h |

**Sprint 2 deliverable**: Type "open Chrome" → classified locally as LAUNCH_APP. Type "plan a three-day trip to Kyoto with budget considerations" → routed to Claude, returns structured multi-step plan. Both paths work from the CLI.

---

### Sprint 3: Security & Storage (Week 5)
**Goal**: Action policy engine validates plans, encrypted storage holds credentials and history.

#### Epic 5: Action Policy Engine

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 5.1 | Define `PolicyRule` struct: `action_pattern: String`, `category: PolicyCategory` (Allow/Confirm/Deny/RateLimit), `description: String` | Struct in nv-security, serde-deserializable | 30m |
| 5.2 | Create default policy file `default-policy.json5` with all rules from the spec (ALLOW: launch app, read screen, etc. CONFIRM: send message, modify contacts. DENY: factory reset, disable policy engine.) | Policy file loads and parses correctly | 1h |
| 5.3 | Implement `PolicyEngine::new(policy_path) -> Result<Self>` — loads and validates policy rules | Constructor fails on malformed policy files | 1h |
| 5.4 | Implement `PolicyEngine::evaluate(action: &ActionStep) -> PolicyDecision` — matches action against rules, returns Allow/Confirm(reason)/Deny(reason) | Unit tests for each category: allow passes, confirm requires ack, deny blocks | 2h |
| 5.5 | Implement `PolicyEngine::evaluate_plan(plan: &ActionPlan) -> PlanDecision` — evaluates entire plan, returns overall decision (if any step is Deny → whole plan denied; if any step is Confirm → plan needs confirmation) | Plan with mixed steps returns correct aggregate decision | 1h |
| 5.6 | Implement rate limiting: track action counts per minute, trigger Confirm if threshold exceeded | Simulated burst of 31 actions triggers rate limit | 1h |
| 5.7 | Policy file signing: compute HMAC-SHA256 of policy file, verify on load, reject unsigned or tampered files | Modifying one byte of policy file causes load failure | 2h |
| 5.8 | CLI integration: when policy returns Confirm, prompt user with "[Y/n] Allow: send message to Sarah?" | Interactive confirmation works in terminal | 1h |

#### Epic 6: Encrypted Storage

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 6.1 | Add redb dependency to nv-storage, create `Storage::open(path, encryption_key) -> Result<Self>` | Database file created at specified path | 1h |
| 6.2 | Implement encryption layer: AES-256-GCM via ring. `encrypt(plaintext, key) -> ciphertext`, `decrypt(ciphertext, key) -> plaintext`. Unique nonce per write. | Round-trip test: encrypt then decrypt returns original | 2h |
| 6.3 | Implement `Storage::put(table, key, value)` and `Storage::get(table, key) -> Option<Value>` with transparent encryption | Put then get returns original value; raw database file is encrypted | 1h |
| 6.4 | Implement `CredentialStore` wrapper: `store_api_key(name, key)`, `get_api_key(name) -> Option<String>`, `delete_api_key(name)` | Claude API key stored and retrieved correctly | 1h |
| 6.5 | Implement `ConversationHistory` wrapper: `append(role, content)`, `get_recent(n) -> Vec<Message>`, `clear()`, `search(query) -> Vec<Message>` | Conversation history persists across daemon restarts | 2h |
| 6.6 | Implement `Preferences` wrapper: `set(key, value)`, `get(key) -> Option<Value>` for user preferences and learned patterns | Preferences persist and load correctly | 1h |
| 6.7 | Key derivation: `derive_key(user_pin: &str, device_id: &str) -> EncryptionKey` using HKDF-SHA256 via ring | Same PIN + device_id always produces same key; different inputs produce different keys | 1h |
| 6.8 | First-run setup: `nova setup` prompts for PIN, derives key, stores Claude API key | Setup wizard works end-to-end in terminal | 1h |
| 6.9 | Implement append-only audit log: `AuditLog::append(event: AuditEvent)` with chained SHA-256 hashes for tamper detection | Log entries chain correctly; inserting/modifying an entry breaks the chain verification | 2h |
| 6.10 | Audit event types: `CommandReceived`, `IntentClassified`, `PlanGenerated`, `PolicyEvaluated`, `ActionExecuted`, `SkillInvoked`, `ErrorOccurred` | Each event type has a structured format with timestamp | 1h |

**Sprint 3 deliverable**: Agent generates a plan → policy engine evaluates it → user confirms if needed → action logged to audit trail. API key stored encrypted. Conversation history persists between sessions.

---

### Sprint 4: The Simulated Phone (Week 6)
**Goal**: Agent can generate and "execute" realistic action plans against a virtual phone environment.

#### Epic 7: Phone Simulator

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 7.1 | Define `PhoneActions` trait in nv-core: `tap(target)`, `swipe(direction)`, `type_text(text)`, `launch_app(name)`, `go_home()`, `go_back()`, `read_screen()`, `get_notifications()`, `set_setting(key, value)` | Trait defined with Result return types and documentation | 1h |
| 7.2 | Create `SimulatedPhone` struct implementing `PhoneActions` with in-memory state | Struct compiles with all trait methods | 1h |
| 7.3 | Implement `SimApp` with name, package_id, and mock screen states (each app has 2-3 screens with named UI elements) | 10 simulated apps: Chrome, Messages, Calendar, Contacts, Maps, Settings, Camera, Clock, Calculator, Gmail | 2h |
| 7.4 | Implement `SimScreen` with named elements: `buttons: Vec<(String, Rect)>`, `text_fields: Vec<(String, Rect)>`, `labels: Vec<(String, Rect)>` | `read_screen()` returns a structured screen state with element names | 1h |
| 7.5 | Implement `launch_app`: changes SimulatedPhone screen to app's home screen state | `launch_app("Chrome")` → screen shows Chrome elements (address bar, tabs, etc.) | 1h |
| 7.6 | Implement `tap(target)`: finds element by name on current screen, transitions to next screen state if applicable | `tap("address bar")` on Chrome → screen shows keyboard/cursor state | 2h |
| 7.7 | Implement `type_text(text)`: simulates typing into focused text field | After tap("address bar") + type_text("flights to tokyo"), the address bar contains the text | 1h |
| 7.8 | Implement `read_screen()`: returns current screen state as `ScreenState` with app name, visible elements, and text content | ScreenState is structured enough for the agent to reason about | 1h |
| 7.9 | Implement simulated contacts: 10 fake contacts with names, phone numbers, emails | `get_contacts()` returns the list, searchable by name | 30m |
| 7.10 | Implement simulated calendar: 5 fake events for "today" and "tomorrow" | `get_events(date_range)` returns matching events | 30m |
| 7.11 | Implement simulated notifications: push fake notifications (message from Sarah, calendar reminder, weather alert) | `get_notifications()` returns pending notifications with source app and content | 1h |
| 7.12 | Implement `SimulatedPhone::describe()` — human-readable dump of phone state for agent context | Description includes current app, screen elements, pending notifications, recent actions | 1h |
| 7.13 | Wire `PhoneActions` trait into nv-agent executor: agent calls `phone.launch_app()`, `phone.tap()`, etc. The agent doesn't know it's simulated. | Agent generates plan → executor runs it against SimulatedPhone → state changes accordingly | 2h |
| 7.14 | Implement execution logging: each simulated action prints `[SIM] Tapping "Send" button in Messages app` to terminal | User can watch the agent's actions unfold step by step | 1h |

**Sprint 4 deliverable**: Say "text Sarah that I'll be 10 minutes late." Agent classifies intent, generates plan (open Messages → find Sarah → type message → tap send), policy engine confirms (CONFIRM: send message), user approves, executor runs steps against SimulatedPhone, each step logged. Phone simulator state reflects the completed action.

---

### Sprint 5: WASM Skills (Week 7)
**Goal**: Load and execute signed WASM skills with enforced capability boundaries.

#### Epic 8: WASM Skill Runtime

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 8.1 | Add wasmtime dependency to nv-skills. Create `SkillRuntime::new() -> Result<Self>` | wasmtime engine initializes | 30m |
| 8.2 | Define skill manifest format (TOML): name, version, author, signature, capabilities (network domains, storage quota, phone actions, sensors), triggers | Manifest schema documented, sample manifest parses correctly | 1h |
| 8.3 | Define host API (functions exported to WASM guests): `host_log(level, msg)`, `host_http_get(url) -> response`, `host_storage_get(key) -> value`, `host_storage_set(key, value)`, `host_get_location() -> latlon` | Host functions defined as wasmtime Func exports | 2h |
| 8.4 | Implement capability enforcement in host functions: `host_http_get` checks URL against manifest's allowed domains, rejects unauthorized requests | WASM skill trying to fetch unauthorized domain gets CapabilityDenied error | 2h |
| 8.5 | Implement `host_storage_get`/`host_storage_set` with per-skill namespace isolation and storage quota enforcement | Skill A cannot read Skill B's storage; exceeding quota returns error | 1h |
| 8.6 | Create example skill in Rust: `weather-skill` — calls Open-Meteo API, returns formatted weather for a given location | Skill compiles to .wasm, has valid manifest | 2h |
| 8.7 | Implement `SkillLoader::load(wasm_path, manifest_path) -> Result<LoadedSkill>` — verifies signature, parses manifest, instantiates WASM module | Unsigned skill rejected; signed skill loads | 2h |
| 8.8 | Implement Ed25519 skill signing: `nova skill sign <wasm_path>` using key from setup | Produces `.sig` file; verification succeeds with correct key, fails with wrong key | 2h |
| 8.9 | Implement `LoadedSkill::invoke(action: &str, params: &str) -> Result<String>` — calls the skill's exported function with parameters | Weather skill: `invoke("get_weather", "New York")` returns weather data | 1h |
| 8.10 | Wire skills into agent: agent can discover available skills and include them in action plans | Agent asked "what's the weather" routes to weather skill instead of generating a phone action plan | 2h |
| 8.11 | Implement skill installation: `nova skill install <path>` copies WASM + manifest to `~/.nova/skills/`, verifies signature | Installed skill appears in `nova skill list` | 1h |
| 8.12 | Create second example skill: `calculator-skill` — evaluates math expressions. Demonstrates skill with no network capability. | Calculator skill works; attempting network access from it fails | 1h |

**Sprint 5 deliverable**: `nova skill install weather.wasm` → skill registered. Ask "what's the weather in New York?" → agent routes to weather skill → skill calls Open-Meteo API (allowed by manifest) → agent returns formatted response. Audit log shows skill invocation.

---

### Sprint 6: Integration & Polish (Week 8)
**Goal**: Everything wired together, interactive REPL, end-to-end test suite, documentation.

#### Epic 9: Interactive CLI & Integration

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 9.1 | Implement `nova chat` REPL with rustyline (readline-like): prompt, history, multi-line input, Ctrl+C handling | Interactive session with command history that persists between sessions | 2h |
| 9.2 | Add visual feedback in REPL: `[LOCAL]` or `[CLOUD]` prefix on responses, `[POLICY: ALLOW]` or `[POLICY: CONFIRM]` indicators, streaming output for cloud responses | User can see routing decisions and policy evaluations inline | 1h |
| 9.3 | Implement `nova doctor` — checks: model file exists, API key configured, storage accessible, skills directory exists, policy file valid | `doctor` prints checklist with ✓/✗ for each check | 1h |
| 9.4 | Implement `nova config show` — prints current config (redacting API keys) | Config displayed with `api_key: ****...****` | 30m |
| 9.5 | Implement `nova audit show [--last N]` — displays recent audit log entries in human-readable format | Audit entries printed with timestamps and event types | 1h |
| 9.6 | Implement `nova audit verify` — verifies hash chain integrity of audit log | Tampered log detected; intact log verified | 1h |
| 9.7 | Implement `nova skill list` — shows installed skills with name, version, capabilities | Skills listed with capability summary | 30m |
| 9.8 | Implement `nova sim status` — dumps current simulated phone state (current app, screen, notifications) | Phone state printed in readable format | 30m |
| 9.9 | Implement `nova sim notify <app> <message>` — pushes a fake notification to test proactive behavior | Agent processes the notification and suggests/takes action | 1h |
| 9.10 | Implement `nova sim reset` — resets simulated phone to home screen, clears notifications | State returns to initial | 30m |

#### Integration Testing

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 9.11 | End-to-end test: simple command flow (launch app) | Input → classify(local) → plan → policy(ALLOW) → execute(sim) → response | 1h |
| 9.12 | End-to-end test: complex command flow (multi-step) | Input → classify(local, low confidence) → route(cloud) → plan(multi-step) → policy(CONFIRM) → user approves → execute(sim) → response | 1h |
| 9.13 | End-to-end test: skill invocation | Input → classify → route to skill → skill executes → response | 1h |
| 9.14 | End-to-end test: policy denial | Input "factory reset my phone" → plan → policy(DENY) → agent responds "I can't do that" | 30m |
| 9.15 | End-to-end test: conversation context | Multi-turn conversation maintains context ("search for flights to Tokyo" → "now find hotels there" — "there" resolves to Tokyo) | 1h |
| 9.16 | End-to-end test: prompt injection resistance | Simulated notification with injection text → agent does NOT execute injected command | 1h |
| 9.17 | End-to-end test: audit trail completeness | Run 10 commands, verify audit log has entries for every step of every command | 1h |
| 9.18 | End-to-end test: storage persistence | Run commands, quit, restart, verify conversation history and preferences persist | 30m |
| 9.19 | End-to-end test: graceful degradation — cloud unavailable | Set invalid API key, verify agent falls back to local-only with appropriate message | 30m |
| 9.20 | Performance benchmark: measure and log latencies for each pipeline stage (classify, route, plan, policy, execute) for 20 sample commands | Benchmark results printed; identify bottleneck stage | 2h |

#### Documentation

| Task | Description | Acceptance Criteria | Est. |
|---|---|---|---|
| 9.21 | Write README.md: project overview, quickstart (setup + first chat), architecture diagram, contributing guide | Someone can clone and run within 15 minutes | 2h |
| 9.22 | Write SKILLS.md: how to create a skill (Rust template, manifest, build, sign, install), capability reference | A developer can follow the guide to create and install a new skill | 1h |
| 9.23 | Write SECURITY.md: threat model, policy engine design, encryption details, audit log, responsible disclosure | Security model clearly documented | 1h |
| 9.24 | Update SPEC.md with actual metric measurements from performance benchmark | Spec reflects reality, not estimates | 30m |

**Sprint 6 deliverable**: Full interactive demo. `nova chat` runs a persistent session with local+cloud LLM, policy enforcement, phone simulation, skill execution, encrypted storage, and audit logging. Complete test suite passes. Documentation allows another developer to understand and contribute.

---

## 5. Hurdles & Blind Spots: Pre-PoC Specific

### 5.1 Things That Will Go Wrong

**llama.cpp on macOS is not identical to llama.cpp on Android.** The Mac build uses Metal for GPU acceleration; Android will use Vulkan, OpenCL, or CPU-only. Prompt behavior, token generation speed, and even output quality can differ between backends at the same quantization level. Don't over-tune prompts to Mac-specific behavior — keep them robust. Test with CPU-only mode periodically to simulate the phone experience.

**The simulated phone is too easy.** A real phone has apps that crash, take 3 seconds to load, show interstitial ads, have login screens, change layout with updates, and present error dialogs. The simulator has none of this. The agent will work perfectly on the simulator and fail on the first real app. This is expected and acceptable — the simulator validates the *reasoning* pipeline, not the *execution* layer. But don't mistake simulator success for phone success.

**Intent classification accuracy at 1.7B parameters is marginal.** Small models struggle with ambiguous commands ("set it up for tomorrow" — set what? calendar? alarm? reminder?). At 85% accuracy, 1 in 7 commands gets misrouted. This is noticeable. You may need to iterate on prompt engineering more than expected, or evaluate multiple small models (Qwen3-0.6B, Phi-4-mini, Gemma 3n) to find the best classifier at the size constraint.

**Conversation context window management is tricky.** The local model has a 2-4K context window. After 5-6 exchanges, earlier context falls off. The agent loses track of what the user asked three turns ago. Claude has a larger window but costs money per token. The conversation history needs intelligent summarization (compress old turns into a summary) rather than simple truncation.

**WASM skill API design is hard to get right on the first try.** The host API you define now becomes the contract that all future skills depend on. If you get the function signatures wrong, you'll need to version the API and support multiple versions forever. Spend extra time on the host API design. Study WASI and WASI-NN for conventions. Ask: "would this API still make sense for a navigation skill? A banking skill? A smart home skill?"

**The weather skill requires network access, which means you need an HTTP client inside WASM.** WASM doesn't have native network access — you need to implement `host_http_get` as a host function that the Rust runtime executes on behalf of the WASM guest. This works but means the skill can't use arbitrary HTTP libraries; it must use the host-provided function. This is by design (capability enforcement), but it constrains skill development.

### 5.2 Decisions That Can Wait

- **Which whisper model to use**: Mac pre-PoC has no voice. Decide when adding nv-voice in Horizon 1.
- **Android Accessibility Service architecture**: Not relevant until the phone companion app.
- **Touch injection coordinate mapping**: Simulator uses named elements, not coordinates.
- **Battery optimization**: Mac Mini is plugged in.
- **Remote command queue protocol**: No cloud sync in pre-PoC (can be added in Sprint 5-6 if time permits).

### 5.3 Decisions That CANNOT Wait

- **`PhoneActions` trait design**: This is the abstraction boundary between the agent and the phone. Both the simulator and the real phone implement it. Get this right now, because changing it later means changing every action plan the agent generates and every executor integration. Over-design slightly: include methods you think you'll need on the real phone even if the simulator doesn't need them yet.

- **Host API for WASM skills**: Same reasoning. This is the contract between the OS and all future "apps." Version it from day one (`host_api_v1::http_get` etc.) so you can add v2 functions later without breaking v1 skills.

- **Audit log format**: The audit log is append-only with chained hashes. Changing the format later means the hash chain breaks. Define the event schema carefully and include extensibility (e.g., a `metadata: Map<String, Value>` field for future event types).

- **Encryption key derivation**: The KDF (key derivation function) produces the master key that encrypts everything. If you change the KDF later, all existing encrypted data becomes unreadable. Pick HKDF-SHA256, document the exact parameters (salt, info string), and don't change them.

---

## 6. Definition of Done (Pre-PoC Complete)

All of the following must be true:

- [ ] `nova chat` runs interactively on Mac Mini with local + cloud LLM
- [ ] Intent classification routes simple commands locally, complex tasks to Claude
- [ ] Action plans are generated with concrete steps
- [ ] Policy engine evaluates every plan before execution
- [ ] User is prompted for confirmation on CONFIRM-category actions
- [ ] DENY-category actions are blocked with explanation
- [ ] Simulated phone reflects executed actions correctly
- [ ] At least 2 WASM skills installed and invocable by the agent
- [ ] All data at rest is encrypted with PIN-derived key
- [ ] Audit log records every action with verifiable hash chain
- [ ] Conversation history persists across sessions
- [ ] All integration tests pass
- [ ] README enables a new developer to set up in 15 minutes
- [ ] Performance benchmarks recorded and compared to spec targets
- [ ] `nova doctor` validates complete setup

When this checklist is complete, purchase the Pixel 8a and begin Horizon 1.

---

## 7. Transition to Horizon 1

After the pre-PoC is validated, the transition to the Android PoC involves:

1. **Add Android cross-compilation target** to Cargo workspace (uncomment aarch64-linux-android config)
2. **Swap llama-cpp-sys build** from Metal to CPU-only (initially) for Android NDK
3. **Implement real nv-phone** crate behind the `PhoneActions` trait (replacing nv-phone-sim)
4. **Build Android companion app** (Kotlin) with foreground service, accessibility, notification listener
5. **Add nv-voice** with whisper.cpp STT and Porcupine wake word
6. **Deploy to Pixel 8a** via ADB and test against real apps

The agent brain, policy engine, storage, skills, and audit system are unchanged. Only the perception (voice) and action (phone control) layers are new.

Estimated transition time: 2-3 weeks to get the first voice command working on the phone, because the hard problems (reasoning, routing, policy, skills) are already solved.
