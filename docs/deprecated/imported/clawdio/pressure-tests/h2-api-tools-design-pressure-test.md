# H2 API Tools — Design Doc + Pressure Test

*Design validated against OpenClaw source (pi-embedded-8DITBEle.js, 2026.2.15)*

---

## 1. Overview

Add two built-in API tools to Fawx:
- **`web_search`** — Search the web via Brave Search API
- **`web_fetch`** — Fetch and extract readable content from a URL

These are the first tools that reach **outside the phone** — critical security boundary.

### Why now
- User asked to prioritize API tools over remaining H1 polish
- Typed `ToolResult` (#496) landed — new tools get clean error typing from day one
- Model floor + tool gating infrastructure already in place from H1

---

## 2. OpenClaw Reference Analysis

### 2.1 Tool Registration Architecture

OpenClaw uses a **plugin registry pattern**:
```
Plugin.registerTool(record, tool, opts) → registry.tools[]
  → factory(ctx) creates tool instance
  → tool.execute(toolCallId, args) handles invocation
```

Each tool has:
- `name: string` — tool identifier
- `label: string` — human-readable display name  
- `description: string` — for system prompt and model context
- `parameters: TypeBoxSchema` — JSON Schema for input validation
- `execute: async (toolCallId, args) => result` — execution function

Tools return structured JSON payloads, not raw strings.

### 2.2 web_search Implementation

**Schema** (`WebSearchSchema`):
```typescript
{
  query: string (required),
  count?: number (1-10, default 5),
  country?: string (2-letter code, default 'US'),
  search_lang?: string (ISO language code),
  ui_lang?: string (ISO language code),
  freshness?: string ('pd'|'pw'|'pm'|'py'|'YYYY-MM-DDtoYYYY-MM-DD')
}
```

**Multi-provider support**: Brave (default), Perplexity (via OpenRouter or direct), Grok (xAI).
- Provider selection via config `tools.web.search.provider`
- API key resolution: config → env var → error with setup instructions

**Features**:
- In-memory result cache (`SEARCH_CACHE`) with TTL
- Freshness validation (shortcuts + date ranges)
- Content wrapping with `wrapWebContent()` for security (untrusted content markers)
- Count clamping: `Math.max(1, Math.min(10, count))`

**Return format** (Brave):
```json
{
  "query": "...",
  "provider": "brave",
  "count": 5,
  "tookMs": 342,
  "externalContent": { "untrusted": true, "source": "web_search" },
  "results": [
    { "title": "...", "url": "...", "description": "...", "published": "...", "siteName": "..." }
  ]
}
```

### 2.3 web_fetch Implementation

**Schema** (`WebFetchSchema`):
```typescript
{
  url: string (required),
  extractMode?: 'markdown' | 'text' (default 'markdown'),
  maxChars?: number (min 100)
}
```

**Multi-backend**: Native fetch + Readability (default), Firecrawl (optional premium).
- Firecrawl requires API key, used when configured
- Native path: fetch HTML → Mozilla Readability → markdown/text extraction
- maxChars cap configurable per-instance

**Return format**:
```json
{
  "url": "...",
  "finalUrl": "...",
  "status": 200,
  "contentType": "text/html",
  "title": "...",
  "extractMode": "markdown",
  "externalContent": { "untrusted": true, "source": "web_fetch", "wrapped": true },
  "truncated": false,
  "length": 5000,
  "text": "..."
}
```

### 2.4 Security Model

OpenClaw's approach:
1. **Content wrapping**: All web content wrapped in `<<<EXTERNAL_UNTRUSTED_CONTENT>>>` markers
2. **Security notice injection**: Warning header prepended to fetched content
3. **Tool policy**: `allow`/`deny` lists per agent, per provider, per group
4. **No confirmation UI**: Tools execute automatically — gating is config-level, not runtime

### 2.5 Tool Policy System

OpenClaw has a multi-layer policy resolution:
```
resolveEffectiveToolPolicy():
  1. globalPolicy (tools.allow/deny in config)
  2. globalProviderPolicy (tools.byProvider.anthropic.allow/deny)
  3. agentPolicy (agents.list[].tools.allow/deny)
  4. agentProviderPolicy (agents.list[].tools.byProvider...)
  5. groupPolicy (channel-specific per-group overrides)
```

ALL policies must agree (AND logic via `isToolAllowedByPolicies`).

---

## 3. Fawx Design

### 3.1 Architecture

```
PhoneTools.kt          — Tool definitions (schemas)
  ↓
PhoneAgentApi.kt       — Tool execution (executeToolCall switch)
  ↓
WebSearchClient.kt     — Brave Search API client (new)
WebFetchClient.kt      — URL fetch + readability (new)
  ↓
ToolResult(text, isError)  — Typed result
```

New files:
- `core/src/main/kotlin/ai/fawx/core/WebSearchClient.kt`
- `core/src/main/kotlin/ai/fawx/core/WebFetchClient.kt`

### 3.2 Tool Definitions

Add to `PhoneTools.kt`:

```kotlin
val WEB_SEARCH = Tool(
    name = "web_search",
    description = "Search the web. Returns titles, URLs, and snippets. Use for current events, facts, or research.",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "query" to mapOf(
                "type" to "string",
                "description" to "Search query string"
            ),
            "count" to mapOf(
                "type" to "integer",
                "description" to "Number of results (1-5, default 3)",
                "minimum" to 1,
                "maximum" to 5
            )
        ),
        "required" to listOf("query")
    )
)

val WEB_FETCH = Tool(
    name = "web_fetch",
    description = "Fetch and read a web page. Returns extracted text content. Use after web_search to read a specific result.",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "url" to mapOf(
                "type" to "string",
                "description" to "URL to fetch (http or https)"
            ),
            "max_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters to return (default 5000)",
                "minimum" to 100,
                "maximum" to 50000
            )
        ),
        "required" to listOf("url")
    )
)
```

### 3.3 Intentional Divergences from OpenClaw

| Aspect | OpenClaw | Fawx | Rationale |
|--------|----------|--------|-----------|
| Search count | 1-10, default 5 | 1-5, default 3 | Phone context window is smaller; 3 results is enough for most queries |
| Search providers | Brave, Perplexity, Grok | Brave only (H2) | YAGNI — add providers when needed |
| Fetch backends | Native + Firecrawl | OkHttp + Readability (Jsoup) | Android-native, no external service dependency |
| Content wrapping | `<<<EXTERNAL_UNTRUSTED_CONTENT>>>` markers | Not needed — phone agent doesn't execute code from web content | No prompt injection risk for phone UI actions |
| Confirmation UI | None (config-level gating) | **User confirmation for web_fetch** | Phone agent acts on user's behalf — fetching arbitrary URLs needs consent |
| Result caching | In-memory with TTL | None initially | Phone sessions are short; cache adds complexity for little gain |
| Tool policy | 5-layer policy resolution | Model tier gating (H1 infrastructure) | Phone has one user, one agent — simpler model |
| Extract modes | markdown, text | text only (H2), markdown later | Phone display is limited; raw text extracts better for phone context |

### 3.4 Security Design

#### 3.4.1 Model Tier Gating

API tools are only available to models at or above the floor:
- `ModelClassifier.TIER_HIGH` (Opus, GPT-4o) → `web_search` ✅, `web_fetch` ✅
- `ModelClassifier.TIER_MEDIUM` (Sonnet, Haiku) → `web_search` ✅, `web_fetch` ✅  
- `ModelClassifier.TIER_LOW` (small/local models) → `web_search` ❌, `web_fetch` ❌

Rationale: Small models + web tools = prompt injection risk. They lack the reasoning to evaluate untrusted content safely.

Implementation: `PhoneAgentApi.getToolsForModel(tier: ModelTier): List<Tool>` — filters tool list based on tier.

#### 3.4.2 Tool Execution Policy

| Tool | Policy | Rationale |
|------|--------|-----------|
| `web_search` | ALLOW | Search queries are low risk — user sees results, agent can't act on them without further tools |
| `web_fetch` | ALLOW | Fetch is read-only — content goes into context, agent still needs phone tools to act |

**Reconsidered**: Originally planned CONFIRM for `web_fetch`, but:
1. The agent already has `open_app` (browser) which is more dangerous
2. Fetch is read-only — it can't modify anything
3. Confirmation fatigue defeats the purpose (user just taps "yes" every time)
4. Model tier gating prevents small models from using it

Both tools ALLOW in H2. Revisit if abuse patterns emerge.

#### 3.4.3 API Key Management

Brave Search API key stored in `WalletManager` alongside LLM API keys:
- New key type: `WalletKey.Type.BRAVE_SEARCH`
- Setup: user enters key during onboarding or via settings
- Fallback: tool returns clear error message if no key configured

### 3.5 WebSearchClient

```kotlin
class WebSearchClient(
    private val httpClient: OkHttpClient,
    private val apiKey: String
) {
    companion object {
        private const val BRAVE_ENDPOINT = "https://api.search.brave.com/res/v1/web/search"
        private const val DEFAULT_COUNT = 3
        private const val MAX_COUNT = 5
        private const val TIMEOUT_SECONDS = 10L
    }

    suspend fun search(
        query: String,
        count: Int = DEFAULT_COUNT
    ): ToolResult {
        val clampedCount = count.coerceIn(1, MAX_COUNT)
        
        val url = "$BRAVE_ENDPOINT?q=${URLEncoder.encode(query, "UTF-8")}&count=$clampedCount"
        
        val request = Request.Builder()
            .url(url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", apiKey)
            .get()
            .build()

        return withContext(Dispatchers.IO) {
            try {
                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ToolResult(
                        "Search failed (${response.code}): ${response.message}",
                        isError = true
                    )
                }
                val body = response.body?.string() ?: return@withContext ToolResult(
                    "Search returned empty response",
                    isError = true
                )
                val results = parseResults(body)
                ToolResult(formatResults(query, results))
            } catch (e: Exception) {
                ToolResult("Search failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    private fun parseResults(json: String): List<SearchResult> { ... }
    private fun formatResults(query: String, results: List<SearchResult>): String { ... }
    
    data class SearchResult(
        val title: String,
        val url: String,
        val description: String,
        val age: String?
    )
}
```

### 3.6 WebFetchClient

```kotlin
class WebFetchClient(
    private val httpClient: OkHttpClient
) {
    companion object {
        private const val DEFAULT_MAX_CHARS = 5000
        private const val MAX_CHARS_CAP = 50000
        private const val TIMEOUT_SECONDS = 15L
    }

    suspend fun fetch(
        url: String,
        maxChars: Int = DEFAULT_MAX_CHARS
    ): ToolResult {
        // Validate URL
        if (!url.startsWith("http://") && !url.startsWith("https://")) {
            return ToolResult("Invalid URL: must start with http:// or https://", isError = true)
        }
        
        val clampedMaxChars = maxChars.coerceIn(100, MAX_CHARS_CAP)

        return withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url(url)
                    .header("User-Agent", "Fawx/1.0")
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    return@withContext ToolResult(
                        "Fetch failed (${response.code}): ${response.message}",
                        isError = true
                    )
                }
                
                val contentType = response.header("Content-Type") ?: ""
                val body = response.body?.string() ?: return@withContext ToolResult(
                    "Fetch returned empty response",
                    isError = true
                )
                
                val text = if (contentType.contains("text/html")) {
                    extractReadableText(body)
                } else {
                    body
                }
                
                val truncated = text.take(clampedMaxChars)
                val wasTruncated = text.length > clampedMaxChars
                
                ToolResult(buildString {
                    append("URL: $url\n")
                    if (wasTruncated) append("(truncated to $clampedMaxChars chars)\n")
                    append("\n$truncated")
                })
            } catch (e: Exception) {
                ToolResult("Fetch failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    private fun extractReadableText(html: String): String {
        // Use Jsoup for HTML → text extraction
        // Jsoup is already common on Android and handles malformed HTML well
        val doc = Jsoup.parse(html)
        // Remove script, style, nav, footer, header elements
        doc.select("script, style, nav, footer, header, aside").remove()
        return doc.text()
    }
}
```

### 3.7 PhoneAgentApi Integration

```kotlin
// In executeToolCall():
"web_search" -> {
    val apiKey = walletManager.getBraveSearchKey()
        ?: return ToolResult("Web search not configured. Add a Brave Search API key in Settings.", isError = true)
    val query = input["query"]?.toString() ?: return ToolResult("Missing required parameter: query", isError = true)
    val count = (input["count"] as? Number)?.toInt() ?: 3
    WebSearchClient(httpClient, apiKey).search(query, count)
}

"web_fetch" -> {
    val url = input["url"]?.toString() ?: return ToolResult("Missing required parameter: url", isError = true)
    val maxChars = (input["max_chars"] as? Number)?.toInt() ?: 5000
    WebFetchClient(httpClient).fetch(url, maxChars)
}
```

### 3.8 Tool Availability

Tools are included in the tool list sent to the model only when:
1. Model tier >= MEDIUM (tier gating from H1)
2. Brave API key is configured (for web_search)
3. Tool is in the active tool set for the current loop

```kotlin
fun getAvailableTools(modelTier: ModelTier, hasSearchKey: Boolean): List<Tool> {
    val tools = mutableListOf<Tool>()
    tools.addAll(PHONE_TOOLS)  // existing 27 tools
    
    if (modelTier >= ModelTier.MEDIUM) {
        if (hasSearchKey) tools.add(WEB_SEARCH)
        tools.add(WEB_FETCH)  // No key needed
    }
    
    return tools
}
```

---

## 4. Comparison Summary

### What we're adopting from OpenClaw:
- Brave Search API as primary search provider
- Similar schema design (query, count, country/freshness optional)
- Structured JSON return format
- Clear error messages when API key is missing

### What we're simplifying:
- Single search provider (Brave) vs multi-provider
- No content security wrapping (phone agent context is different)
- No result caching (short sessions, low query volume)
- No Firecrawl integration (Jsoup is sufficient)
- Simpler tool policy (model tier gating vs 5-layer policy)

### What we're adding beyond OpenClaw:
- Model tier gating (small models can't use web tools)
- Integration with WalletManager for key storage
- Phone-optimized defaults (3 results instead of 5, 5K char fetch limit)

---

## 5. Gaps & Deferred Items

| Gap | Severity | Resolution |
|-----|----------|------------|
| No result caching | Low | Defer — phone sessions are short |
| No Perplexity/Grok support | Low | Defer — Brave is sufficient |
| No freshness filter | Medium | Include in schema but defer implementation to v2 |
| No content security wrapping | Low | Not needed for phone agent context |
| text-only extraction (no markdown) | Low | Jsoup `.text()` is clean; add markdown extraction later |
| No `http_request` (arbitrary HTTP) | Medium | Originally planned, deferred — web_search + web_fetch cover research use cases. Revisit when real need emerges. |

---

## 6. Implementation Plan

### PR 1: WebSearchClient + tool definition
- `WebSearchClient.kt` with Brave API integration
- `PhoneTools.WEB_SEARCH` definition
- `PhoneAgentApi.executeToolCall` case for "web_search"
- `WalletManager` Brave key type + storage
- Model tier gating in tool list
- Tests: WebSearchClient unit tests (mocked HTTP), integration in AgentExecutor

### PR 2: WebFetchClient + tool definition
- `WebFetchClient.kt` with OkHttp + Jsoup extraction
- `PhoneTools.WEB_FETCH` definition
- `PhoneAgentApi.executeToolCall` case for "web_fetch"
- URL validation, content type handling, truncation
- Tests: WebFetchClient unit tests (mocked HTTP), HTML extraction

### Dependencies
- Jsoup: `implementation("org.jsoup:jsoup:1.17.2")` — likely already available or easy to add
- OkHttp: already in the project (used by provider clients)
- No new external services required (Brave API is free tier available)

---

## 7. Test Plan

### Unit tests
- `WebSearchClientTest`: successful search, API error, empty results, count clamping, missing key
- `WebFetchClientTest`: HTML page, non-HTML content, invalid URL, timeout, truncation, large page
- `PhoneToolsTest`: WEB_SEARCH and WEB_FETCH schema validation
- `PhoneAgentApiTest`: web_search/web_fetch routing, missing key error, model tier filtering

### Integration tests  
- `AgentExecutorTest`: web tools in tool loop, error handling, mixed with phone tools
- Model tier gating: verify tools excluded for low-tier models

---

*Created 2026-02-17. Pressure-tested against OpenClaw 2026.2.15 source.*
