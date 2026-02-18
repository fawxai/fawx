# Testing Patterns

Reusable patterns and conventions for Citros Android tests.

## ScriptedProviderClient

A test fake that returns pre-defined responses in sequence, useful for multi-turn conversation tests.

**Location:** `chat/src/test/kotlin/ai/citros/chat/ChatViewModelTest.kt`

### Usage

```kotlin
val scripted = ScriptedProviderClient(
    responses = listOf(
        ChatResponse("First response", toolCalls = listOf(someToolCall)),
        ChatResponse("Second response"),
    )
)

// Each call to chat() returns the next response in order.
// Tracks call count via callCount.get()
// Returns a fallback "No more scripted responses" when exhausted.
```

### When to Use

- Testing multi-turn conversations where each turn needs a different response
- Verifying tool call → tool result → final response flows
- Testing retry/error recovery (mix normal responses with thrown exceptions)

### Key Properties

- `callCount: AtomicInteger` — tracks how many times `chat()` was called
- `lastMessages` — captures the last message list for assertion
- Falls gracefully when exhausted (returns a completion response, doesn't throw)

## Shared Test Fixtures

Common test fakes live in `chat/src/test/kotlin/ai/citros/chat/TestFixtures.kt`:

- **`InMemoryKeyStore`** — in-memory `KeyStore` implementation
- **`InMemoryCredentialStore`** — in-memory `CredentialStore` implementation

Use these instead of creating private inner classes in each test file.

## PR Test Count Accounting

When reporting test counts in PR descriptions, distinguish between:

- **Tests added by this PR:** The actual new `@Test` methods in the diff
- **Branch total:** Cumulative count from `./gradlew test` output

Example:
> This PR adds 11 tests. Branch total: 645 tests (0 failures, 13 skipped).

This avoids confusion when the branch total includes tests from other recently merged PRs.

## Conventions

- Use backtick-delimited test names: `` `descriptive name of behavior` ``
- One assertion focus per test
- TDD: RED → GREEN → REFACTOR
- `@Ignore` with a comment for tests blocked by framework bugs (e.g., Robolectric touch injection)
- Tag tracking issues for ignored tests (see #361)
