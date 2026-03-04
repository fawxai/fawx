# Sprint 2: Action Playbooks

*Record once. Replay forever. The agent that gets cheaper the more you use it.*

**Status:** Spec
**Prerequisite:** Sprint 1 (Loop Tuning)
**Depends on:** Reliable first executions + per-action failure detection
**Estimated PRs:** 3-4

---

## Problem

Every task execution is exploration. "Send a text to Mom" means the agent navigates the messaging app from scratch: open app → find compose → tap it → find recipient field → type → find send → tap. Seven LLM calls, each processing the full screen. Next time you say "Send a text to Mom," it does the exact same exploration again.

A human learns the path once and follows it forever. The agent should too.

This is also the foundation for the community knowledge moat (H3.7 in the roadmap). If every successful execution produces a reusable playbook, and playbooks can be shared anonymously across users, the product gets better with every user. New competitors face a cold-start problem that compounds over time.

### Cost Curve Inversion

Every other AI product has linear costs — more usage = more API calls. Playbooks make costs *logarithmic*:

| Phase | API Calls per Task | Cost |
|-------|-------------------|------|
| First execution (exploration) | 5-12 LLM calls | Full cost |
| Second execution (playbook) | 1 LLM call (match) + 0-2 (divergence) | ~80% cheaper |
| Nth execution (refined playbook) | 1 LLM call (match) + 0 (stable path) | ~90% cheaper |

---

## Solution

### Architecture

```
Task arrives
  ↓
LLM matches task to playbook (1 cheap call)
  ↓
┌─────────────────────────────────────────┐
│ Playbook found?                         │
│                                         │
│   YES → Follow playbook                 │
│     Step 1: fingerprint match? → act    │
│     Step 2: fingerprint match? → act    │
│     Step N: divergence? → LLM fallback  │
│     Record updated playbook             │
│                                         │
│   NO → Full LLM exploration (Sprint 1)  │
│     Record new playbook on success      │
└─────────────────────────────────────────┘
```

### Key Design Decisions

1. **Playbooks are templates, not recordings.** Steps capture intent (`tap element containing "{recipient}"`) not mechanics (`tap element at index 3`). Parameters are extracted from the task and filled at runtime.

2. **The LLM is still the brain for novel situations.** Playbooks handle the known path. When the current screen diverges from expectations, the agent falls back to LLM exploration seamlessly. No hard failure — just a graceful transition from replay to exploration.

3. **Confidence scoring handles quality naturally.** Bad playbooks from flawed first executions get low confidence through failed replays. Good playbooks get reinforced. No manual curation needed.

4. **SQLite stores everything.** Small data, simple queries, already in the codebase. No vector store, no embeddings, no exotic infrastructure.

---

## Design

### 1. Screen Fingerprinting

The core challenge: screens that are "the same" (same app, same page) have different element IDs, different text content, and different coordinates every time. We need a structural hash that captures the UI *shape* without volatile details.

#### 1.1 ScreenFingerprint

```kotlin
package ai.fawx.core

/**
 * Structural fingerprint of a screen state.
 *
 * Captures the shape of the UI hierarchy without volatile details.
 * Two screens showing "the same page" in the same app should produce
 * the same (or very similar) fingerprint even if text content,
 * element IDs, and coordinates differ.
 */
data class ScreenFingerprint(
    /** Hash of the structural skeleton. */
    val structuralHash: String,
    /** Package name of the foreground app. */
    val packageName: String?,
    /** Activity name (more specific than package). */
    val activityName: String?,
    /** Number of interactive elements on screen. */
    val interactiveCount: Int,
    /** Depth of the deepest node in the tree. */
    val maxDepth: Int,
    /** Ordered list of element class names (for fuzzy matching). */
    val classSignature: List<String>
)
```

#### 1.2 Fingerprint Computation

```kotlin
/**
 * Compute a structural fingerprint from a ScreenContent.
 *
 * Algorithm:
 * 1. Walk the accessibility tree
 * 2. For each node, extract: (className, resourceId, isClickable, isEditable, depth)
 * 3. IGNORE: text content, element IDs, bounds/coordinates, content descriptions
 * 4. Sort by (depth, className, resourceId)
 * 5. Hash the sorted tuple list with SHA-256
 *
 * Why ignore text? Because "Contact: Mom" and "Contact: Dad" are the
 * same screen structurally. The playbook step that says "tap the contact"
 * works regardless of which contact is displayed.
 *
 * Why keep resourceId? Because resource IDs are developer-defined and
 * stable across runs (unlike runtime element IDs). They distinguish
 * "search bar" from "message input" even when both are EditText.
 */
fun computeFingerprint(screen: ScreenContent): ScreenFingerprint {
    val nodes = screen.flattenNodes()
    val structural = nodes.map { node ->
        StructuralNode(
            className = node.className ?: "Unknown",
            resourceId = node.resourceId,  // nullable, stable across runs
            isClickable = node.isClickable,
            isEditable = node.isEditable,
            depth = node.depth
        )
    }.sortedWith(compareBy({ it.depth }, { it.className }, { it.resourceId ?: "" }))

    val hashInput = structural.joinToString("|") {
        "${it.depth}:${it.className}:${it.resourceId ?: ""}:${it.isClickable}:${it.isEditable}"
    }

    return ScreenFingerprint(
        structuralHash = sha256(hashInput),
        packageName = screen.packageName,
        activityName = screen.activityName,
        interactiveCount = nodes.count { it.isClickable || it.isEditable },
        maxDepth = nodes.maxOfOrNull { it.depth } ?: 0,
        classSignature = structural.map { it.className }.distinct()
    )
}
```

