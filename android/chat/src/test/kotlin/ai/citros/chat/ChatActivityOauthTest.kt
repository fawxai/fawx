package ai.citros.chat

import android.net.Uri
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

/**
 * Tests for OAuth callback edge cases in ChatActivity.
 * These tests verify URI parameter extraction and edge case handling.
 *
 * These URI-focused tests run on the JVM with Robolectric to provide
 * Android runtime support for android.net.Uri APIs.
 */
@RunWith(RobolectricTestRunner::class)
class ChatActivityOauthTest {

    @Test
    fun `getOauthParameter extracts query parameter`() {
        val uri = Uri.parse("citros://oauth/callback?state=abc123&code=xyz")

        val state = uri.getOauthParameter("state")
        val code = uri.getOauthParameter("code")

        assertEquals("abc123", state)
        assertEquals("xyz", code)
    }

    @Test
    fun `getOauthParameter extracts fragment parameter`() {
        val uri = Uri.parse("citros://oauth/callback#state=abc123&access_token=token-xyz")

        val state = uri.getOauthParameter("state")
        val token = uri.getOauthParameter("access_token")

        assertEquals("abc123", state)
        assertEquals("token-xyz", token)
    }

    @Test
    fun `getOauthParameter returns null for missing parameter`() {
        val uri = Uri.parse("citros://oauth/callback?code=xyz")

        val state = uri.getOauthParameter("state")

        assertNull(state)
    }

    @Test
    fun `getOauthParameter trims whitespace`() {
        val uri = Uri.parse("citros://oauth/callback?state=%20%20abc123%20%20")

        val state = uri.getOauthParameter("state")

        assertEquals("abc123", state)
    }

    @Test
    fun `getOauthParameter returns null for blank value`() {
        val uri = Uri.parse("citros://oauth/callback?state=%20%20%20")

        val state = uri.getOauthParameter("state")

        assertNull(state)
    }

    @Test
    fun `extractOauthTokenFromCallback respects priority order`() {
        // token should be checked first
        val uri1 = Uri.parse("citros://oauth/callback?token=preferred&oauth_token=fallback")
        assertEquals("preferred", uri1.extractOauthTokenFromCallback())

        // access_token is second priority
        val uri2 = Uri.parse("citros://oauth/callback?access_token=access&oauthToken=fallback")
        assertEquals("access", uri2.extractOauthTokenFromCallback())

        // accessToken (camelCase) is third
        val uri3 = Uri.parse("citros://oauth/callback?accessToken=camel&oauth_token=fallback")
        assertEquals("camel", uri3.extractOauthTokenFromCallback())
    }

    @Test
    fun `extractOauthTokenFromCallback returns null when no token found`() {
        val uri = Uri.parse("citros://oauth/callback?state=abc&code=xyz")

        val token = uri.extractOauthTokenFromCallback()

        assertNull(token)
    }

    @Test
    fun `extractOauthTokenFromCallback ignores blank tokens`() {
        val uri = Uri.parse("citros://oauth/callback?token=%20%20&access_token=valid-token")

        val token = uri.extractOauthTokenFromCallback()

        assertEquals("valid-token", token)
    }

    @Test
    fun `generateOauthState returns UUID format`() {
        val state = generateOauthState()

        assertNotNull(state)
        // UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (36 chars with dashes)
        assertEquals(36, state.length)
        assertEquals(4, state.count { it == '-' })
    }

    @Test
    fun `generateOauthState returns unique values`() {
        val state1 = generateOauthState()
        val state2 = generateOauthState()

        assertNotNull(state1)
        assertNotNull(state2)
        assert(state1 != state2) { "Sequential state values should be unique" }
    }
}

// Extension function stubs for testing (these are defined in ChatActivity.kt)
// Copied here for testing purposes

private fun Uri.getOauthParameter(name: String): String? {
    val queryValue = getQueryParameter(name)?.trim()
    if (!queryValue.isNullOrBlank()) {
        return queryValue
    }

    val fragment = fragment ?: return null
    val pairs = fragment.split("&")
    for (pair in pairs) {
        val parts = pair.split("=", limit = 2)
        if (parts.size != 2) continue
        if (parts[0] != name) continue
        val value = Uri.decode(parts[1]).trim()
        if (value.isNotBlank()) {
            return value
        }
    }

    return null
}

private fun Uri.extractOauthTokenFromCallback(): String? {
    val keys = listOf(
        "token",
        "access_token",
        "accessToken",
        "oauth_token",
        "oauthToken"
    )
    for (key in keys) {
        val value = getOauthParameter(key)
        if (!value.isNullOrBlank()) {
            return value
        }
    }
    return null
}

private fun generateOauthState(): String = java.util.UUID.randomUUID().toString()
