# On-Device Memory — Task Spec for Jarvis

## Goal

Wire the existing `SqliteMemoryProvider` into `PhoneAgentApi` so the agent
can use `remember`, `recall`, and `list_memories` tools. Currently these tools
exist in code but throw "Memory provider not configured" because the provider
is never passed to `PhoneAgentApi`.

## What Already Exists

### Core module (Clawdio owns — DO NOT MODIFY)
- **`MemoryProvider` interface** (`core/.../MemoryProvider.kt`) — `store()`, `search()`, `delete()`, `list()`
- **`MemoryResult`, `MemoryMetadata`, `MemoryFilter`** data classes
- **`PhoneAgentApi`** constructor takes `memoryProvider: MemoryProvider? = null`
- **Tool definitions** in `PhoneTools.kt`: `REMEMBER`, `RECALL`, `LIST_MEMORIES` (already in `ALL` list)
- **Tool execution** in `PhoneAgentApi.executeToolCall()`: `remember`, `recall`, `list_memories` cases already implemented

### Chat module (Jarvis owns)
- **`SqliteMemoryProvider`** (`chat/.../SqliteMemoryProvider.kt`) — fully implemented with:
  - SQLite table `memories` (id, content, tags, source, created_at)
  - FTS5 full-text search with LIKE fallback
  - Tag normalization (comma-delimited, lowercase)
  - All `MemoryProvider` methods working
- **`SqliteMemoryProviderTest`** — 206 lines of existing tests

## What Needs to Be Done

### Task 1: Wire SqliteMemoryProvider into PhoneAgentApi

**File:** `ChatViewModel.kt`  
**Location:** `buildWalletBackend()` method (~line 370)

**Current code:**
```kotlin
agent = PhoneAgentApi(chatClient, actionClient, actionModelId = actionModelId)
```

**Change to:**
```kotlin
agent = PhoneAgentApi(
    chatClient = chatClient,
    actionClient = actionClient,
    actionModelId = actionModelId,
    memoryProvider = memoryProvider  // new
)
```

You need to:
1. Create the `SqliteMemoryProvider` instance in ChatViewModel (or ChatActivity)
2. Pass it through to `buildWalletBackend()`
3. The SQLiteDatabase should be created/opened in ChatActivity and passed to the ViewModel

**Recommended approach:**
```kotlin
// In ChatActivity or ChatViewModel initialization:
val memoryDb = SQLiteDatabase.openOrCreateDatabase(
    context.getDatabasePath("citros_memories.db"),
    null
)
val memoryProvider = SqliteMemoryProvider(memoryDb)
```

Then pass `memoryProvider` to the ViewModel (via constructor parameter, factory, or a setter).

### Task 2: Also wire into createTestBackend()

**File:** `ChatViewModel.kt`  
**Location:** `createTestBackend()` method (~line 407)

Add optional `memoryProvider` parameter for test injection:
```kotlin
internal fun createTestBackend(
    provider: Provider,
    chatClient: ProviderClient,
    actionClient: ProviderClient = chatClient,
    memoryProvider: MemoryProvider? = null,  // new
    agent: PhoneAgentApi = PhoneAgentApi(chatClient, actionClient, memoryProvider = memoryProvider).also {
        it.phoneControlOverride = true
    }
): TestApiBackend = TestApiBackend(provider, chatClient, actionClient, agent)
```

### Task 3: Database lifecycle management

The SQLiteDatabase needs to be:
- Opened when the app starts (ChatActivity.onCreate)
- Closed when the app is destroyed (ChatActivity.onDestroy)
- Passed to ChatViewModel

**Option A (simplest):** Open in ChatActivity, pass to ViewModel via a setter:  
```kotlin
// ChatActivity.onCreate:
val memoryDb = SQLiteDatabase.openOrCreateDatabase(
    getDatabasePath("citros_memories.db"), null
)
viewModel.setMemoryProvider(SqliteMemoryProvider(memoryDb))

// ChatActivity.onDestroy:
memoryDb.close()
```

**Option B (cleaner):** Use Android's `SQLiteOpenHelper` for version management:  
```kotlin
class MemoryDatabaseHelper(context: Context) : SQLiteOpenHelper(
    context, "citros_memories.db", null, 1
) {
    override fun onCreate(db: SQLiteDatabase) {
        // SqliteMemoryProvider.init handles table creation
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {}
}
```

Option A is fine for v1. Option B is better long-term.

### Task 4: Tests

Add tests verifying the wiring works:
1. **Memory tools return results (not "not configured")** — create a test backend with a real SqliteMemoryProvider (in-memory DB), send a message that triggers `remember`, verify it succeeds
2. **Round-trip test** — remember → recall → verify content matches
3. **Existing `SqliteMemoryProviderTest` should still pass** — don't modify

## File Ownership

| File | Owner | Notes |
|------|-------|-------|
| `SqliteMemoryProvider.kt` | Jarvis | Already exists |
| `ChatActivity.kt` | Jarvis | DB lifecycle |
| `ChatViewModel.kt` | Shared | Only touch `buildWalletBackend()`, `createTestBackend()`, add `setMemoryProvider()` |
| `PhoneAgentApi.kt` | Clawdio | DO NOT MODIFY |
| `MemoryProvider.kt` | Clawdio | DO NOT MODIFY |
| `PhoneTools.kt` | Clawdio | DO NOT MODIFY |

## Branch Convention

Branch: `ui/on-device-memory` from `feat/android-mvp`  
PR title: `[Jarvis] On-device memory: wire SqliteMemoryProvider into agent`  
Target: `feat/android-mvp`

## What NOT to Do

- ❌ Don't modify core module files (PhoneAgentApi, MemoryProvider, PhoneTools)
- ❌ Don't add Room/Hilt/Dagger — keep it simple (raw SQLite or SQLiteOpenHelper)
- ❌ Don't add a memory viewer UI yet — just wire the provider so tools work
- ❌ Don't change the tool definitions or execution logic

## Success Criteria

1. User says "remember that my favorite color is blue" → agent calls `remember` tool → returns success with stored ID
2. User says "what's my favorite color?" → agent calls `recall` tool → returns the stored memory
3. No "Memory provider not configured" errors
4. All existing tests pass
5. New tests cover the wiring