#### 1.3 Fuzzy Matching

Exact fingerprint matches cover ~70% of cases. For the rest (minor UI variations, dynamic content), we need fuzzy matching:

```kotlin
/**
 * Compare two fingerprints for similarity.
 *
 * @return Similarity score 0.0-1.0 where 1.0 = identical structure
 */
fun similarity(a: ScreenFingerprint, b: ScreenFingerprint): Float {
    // Different app = no match
    if (a.packageName != b.packageName) return 0f

    // Exact structural hash = perfect match
    if (a.structuralHash == b.structuralHash) return 1f

    // Fuzzy: compare class signatures (ordered list of element types)
    val classOverlap = jaccard(a.classSignature.toSet(), b.classSignature.toSet())

    // Fuzzy: compare interactive element count (should be close)
    val countSimilarity = 1f - (abs(a.interactiveCount - b.interactiveCount).toFloat() /
        max(a.interactiveCount, b.interactiveCount).coerceAtLeast(1))

    // Fuzzy: same activity = bonus
    val activityBonus = if (a.activityName == b.activityName && a.activityName != null) 0.2f else 0f

    return (classOverlap * 0.5f + countSimilarity * 0.3f + activityBonus).coerceIn(0f, 1f)
}

/** Jaccard similarity: |A ∩ B| / |A ∪ B| */
private fun jaccard(a: Set<String>, b: Set<String>): Float {
    val intersection = a.intersect(b).size
    val union = a.union(b).size
    return if (union == 0) 0f else intersection.toFloat() / union
}
```

**Match thresholds:**
- `>= 0.9` → High confidence match, follow playbook
- `0.7 - 0.9` → Partial match, follow playbook with LLM verification at each step
- `< 0.7` → No match, full LLM exploration

---

### 2. Playbook Data Model

#### 2.1 SQLite Schema

```sql
-- Playbooks: top-level task templates
CREATE TABLE playbooks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_package TEXT NOT NULL,
    task_type TEXT NOT NULL,           -- "send_message", "search", "open_settings"
    description TEXT,                  -- Natural language: "Send a text message"
    parameter_schema TEXT,             -- JSON: {"recipient": "string", "message": "string"}

    success_count INTEGER DEFAULT 1,
    fail_count INTEGER DEFAULT 0,
    confidence REAL DEFAULT 0.5,

    app_version_code INTEGER,          -- App version when recorded
    created_at INTEGER NOT NULL,       -- Epoch ms
    last_used_at INTEGER NOT NULL,     -- Epoch ms
    last_succeeded_at INTEGER,         -- Epoch ms

    shared INTEGER DEFAULT 0,          -- Eligible for community sharing
    source TEXT DEFAULT 'local'        -- 'local' or 'community'
);

-- Steps: ordered actions within a playbook
CREATE TABLE playbook_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    playbook_id INTEGER NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    step_order INTEGER NOT NULL,

    screen_fingerprint TEXT NOT NULL,   -- Structural hash of expected screen
    screen_package TEXT,                -- Expected foreground package
    screen_activity TEXT,               -- Expected activity (more specific)

    tool_name TEXT NOT NULL,            -- "tap_text", "type_text", "open_app", etc.
    tool_input_template TEXT NOT NULL,  -- JSON with parameter placeholders: {"text": "{recipient}"}
    selector_strategy TEXT NOT NULL,    -- How to find the target: "text_match", "resource_id", "content_desc"
    selector_value TEXT NOT NULL,       -- The selector: the text/resource_id/content_desc to match

    expected_next_fingerprint TEXT,     -- What screen should look like after this step
    settle_time_ms INTEGER DEFAULT 1000, -- Learned settle time for this app/action

    alternatives TEXT,                  -- JSON: fallback actions if primary fails

    UNIQUE(playbook_id, step_order)
);

-- Indices for fast lookup
CREATE INDEX idx_playbook_lookup ON playbooks(app_package, task_type);
CREATE INDEX idx_playbook_confidence ON playbooks(confidence DESC);
CREATE INDEX idx_step_fingerprint ON playbook_steps(screen_fingerprint);
CREATE INDEX idx_step_playbook ON playbook_steps(playbook_id, step_order);
```

