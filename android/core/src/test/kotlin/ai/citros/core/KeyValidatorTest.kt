package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

/**
 * Tests for [inferKeyHealth] key health inference logic.
 *
 * Verifies that auth failures, expiry, server errors, and non-provider
 * exceptions are correctly classified into [KeyHealth] states.
 */
class KeyValidatorTest {

    /** Successful API calls (null error) should result in VALID health. */
    @Test
    fun `inferKeyHealth returns VALID for null error`() {
        assertEquals(KeyHealth.VALID, inferKeyHealth(null))
    }

    /** Auth failure (401/403) without expiry keywords should be INVALID. */
    @Test
    fun `inferKeyHealth returns INVALID for auth failure`() {
        val error = ProviderException(
            provider = Provider.ANTHROPIC,
            statusCode = 401,
            message = "Invalid API key",
            isAuthFailure = true
        )
        assertEquals(KeyHealth.INVALID, inferKeyHealth(error))
    }

    /** Auth failure with "expired" in message should be EXPIRED, not INVALID. */
    @Test
    fun `inferKeyHealth returns EXPIRED for expired key`() {
        val error = ProviderException(
            provider = Provider.ANTHROPIC,
            statusCode = 401,
            message = "API key has expired",
            isAuthFailure = true
        )
        assertEquals(KeyHealth.EXPIRED, inferKeyHealth(error))
    }

    /** Auth failure with "revoked" in message should be EXPIRED. */
    @Test
    fun `inferKeyHealth returns EXPIRED for revoked key`() {
        val error = ProviderException(
            provider = Provider.OPENAI,
            statusCode = 403,
            message = "This key has been revoked",
            isAuthFailure = true
        )
        assertEquals(KeyHealth.EXPIRED, inferKeyHealth(error))
    }

    /** Server errors (non-auth) should be UNKNOWN regardless of message content. */
    @Test
    fun `inferKeyHealth returns UNKNOWN for server error`() {
        val error = ProviderException(
            provider = Provider.ANTHROPIC,
            statusCode = 500,
            message = "Internal server error",
            isAuthFailure = false
        )
        assertEquals(KeyHealth.UNKNOWN, inferKeyHealth(error))
    }

    /** Non-ProviderException errors should always be UNKNOWN. */
    @Test
    fun `inferKeyHealth returns UNKNOWN for non-ProviderException`() {
        val error = RuntimeException("Network unreachable")
        assertEquals(KeyHealth.UNKNOWN, inferKeyHealth(error))
    }

    /**
     * Non-auth error with "expired" in message must return UNKNOWN, not EXPIRED.
     * Only auth failures should be classified as EXPIRED — a 500 "connection expired"
     * is a transient server issue, not a key problem.
     */
    @Test
    fun `inferKeyHealth returns UNKNOWN for non-auth error with expired in message`() {
        val error = ProviderException(
            provider = Provider.ANTHROPIC,
            statusCode = 500,
            message = "Connection expired",
            isAuthFailure = false
        )
        assertEquals(KeyHealth.UNKNOWN, inferKeyHealth(error))
    }

    /**
     * Non-auth error with "revoked" in message must return UNKNOWN.
     * Same principle: only auth failures get the EXPIRED classification.
     */
    @Test
    fun `inferKeyHealth returns UNKNOWN for non-auth error with revoked in message`() {
        val error = ProviderException(
            provider = Provider.OPENAI,
            statusCode = 502,
            message = "Certificate revoked by upstream",
            isAuthFailure = false
        )
        assertEquals(KeyHealth.UNKNOWN, inferKeyHealth(error))
    }
}
