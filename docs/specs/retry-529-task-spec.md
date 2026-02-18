# Task Spec: Retry on 529 Overloaded (#511)

**Issue:** #511
**Branch:** `fix/retry-529` from `feat/android-mvp`
**PR title:** `[Jarvis] Retry on Anthropic 529 Overloaded and 503 Service Unavailable`

---

## Problem

When Anthropic returns HTTP 529 (Overloaded), the request fails immediately with no retry.
The error surfaces raw to the user as "Action failed" even for simple chat messages.
529 is explicitly transient — same as 429, should be retried with backoff.

## Changes Required

### File: `core/src/main/kotlin/ai/citros/core/BaseProviderClient.kt`

**1. Add helper method** after `shouldRetryRateLimit()` (~line 111):

```kotlin
/**
 * Whether this HTTP status code is a retryable server error.
 * - 529: Anthropic "Overloaded" — explicitly transient
 * - 503: Service Unavailable — common transient error
 */
private fun isRetryableServerError(code: Int): Boolean =
    code == 529 || code == 503
```

**2. Expand retry condition in `executeRequest()`** (~line 197):

FROM:
```kotlin
// Handle 429 rate limit errors with retry (except daily hard caps)
if (response.code == 429 && attempt < maxAttempts) {
    val parsedError = parseApiError(body)
    if (shouldRetryRateLimit(parsedError)) {
        val retryAfter = response.header("retry-after")?.toLongOrNull()
            ?: (1L shl attempt) // Exponential backoff: 1s, 2s, 4s

        attempt++
        delay(retryAfter * 1000) // Convert to milliseconds
        continue
    }
}
```

TO:
```kotlin
// Handle retryable errors: 429 (rate limit), 529 (overloaded), 503 (service unavailable)
if (attempt < maxAttempts) {
    if (response.code == 429) {
        val parsedError = parseApiError(body)
        if (shouldRetryRateLimit(parsedError)) {
            val retryAfter = response.header("retry-after")?.toLongOrNull()
                ?: (1L shl attempt) // Exponential backoff: 1s, 2s, 4s
            attempt++
            delay(retryAfter * 1000)
            continue
        }
    } else if (isRetryableServerError(response.code)) {
        val retryAfter = response.header("retry-after")?.toLongOrNull()
            ?: (1L shl attempt) // Exponential backoff: 1s, 2s, 4s
        Log.w(TAG, "Retryable server error ${response.code}, attempt $attempt/$maxAttempts, retry in ${retryAfter}s")
        attempt++
        delay(retryAfter * 1000)
        continue
    }
}
```

**3. Update KDocs:**
- Class KDoc (~line 15-28): Add 529/503 alongside 429 in retry description
- `executeRequest()` method KDoc: "Retries on 429 (rate limit), 529 (overloaded), and 503 (service unavailable)"
- Update exhausted-attempts message (~line 252): "Request failed after" instead of "Rate limited after"

## Build & Verify

```bash
cd ~/citros/android
git checkout feat/android-mvp && git pull
git checkout -b fix/retry-529
# Make changes
./gradlew :core:compileDebugKotlin
./gradlew :core:testDebugUnitTest 2>&1 | tail -30
git add -A && git commit -m "Retry on HTTP 529 (Overloaded) and 503 with exponential backoff

Fixes #511"
git push -u origin fix/retry-529
gh pr create --base feat/android-mvp --title "[Jarvis] Retry on Anthropic 529 Overloaded and 503" --body "Fixes #511

Adds HTTP 529 (Anthropic Overloaded) and 503 (Service Unavailable) to the retry logic
with the same exponential backoff used for 429. These are transient server errors that
resolve with short waits."
```

Then comment `@claude review this PR` on the PR.

## Git Flow
1. Create branch from feat/android-mvp
2. Make changes
3. Push, open PR
4. `@claude review this PR`
5. Address ALL review items
6. Push fixes -> `@claude review this PR` again
7. Repeat until clean
8. ALL CI checks must pass
9. `@abbudjoe ready for merge`