#### 2.2 Room Entities

```kotlin
@Entity(tableName = "playbooks")
data class PlaybookEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    @ColumnInfo(name = "app_package") val appPackage: String,
    @ColumnInfo(name = "task_type") val taskType: String,
    val description: String?,
    @ColumnInfo(name = "parameter_schema") val parameterSchema: String?,

    @ColumnInfo(name = "success_count") val successCount: Int = 1,
    @ColumnInfo(name = "fail_count") val failCount: Int = 0,
    val confidence: Float = 0.5f,

    @ColumnInfo(name = "app_version_code") val appVersionCode: Int? = null,
    @ColumnInfo(name = "created_at") val createdAt: Long,
    @ColumnInfo(name = "last_used_at") val lastUsedAt: Long,
    @ColumnInfo(name = "last_succeeded_at") val lastSucceededAt: Long? = null,

    val shared: Boolean = false,
    val source: String = "local"
)

@Entity(
    tableName = "playbook_steps",
    foreignKeys = [ForeignKey(
        entity = PlaybookEntity::class,
        parentColumns = ["id"],
        childColumns = ["playbook_id"],
        onDelete = ForeignKey.CASCADE
    )]
)
data class PlaybookStepEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    @ColumnInfo(name = "playbook_id") val playbookId: Long,
    @ColumnInfo(name = "step_order") val stepOrder: Int,

    @ColumnInfo(name = "screen_fingerprint") val screenFingerprint: String,
    @ColumnInfo(name = "screen_package") val screenPackage: String?,
    @ColumnInfo(name = "screen_activity") val screenActivity: String?,

    @ColumnInfo(name = "tool_name") val toolName: String,
    @ColumnInfo(name = "tool_input_template") val toolInputTemplate: String,
    @ColumnInfo(name = "selector_strategy") val selectorStrategy: String,
    @ColumnInfo(name = "selector_value") val selectorValue: String,

    @ColumnInfo(name = "expected_next_fingerprint") val expectedNextFingerprint: String?,
    @ColumnInfo(name = "settle_time_ms") val settleTimeMs: Int = 1000,

    val alternatives: String? = null
)
```

#### 2.3 Parameter Templates

Playbook steps use parameter placeholders instead of hardcoded values:

```kotlin
/**
 * Resolve a tool input template by filling parameter placeholders.
 *
 * Template: {"text": "{recipient}", "app_name": "Messages"}
 * Parameters: {"recipient": "Mom"}
 * Result: {"text": "Mom", "app_name": "Messages"}
 *
 * Placeholders use {param_name} syntax. Unresolved placeholders
 * cause the step to fall back to LLM exploration.
 */
fun resolveTemplate(
    template: Map<String, Any>,
    parameters: Map<String, String>
): ResolvedTemplate {
    val resolved = mutableMapOf<String, Any>()
    val unresolved = mutableListOf<String>()

    for ((key, value) in template) {
        if (value is String && value.startsWith("{") && value.endsWith("}")) {
            val paramName = value.substring(1, value.length - 1)
            val paramValue = parameters[paramName]
            if (paramValue != null) {
                resolved[key] = paramValue
            } else {
                unresolved.add(paramName)
            }
        } else {
            resolved[key] = value
        }
    }

    return ResolvedTemplate(
        inputs = resolved,
        unresolvedParams = unresolved,
        isComplete = unresolved.isEmpty()
    )
}

data class ResolvedTemplate(
    val inputs: Map<String, Any>,
    val unresolvedParams: List<String>,
    val isComplete: Boolean
)
```

---

### 3. Playbook Recording

#### 3.1 ExecutionRecorder

