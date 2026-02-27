# Retroactive Pressure Test: Tool Schema Design

*Pressure test for #478 — Tier 2 retroactive audit*
*Fawx: `PhoneTools.kt` (530 lines, 27 tools) | OpenClaw: `types.ts` (`AgentTool`), extension tool registration*

---

## 1. OpenClaw's Architecture (Source-Level)

### Tool Definition

OpenClaw uses TypeBox schemas for typed tool definitions:

```typescript
interface AgentTool<TParameters extends TSchema, TDetails> extends Tool<TParameters> {
  label: string;  // Human-readable UI label
  execute: (toolCallId, params: Static<TParameters>, signal?, onUpdate?) => Promise<AgentToolResult<TDetails>>;
}
```

The base `Tool<TParameters>` (from `@mariozechner/pi-ai`) provides:
- `name: string`
- `description: string`
- `parameters: TParameters` — TypeBox schema, compiled to JSON Schema at the LLM boundary

**Key characteristics:**
1. **TypeBox schemas** — compile-time type safety via `Static<TParameters>`. Parameters are validated before execution via `validateToolArguments()`.
2. **Self-executing** — each tool carries its own `execute` function. No central dispatch.
3. **Typed results** — `AgentToolResult<TDetails>` with `content: (TextContent | ImageContent)[]` and typed `details: TDetails`.
4. **Streaming updates** — `onUpdate` callback for progressive tool results.
5. **Cancellation** — `AbortSignal` threaded through to every tool.
6. **Extension wrapping** — tools can be wrapped with pre/post hooks via `ext-wrapper.ts`.

### Tool Registration

Tools are registered via extensions (plugins). Each extension provides tool definitions that are wrapped into `AgentTool` objects. The application layer (agent-session) collects tools from all extensions and passes them to the agent loop.

### Schema Validation

`validateToolArguments()` (from pi-ai) validates arguments against the TypeBox schema before execution. Invalid arguments throw before the tool runs.

---

## 2. Fawx's Architecture

### Tool Definition

Fawx defines tools as static `Tool` objects in `PhoneTools.kt`:

```kotlin
data class Tool(
    val name: String,
    val description: String,
    val inputSchema: Map<String, Any>
)
```

**Schema definition pattern** — raw `Map<String, Any>` representing JSON Schema:
```kotlin
val TAP = Tool(
    name = "tap",
    description = "Tap a UI element by its numeric ID from the screen content",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "element_id" to mapOf(
                "type" to "integer",
                "description" to "The numeric ID of the element to tap"
            )
        ),
        "required" to listOf("element_id")
    )
)
```

### 27 Tools Inventory

| Category | Tools | Count |
|----------|-------|-------|
| **UI Interaction** | tap, tap_text, long_press, type_text, swipe, scroll | 6 |
| **Navigation** | press_back, press_home, open_app, open_notifications | 4 |
| **Screen** | read_screen, screenshot, wait | 3 |
| **Clipboard** | copy, set_clipboard, paste | 3 |
| **Notifications** | read_notifications, tap_notification, dismiss_notification, reply_notification | 4 |
| **Files** | read_file, write_file, list_files | 3 |
| **Memory** | remember, recall, list_memories | 3 |
| **Reasoning** | think | 1 |

All tools collected in `PhoneTools.ALL` list (line ~500).

### Parameter Types Used

- `integer` — element_id (tap, long_press)
- `string` — text, direction, app_name, path, content, query, prompt, notification_key
- `string` with `enum` — direction (swipe: up/down/left/right; scroll: up/down)
- `boolean` — include_ongoing (read_notifications)
- No nested objects, no arrays as parameters

### Tool Execution / Dispatch

`PhoneAgentApi.executeToolCall()` — ~300-line `when` block dispatching on `toolCall.name`:
- Parameter extraction with type casting and validation: `(toolCall.input["element_id"] as? Number)?.toInt() ?: throw IllegalArgumentException(...)`
- Result is always `String`
- Errors caught at two levels: per-tool `throw` → outer `catch` returning `"Failed: ${toolCall.name}: ${e.message}"`

### UI-Mutating Tool Classification

`PhoneAgentApi.UI_MUTATING_TOOLS` (companion object):
```kotlin
val UI_MUTATING_TOOLS = setOf(
    "tap", "tap_text", "long_press", "type_text",
    "swipe", "scroll", "press_back", "press_home",
    "open_app", "open_notifications"
)
```

After these tools, screen content is refreshed and appended to the tool result.

### Schema Validation

**No pre-execution schema validation.** Parameters are validated inline during execution via type casts and null checks. Invalid params throw `IllegalArgumentException`, caught by the outer handler.

### Task Completion Convention

No explicit `task_complete` tool. When the model returns `stopReason: "end_turn"` with text and no tool calls, the task is considered complete.

---

## 3. Comparison Table

