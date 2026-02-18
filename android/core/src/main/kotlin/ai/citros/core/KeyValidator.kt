package ai.citros.core

/**
 * Key health validation system for Citros API keys.
 *
 * Provides [KeyHealth] status inference from API call errors via [inferKeyHealth],
 * and a [KeyValidator] functional interface for active validation against providers.
 *
 * Key health is used by the UI to display status indicators (green/red/yellow dots)
 * and can drive automatic retry or key rotation logic in future versions.
 */

/**
 * Health status of an API key, determined by validation against the provider.
 */
enum class KeyHealth {
    /** Key has not been validated yet. */
    UNCHECKED,
    /** Key is valid — successfully authenticated with the provider. */
    VALID,
    /** Key is invalid — provider returned an auth error (401/403). */
    INVALID,
    /** Key has expired or been revoked. */
    EXPIRED,
    /** Validation failed for a non-auth reason (network error, timeout, server error). */
    UNKNOWN
}

/**
 * Infer [KeyHealth] from an API call error.
 *
 * Priority: auth failures are checked first via [ProviderException.isAuthFailure],
 * then message content is parsed for expiry/revocation keywords. Non-auth errors
 * (server errors, network issues) always return [KeyHealth.UNKNOWN] regardless
 * of message content.
 *
 * @param error The error from the API call, or null if the call succeeded
 * @return The inferred health status
 */
fun inferKeyHealth(error: Throwable?): KeyHealth {
    if (error == null) return KeyHealth.VALID

    if (error is ProviderException && error.isAuthFailure) {
        val message = error.message?.lowercase().orEmpty()
        if (message.contains("expired") || message.contains("revoked")) {
            return KeyHealth.EXPIRED
        }
        return KeyHealth.INVALID
    }

    return KeyHealth.UNKNOWN
}

/**
 * Validates an API key by making a lightweight request to the provider.
 *
 * Implementations should send a minimal request (e.g., single-token completion)
 * and return the resulting [KeyHealth]. The call should be fast (≤5 seconds).
 *
 * This is a functional interface so it can be passed as a lambda:
 * ```kotlin
 * val validator: KeyValidator = KeyValidator { provider, rawKey ->
 *     // make API call, return KeyHealth
 * }
 * ```
 */
fun interface KeyValidator {
    suspend fun validate(provider: Provider, rawKey: String): KeyHealth
}