```kotlin
package ai.fawx.core

/**
 * Records successful task executions into playbooks.
 *
 * Attached to AgentExecutor as a listener. Captures each tool step's
 * screen fingerprint, action, and outcome. On successful task completion,
 * distills the recording into a playbook.
 *
 * Recording criteria:
 * - Task used 3+ tool steps (skip trivial tasks)
 * - Task completed successfully (status = SUCCESS)
 * - At least one step involved app interaction (skip conversation-only)
 * - Per-action verification passed for all steps (no false successes)
 */
class ExecutionRecorder(
    private val playbookDao: PlaybookDao,
    private val parameterExtractor: ParameterExtractor
) : AgentExecutionListener {

    private val steps = mutableListOf<RecordedStep>()
    private var taskDescription: String? = null

    override fun onTaskStarted(userMessage: String) {
        steps.clear()
        taskDescription = userMessage
    }

    override fun onToolExecuted(
        toolCall: ToolCall,
        screenBefore: ScreenFingerprint?,
        screenAfter: ScreenFingerprint?,
        result: ToolResult,
        failure: ActionFailure?
    ) {
        // Only record steps that succeeded (no failure detected)
        if (failure != null) return

        steps.add(RecordedStep(
            toolCall = toolCall,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            result = result
        ))
    }

    override fun onTaskCompleted(status: TaskStatus, finalResponse: String) {
        if (status != TaskStatus.COMPLETED) return
        if (steps.size < 3) return
        if (steps.none { it.toolCall.name in INTERACTIVE_TOOLS }) return

        val description = taskDescription ?: return
        distillAndSave(description, steps.toList())
    }

    /**
     * Distill a recording into a parameterized playbook.
     *
     * Uses ParameterExtractor to identify which parts of tool inputs
     * were task-specific (parameters) vs structural (constants).
     */
    private fun distillAndSave(
        description: String,
        recordedSteps: List<RecordedStep>
    ) {
        val appPackage = recordedSteps.firstNotNullOfOrNull {
            it.screenBefore?.packageName
        } ?: return

        // Extract parameters from the task description
        val params = parameterExtractor.extract(description, recordedSteps)

        val playbook = PlaybookEntity(
            appPackage = appPackage,
            taskType = params.taskType,
            description = description,
            parameterSchema = params.schemaJson,
            createdAt = System.currentTimeMillis(),
            lastUsedAt = System.currentTimeMillis(),
            lastSucceededAt = System.currentTimeMillis()
        )

        val playbookId = playbookDao.insertPlaybook(playbook)

        for ((index, step) in recordedSteps.withIndex()) {
            val template = params.templatize(step.toolCall.input)
            val selector = inferSelector(step.toolCall)

            playbookDao.insertStep(PlaybookStepEntity(
                playbookId = playbookId,
                stepOrder = index,
                screenFingerprint = step.screenBefore?.structuralHash ?: "",
                screenPackage = step.screenBefore?.packageName,
                screenActivity = step.screenBefore?.activityName,
                toolName = step.toolCall.name,
                toolInputTemplate = template.toJson(),
                selectorStrategy = selector.strategy,
                selectorValue = selector.value,
                expectedNextFingerprint = step.screenAfter?.structuralHash,
                settleTimeMs = inferSettleTime(step)
            ))
        }
    }

    companion object {
        val INTERACTIVE_TOOLS = setOf(
            "tap", "tap_text", "type_text", "swipe", "scroll",
            "long_press", "open_app", "press_back", "press_home"
        )
    }
}
```

#### 3.2 ParameterExtractor