| Aspect | OpenClaw | Fawx | Notes |
|--------|----------|--------|-------|
| **Schema format** | TypeBox (compile-time typed) | Raw `Map<String, Any>` (runtime JSON Schema) | OpenClaw has static type safety |
| **Schema validation** | `validateToolArguments()` pre-execution | Inline during execution (cast + null check) | OpenClaw validates before execute |
| **Tool definition** | Decentralized (per-extension) | Centralized (`PhoneTools.kt` singleton) | Different architectures |
| **Execution model** | Self-executing (`tool.execute()`) | Central dispatch (`when` block in PhoneAgentApi) | See I1 below |
| **Result type** | `AgentToolResult<T>` (text+image blocks, typed details) | `String` | OpenClaw more structured |
| **Streaming results** | ✅ `onUpdate` callback | ❌ | Not needed for phone tools |
| **Cancellation per-tool** | ✅ `AbortSignal` per tool | ❌ Tools run to completion | See D1 |
| **Parameter types** | Full TypeBox (objects, arrays, unions, etc.) | Primitives only (int, string, bool, enum) | Sufficient for phone tools |
| **Tool label** | ✅ `label` field for UI | ❌ Only `name` and `description` | Minor |
| **Tool count** | Variable (extension-dependent) | 27 static tools | Phone domain is bounded |
| **Extension hooks** | ✅ Pre/post via ext-wrapper | ❌ | Not needed yet |
| **No-param tools** | N/A | Empty properties + required | Works but verbose |

---

## 4. Gaps Found

### Critical

**None.** The tool schema design is functional and appropriate for the phone agent domain.

### Deferred

#### D1: No Per-Tool Cancellation
**Gap:** OpenClaw threads `AbortSignal` to every tool execution. Fawx tools run to completion — there's no way to cancel a long-running tool (e.g., `screenshot` with slow vision API call).
**Impact:** User pressing "Stop" during a screenshot's vision call must wait for completion. Currently mitigated by the cancellation check at the next boundary.
**Recommendation:** H3 — add `Job` cancellation to `executeToolCall()` for long-running tools. Low priority since most phone tools complete in <500ms.

#### D2: No Pre-Execution Schema Validation
**Gap:** OpenClaw validates tool arguments against the schema before execution. Fawx validates inline, meaning invalid params may cause partial execution (e.g., a tool that does side effects before hitting the invalid param).
**Impact:** Low for current tools — parameter extraction happens at the top of each `when` branch before any side effects. But fragile if new tools are added carelessly.
**Recommendation:** H3 — consider adding a `validateParams(tool, input)` step before the `when` dispatch. Can be derived from existing `inputSchema` definitions.

#### D3: Raw Map Schema Definition
**Gap:** Fawx schemas are `Map<String, Any>` — no compile-time validation that schemas are well-formed. A typo in a property name or type string is only caught at runtime (when the LLM tries to use the tool).
**Impact:** Low — 27 tools are static and tested. But any new tool addition risks silent schema errors.
**Recommendation:** H3 — consider a schema builder DSL:
```kotlin
fun tool(name: String, description: String, block: SchemaBuilder.() -> Unit): Tool
// Usage: tool("tap", "Tap element") { required("element_id", integer("The ID")) }
```

#### D4: String-Only Tool Results
**Gap:** All tool results are `String`. No structured output (images, JSON, typed metadata).
**Impact:** `screenshot` returns a text description rather than the image itself. Memory tools return hand-crafted JSON strings. If tools need to return images or structured data to the model, the string format is limiting.
**Recommendation:** H3 — same as D2 in the boundary checks pressure test. Revisit when needed.

### Intentional Divergences

#### I1: Central Dispatch vs Self-Executing Tools
Fawx uses a `when` dispatch in `PhoneAgentApi.executeToolCall()` because tools need Android platform APIs (ScreenReader, ClipboardHelper, NotificationHelper) that aren't available in `:core`. The dispatch acts as a bridge between the pure tool definitions and the platform layer. This is a valid architectural choice for Android.

OpenClaw's self-executing tools work because each tool is a Node.js function with direct access to its dependencies via closure/injection.

#### I2: Fixed Tool Set
Fawx has 27 static tools. OpenClaw's tool set is dynamic (extension-dependent). The phone agent domain is bounded — the set of phone interactions is finite and well-known. A dynamic tool registry adds complexity without benefit here.

#### I3: No Tool Streaming
OpenClaw supports progressive tool results via `onUpdate`. Phone tools are fast (<500ms typical) and don't benefit from streaming. The `screenshot` tool (which calls vision API) could benefit, but the description is short enough to return atomically.

#### I4: Task Completion via Stop Reason
Both systems use the model's natural stop signal (end_turn / stop) rather than an explicit task_complete tool. This is the correct pattern — it avoids wasting a tool call on signaling completion.

---

## 5. Recommendations

1. **No blockers for H2.** Tool schema design is adequate for the phone agent domain.

2. **Watch for schema drift** (D3) as new tools are added. The raw Map approach works for 27 tools but doesn't scale gracefully. A builder DSL would be a quality-of-life improvement.

3. **Pre-execution validation** (D2) is a good defensive measure. Consider implementing it when the tool count grows or when tools with side effects are added.

4. **Per-tool cancellation** (D1) matters most for `screenshot` (vision API latency). Could be a targeted fix rather than a systemic change.

5. **The notification key validation** in `requireValidNotificationKey()` is a good defensive pattern — validates format before passing to platform APIs. Apply similar validation to other tool inputs if injection risks emerge.