```kotlin
package ai.fawx.core

/**
 * Extracts task parameters from the user message and maps them to
 * tool input fields.
 *
 * Example:
 *   Message: "Send a text to Mom saying I'll be late"
 *   Tool input: {"text": "Mom"} (in tap_text for recipient)
 *   Tool input: {"text": "I'll be late"} (in type_text for message body)
 *
 * Extraction strategy:
 * 1. Identify named entities in the user message (names, phrases, numbers)
 * 2. Find which tool inputs contain those entities
 * 3. Replace those inputs with parameter placeholders
 * 4. Generate a parameter schema
 *
 * This is a heuristic, not perfect. False negatives (missed parameters)
 * mean the playbook is too specific — it works for "Mom" but not "Dad."
 * False positives (wrong parameters) mean the playbook has holes —
 * it tries to fill a slot that should be constant.
 *
 * Confidence scoring handles both: bad playbooks fail on replay,
 * confidence drops, they stop being used.
 */
class ParameterExtractor {

    fun extract(
        userMessage: String,
        steps: List<RecordedStep>
    ): ExtractedParameters {
        val taskType = classifyTaskType(userMessage, steps)
        val entities = extractEntities(userMessage)
        val paramMap = mutableMapOf<String, ParameterDef>()

        for (entity in entities) {
            for (step in steps) {
                for ((key, value) in step.toolCall.input) {
                    if (value is String && value.contains(entity.text, ignoreCase = true)) {
                        val paramName = entity.role ?: "param_${paramMap.size}"
                        paramMap[paramName] = ParameterDef(
                            name = paramName,
                            type = "string",
                            sourceField = key,
                            exampleValue = entity.text
                        )
                    }
                }
            }
        }

        return ExtractedParameters(
            taskType = taskType,
            parameters = paramMap,
            schemaJson = buildSchemaJson(paramMap)
        )
    }

    /**
     * Classify task type from user message and tool usage patterns.
     *
     * Simple heuristic based on keywords and tool sequences:
     * - "send" + type_text → "send_message"
     * - "open" + open_app only → "open_app"
     * - "search" / "find" → "search"
     * - "set timer" / "set alarm" → "set_timer"
     * - Default: first app package + "task"
     */
    /**
     * Note: This heuristic classifier is used ONLY during recording (post-execution).
     * During playbook matching (pre-execution), the LLM classifies intent via
     * PlaybookMatcher's "one cheap call." The two classifiers may produce different
     * task_type values. If this becomes a consistency problem, consolidate to
     * LLM-only classification for both paths. The heuristic exists because
     * post-execution classification has richer signal (tool usage patterns)
     * than pre-execution (user message only).
     */
    private fun classifyTaskType(message: String, steps: List<RecordedStep>): String {
        val lower = message.lowercase()
        return when {
            lower.containsAny("send", "text", "message") && steps.any { it.toolCall.name == "type_text" } -> "send_message"
            lower.containsAny("call", "dial", "phone") -> "make_call"
            lower.containsAny("search", "find", "look up", "google") -> "search"
            lower.containsAny("timer", "alarm") -> "set_timer"
            lower.containsAny("email", "mail") && steps.any { it.toolCall.name == "type_text" } -> "send_email"
            lower.containsAny("open") && steps.size <= 3 -> "open_app"
            lower.containsAny("navigate", "directions") -> "navigate"
            else -> "general_task"
        }
    }

    /**
     * Extract named entities from user message.
     *
     * Phase 1: Simple pattern matching (quoted strings, proper nouns, numbers).
     *          ⚠️ English-only in v1 — regex patterns assume English syntax
     *          ("to {recipient}", "saying {message}"). Non-English support
     *          requires Phase 2 (LLM-assisted extraction).
     * Phase 2: LLM-assisted extraction (one cheap call to identify entities).
     *          Language-agnostic — the LLM handles any language the user speaks.
     */
    private fun extractEntities(message: String): List<Entity> {
        val entities = mutableListOf<Entity>()

        // Quoted strings are explicit parameters
        Regex("\"([^\"]+)\"|'([^']+)'").findAll(message).forEach { match ->
            val text = match.groupValues[1].ifEmpty { match.groupValues[2] }
            entities.add(Entity(text = text, role = null))
        }

        // Words after "to" are often recipients
        Regex("\\bto\\s+(\\w+)").find(message)?.let { match ->
            entities.add(Entity(text = match.groupValues[1], role = "recipient"))
        }

        // Words after "saying" / "that says" are message content
        Regex("\\b(?:saying|that says?)\\s+(.+)", RegexOption.IGNORE_CASE).find(message)?.let { match ->
            entities.add(Entity(text = match.groupValues[1].trimEnd('.', '!', '?'), role = "message"))
        }

        return entities
    }
}

data class Entity(val text: String, val role: String?)

data class ParameterDef(
    val name: String,
    val type: String,
    val sourceField: String,
    val exampleValue: String
)

data class ExtractedParameters(
    val taskType: String,
    val parameters: Map<String, ParameterDef>,
    val schemaJson: String
) {
    /** Replace matched values in tool input with parameter placeholders. */
    fun templatize(input: Map<String, Any>): Map<String, Any> {
        val result = input.toMutableMap()
        for ((paramName, paramDef) in parameters) {
            val current = result[paramDef.sourceField]
            if (current is String && current.contains(paramDef.exampleValue, ignoreCase = true)) {
                result[paramDef.sourceField] = "{$paramName}"
            }
        }
        return result
    }
}
```

---

### 4. Playbook-Guided Execution

#### 4.1 PlaybookMatcher

```kotlin
package ai.fawx.core

/**
 * Matches incoming tasks to stored playbooks.
 *
 * Matching strategy:
 * 1. Query playbooks by app_package (if identifiable from message)
 * 2. Filter by task_type
 * 3. Rank by confidence score
 * 4. Return best match above threshold, or null
 *
 * The LLM is involved in step 1-2: a single cheap call to classify
 * the user's intent into (app, task_type) before database lookup.
 * This is faster than embedding search and more accurate than keyword matching.
 */
class PlaybookMatcher(
    private val playbookDao: PlaybookDao
) {
    companion object {
        const val MIN_CONFIDENCE = 0.3f
        const val MIN_SUCCESS_COUNT = 1
    }

    /**
     * Find the best matching playbook for a task.
     *
     * @param appPackage Target app (from LLM classification)
     * @param taskType Task type (from LLM classification)
     * @return Best matching playbook, or null if none found
     */
    fun findMatch(appPackage: String, taskType: String): PlaybookMatch? {
        val candidates = playbookDao.findByAppAndType(appPackage, taskType)
            .filter { it.confidence >= MIN_CONFIDENCE }
            .filter { it.successCount >= MIN_SUCCESS_COUNT }
            .sortedByDescending { it.confidence }

        val best = candidates.firstOrNull() ?: return null
        val steps = playbookDao.getSteps(best.id)

        return PlaybookMatch(
            playbook = best,
            steps = steps,
            confidence = best.confidence
        )
    }
}

data class PlaybookMatch(
    val playbook: PlaybookEntity,
    val steps: List<PlaybookStepEntity>,
    val confidence: Float
)
```

#### 4.2 PlaybookExecutor

```kotlin
package ai.fawx.core

/**
 * Executes a task using a matched playbook.
 *
 * For each step:
 * 1. Compute current screen fingerprint
 * 2. Compare against expected fingerprint in playbook step
 * 3. If match: execute the step's action deterministically (zero LLM cost)
 * 4. If no match: fall back to LLM exploration for this step
 * 5. After action: verify screen changed to expected next state
 *
 * The executor tracks divergence. If more than 50% of steps required
 * LLM fallback, the playbook is not useful — record a new one instead.
 */
class PlaybookExecutor(
    private val toolRunner: ToolRunner,
    private val screenReader: ScreenReader,
    private val fingerprintComputer: ScreenFingerprintComputer,
    private val playbookDao: PlaybookDao,
    private val llmFallback: AgentExecutor  // For steps where playbook diverges
) {
    companion object {
        const val FINGERPRINT_MATCH_THRESHOLD = 0.7f
        const val MAX_DIVERGENCE_RATIO = 0.5f  // >50% diverged = abandon playbook
    }

    suspend fun execute(
        match: PlaybookMatch,
        parameters: Map<String, String>,
        cancellationToken: CancellationToken
    ): PlaybookExecutionResult {
        var divergedSteps = 0
        var completedSteps = 0
        val executionTrace = mutableListOf<PlaybookStepTrace>()

        for (step in match.steps) {
            if (cancellationToken.isCancelled) {
                return PlaybookExecutionResult(
                    status = PlaybookStatus.CANCELLED,
                    stepsCompleted = completedSteps,
                    stepsDiverged = divergedSteps,
                    trace = executionTrace
                )
            }

            val currentScreen = screenReader.getScreenContent()
            val currentFingerprint = fingerprintComputer.compute(currentScreen)

            val similarity = fingerPrintSimilarity(
                currentFingerprint.structuralHash,
                step.screenFingerprint,
                currentFingerprint,
                step
            )

            if (similarity >= FINGERPRINT_MATCH_THRESHOLD) {
                // Screen matches expectation — execute deterministically
                val template = parseTemplate(step.toolInputTemplate)
                val resolved = resolveTemplate(template, parameters)

                if (!resolved.isComplete) {
                    // Unresolved parameters — need LLM to fill them
                    divergedSteps++
                    executionTrace.add(PlaybookStepTrace(step, StepOutcome.PARAM_UNRESOLVED))
                    // Fall back to LLM for this step
                    val llmResult = llmFallback.executeSingleStep(currentScreen)
                    executionTrace.add(PlaybookStepTrace(step, StepOutcome.LLM_FALLBACK))
                } else {
                    // Execute the action directly
                    val toolCall = ToolCall(
                        id = "playbook_${step.playbookId}_${step.stepOrder}",
                        name = step.toolName,
                        input = resolved.inputs
                    )
                    val result = toolRunner.execute(toolCall, currentScreen)

                    // Verify outcome
                    delay(step.settleTimeMs.toLong())
                    val afterScreen = screenReader.getScreenContent()
                    val afterFingerprint = fingerprintComputer.compute(afterScreen)

                    if (step.expectedNextFingerprint != null &&
                        afterFingerprint.structuralHash != step.expectedNextFingerprint) {
                        // Screen didn't change as expected — this step diverged
                        divergedSteps++
                        executionTrace.add(PlaybookStepTrace(step, StepOutcome.DIVERGED))
                    } else {
                        completedSteps++
                        executionTrace.add(PlaybookStepTrace(step, StepOutcome.MATCHED))
                    }
                }
            } else {
                // Screen doesn't match — fall back to LLM for this step.
                // The LLM sees the current (unexpected) screen and decides
                // what action to take. If it succeeds, we continue with the
                // next playbook step. If it fails, the divergence counter
                // increments and may trigger playbook abandonment.
                divergedSteps++
                val llmResult = llmFallback.executeSingleStep(currentScreen)
                executionTrace.add(PlaybookStepTrace(step, StepOutcome.LLM_FALLBACK))

                // After LLM acts, verify we're back on track for the next step
                // (if the LLM navigated to the expected next screen, the playbook
                // can continue from the next step)
            }

            // Abandon playbook if too many divergences
            val totalAttempted = completedSteps + divergedSteps
            if (totalAttempted >= 3 && divergedSteps.toFloat() / totalAttempted > MAX_DIVERGENCE_RATIO) {
                return PlaybookExecutionResult(
                    status = PlaybookStatus.ABANDONED,
                    stepsCompleted = completedSteps,
                    stepsDiverged = divergedSteps,
                    trace = executionTrace
                )
            }
        }

        // Update playbook confidence
        val success = divergedSteps == 0
        updateConfidence(match.playbook, success)

        return PlaybookExecutionResult(
            status = if (success) PlaybookStatus.SUCCESS else PlaybookStatus.PARTIAL,
            stepsCompleted = completedSteps,
            stepsDiverged = divergedSteps,
            trace = executionTrace
        )
    }

    private fun updateConfidence(playbook: PlaybookEntity, success: Boolean) {
        if (success) {
            playbookDao.incrementSuccess(playbook.id)
        } else {
            playbookDao.incrementFail(playbook.id)
        }
        // Confidence = successes / (successes + failures)
        val updated = playbookDao.getPlaybook(playbook.id) ?: return
        val newConfidence = updated.successCount.toFloat() /
            (updated.successCount + updated.failCount).coerceAtLeast(1)
        playbookDao.updateConfidence(playbook.id, newConfidence)
    }
}

enum class PlaybookStatus {
    SUCCESS,     // All steps matched, zero divergence
    PARTIAL,     // Completed with some LLM fallback steps
    ABANDONED,   // Too many divergences, switched to full exploration
    CANCELLED,   // User cancelled
    FAILED       // Critical failure during playbook execution
}

enum class StepOutcome {
    MATCHED,           // Screen matched, action executed deterministically
    DIVERGED,          // Action executed but outcome didn't match expectation
    SCREEN_MISMATCH,   // Current screen didn't match playbook step
    LLM_FALLBACK,      // Fell back to LLM for this step
    PARAM_UNRESOLVED   // Template had unresolved parameters
}

data class PlaybookStepTrace(
    val step: PlaybookStepEntity,
    val outcome: StepOutcome
)

data class PlaybookExecutionResult(
    val status: PlaybookStatus,
    val stepsCompleted: Int,
    val stepsDiverged: Int,
    val trace: List<PlaybookStepTrace>
)
```

---

### 5. Integration with AgentExecutor

```kotlin
// In AgentExecutor, BEFORE starting the LLM exploration loop:

suspend fun run(userMessage: String) {
    // 1. Classify task intent (one cheap LLM call)
    val intent = classifyTaskIntent(userMessage)

    // 2. Check for matching playbook
    val match = if (intent != null) {
        playbookMatcher.findMatch(intent.appPackage, intent.taskType)
    } else null

    if (match != null && match.confidence >= PlaybookMatcher.MIN_CONFIDENCE) {
        // 3a. Playbook-guided execution
        val params = parameterExtractor.extractFromMessage(userMessage, match.playbook)
        val result = playbookExecutor.execute(match, params, cancellationToken)

        when (result.status) {
            PlaybookStatus.SUCCESS -> {
                // Playbook handled it — done
                delegate.onTaskCompleted(result)
                return
            }
            PlaybookStatus.ABANDONED -> {
                // Playbook didn't work — fall through to full exploration
                // RecordingListener will capture the new exploration as a new playbook
            }
            else -> {
                // Partial success or other — let LLM finish
            }
        }
    }

    // 3b. Standard LLM exploration (existing loop)
    // ExecutionRecorder captures this for future playbook recording
    runExplorationLoop(userMessage)
}
```

---

## Fingerprinting Limitations

| App Type | Fingerprint Quality | Notes |
|----------|-------------------|-------|
| Native Android (Gmail, Settings, Messages) | ✅ Excellent | Stable resource IDs, clean hierarchy |
| Jetpack Compose | ✅ Good | Semantic tree has stable test tags |
| Flutter | ⚠️ Fair | Generic `SemanticsNode`, few resource IDs. Rely on class signature + activity |
| React Native | ⚠️ Fair | Similar to Flutter — generic nodes |
| WebView / Chrome | ❌ Poor | DOM-based tree changes with content. Fingerprint per-page, not per-site |
| Games | ❌ None | No accessibility tree. Playbooks won't work. |

**Mitigation for fair/poor apps:** Fall back to activity-name matching + interactive element count. Less precise but still useful for coarse "am I on the right screen?" checks. The LLM handles fine-grained navigation within those screens.

---

## Test Matrix (Implementation PR Gate)

| Layer | Test ID | Scenario | Expected |
|-------|---------|----------|----------|
| Unit | P1 | Fingerprint: same screen produces same hash | Identical structuralHash |
| Unit | P2 | Fingerprint: same screen different text produces same hash | Identical structuralHash (text ignored) |
| Unit | P3 | Fingerprint: different screen produces different hash | Different structuralHash |
| Unit | P4 | Fingerprint: similarity of near-identical screens > 0.9 | High similarity score |
| Unit | P5 | Fingerprint: similarity of different apps = 0 | Zero similarity |
| Unit | P5a | Fingerprint: same screen across app update (minor layout change) | Fuzzy similarity > 0.7 |
| Unit | P6 | Template resolution: all params filled | isComplete = true, correct values |
| Unit | P7 | Template resolution: missing param | isComplete = false, param in unresolvedParams |
| Unit | P8 | Parameter extraction: "Send text to Mom" | recipient = "Mom" extracted |
| Unit | P9 | Parameter extraction: quoted string | Exact quoted text extracted |
| Unit | P10 | Task classification: "Send a text" → "send_message" | Correct taskType |
| Unit | P11 | Task classification: "Open Gmail" → "open_app" | Correct taskType |
| Unit | P12 | Playbook recording: successful 5-step task → playbook created | Playbook + 5 steps in DB |
| Unit | P13 | Playbook recording: failed task → no playbook | No new records |
| Unit | P14 | Playbook recording: 2-step task → no playbook (too short) | No new records |
| Unit | P15 | Playbook matching: exact app + type → returns match | PlaybookMatch returned |
| Unit | P16 | Playbook matching: low confidence → no match | null returned |
| Unit | P17 | Playbook execution: all steps match → SUCCESS | PlaybookStatus.SUCCESS |
| Unit | P18 | Playbook execution: >50% diverged → ABANDONED | PlaybookStatus.ABANDONED |
| Unit | P19 | Confidence update: success → confidence increases | Higher confidence |
| Unit | P20 | Confidence update: failure → confidence decreases | Lower confidence |
| Unit | P21 | Playbook execution: cancelled mid-step → CANCELLED | PlaybookStatus.CANCELLED |
| Integration | P22 | Full record → match → replay cycle | Second execution uses playbook |
| Integration | P23 | Playbook replay with different parameters | Same path, different values |
| Integration | P24 | Playbook divergence → LLM fallback → new playbook recorded | Two playbooks for same task type |

---

## Blindspots

1. **Task intent classification accuracy.** The single LLM call to classify `(app, task_type)` is the gateway to playbook matching. If it misclassifies, we either miss a valid playbook or try the wrong one. Phase 1 uses simple keyword heuristics + one LLM call. If accuracy is poor, Phase 2 could add embedding-based intent matching.

2. **Parameter extraction is lossy.** The regex-based entity extraction will miss complex parameters ("the restaurant we went to last Tuesday") and miscategorize some entities. This is acceptable in v1 — bad parameter extraction means the playbook is too specific, not dangerous. It still works for the exact same parameters and falls back to LLM otherwise.

3. **Playbook staleness after app updates.** When an app ships a UI redesign, all its playbooks become stale. Fingerprints won't match, confidence degrades naturally, and new playbooks get recorded. The transition period (old playbooks failing before new ones stabilize) could be bumpy. Community sharing helps — the first user to encounter the new UI teaches everyone.

4. **Multiple playbooks for the same task.** Over time, multiple playbooks may exist for "send_message in Messages" — one for each UI variation encountered. The confidence system naturally promotes the best one, but the database could grow. Periodic pruning of low-confidence playbooks (cron job) keeps it manageable.

5. **Subtask interaction.** When a task is decomposed into subtasks (Sprint 1), should each subtask have its own playbook, or should the parent task have a multi-phase playbook? Recommendation: playbooks at the subtask level. Each subtask is a focused, repeatable unit — ideal for playbooks. The orchestration layer (which subtasks to run, in what order) stays with the LLM.

6. **Community sharing privacy.** Before sharing playbooks, strip: all parameter values (only keep schema), timestamps, user-identifying metadata. Share only: app_package, task_type, step structures (fingerprint + tool + selector), confidence. This is structural data about how apps work, not personal data about users.

7. **Schema migration.** The playbook tables are new in Sprint 2, so no migration is needed for v1. However, schema evolution is inevitable (new columns, changed semantics). All playbook schema changes must use standard Room migrations (`Migration(N, N+1)`) with explicit SQL. Destructive fallback (`fallbackToDestructiveMigration()`) is NOT acceptable — it would wipe learned playbooks. Consider adding a `playbook_version` column from day one to support future format changes without full table rebuilds.

8. **Concurrent playbook writes.** If two tasks for the same app complete simultaneously, they might create duplicate playbooks. Use `INSERT OR IGNORE` with a unique constraint on `(app_package, task_type, structural signature of steps)` to deduplicate.

9. **Playbook storage growth.** Heavy users could accumulate thousands of playbooks. Retention policy: keep the top 5 playbooks per `(app_package, task_type)` ranked by confidence. Playbooks with confidence < 0.1 and no successful use in 30 days are pruned automatically. A background job (Room `@Query` with `DELETE`) runs weekly or on app startup.

10. **Metacognitive layer relationship.** Sprint 2 playbooks are the concrete implementation of `agentic-loop-v2.md §8.2`'s "cross-task pattern evolution" for action sequences. The metacognitive layer's "app-specific playbooks (learned, not hardcoded)" described in Phase 4 is exactly what this sprint builds. The metacognitive layer adds *reflection* on top of playbooks (why did this playbook work? what patterns generalize?). Sprint 2 provides the data; the metacognitive layer provides the reasoning.

---

*Sprint 3 (Safety Layer) uses the existing `h2-action-policy-engine.md` spec — no additional design work needed.*
