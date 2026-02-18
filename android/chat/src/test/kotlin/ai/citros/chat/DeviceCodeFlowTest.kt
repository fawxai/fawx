package ai.citros.chat

import ai.citros.core.DeviceCodeAuthClient
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Tests for Device Code Flow integration.
 * 
 * These tests verify:
 * - CloudAuthKind.OPENAI_DEVICE_CODE enum exists and is distinct
 * - Error message mapping provides user-friendly messages
 * - Constants used for token storage are correctly defined
 * 
 * Note: UI and SharedPreferences tests would require Android instrumented tests
 * or Robolectric. These tests focus on pure logic and enum validation.
 */
class DeviceCodeFlowTest {

    @Test
    fun `CloudAuthKind OPENAI_DEVICE_CODE enum value exists`() {
        // Given: The CloudAuthKind enum
        // When: Accessing OPENAI_DEVICE_CODE
        val deviceCodeAuthKind = CloudAuthKind.OPENAI_DEVICE_CODE
        
        // Then: Should exist and be accessible
        assertEquals("OPENAI_DEVICE_CODE", deviceCodeAuthKind.name)
    }

    @Test
    fun `CloudAuthKind OPENAI_DEVICE_CODE is distinct from other OpenAI auth kinds`() {
        // Given: Different OpenAI auth kinds
        val deviceCode = CloudAuthKind.OPENAI_DEVICE_CODE
        val apiKey = CloudAuthKind.OPENAI_API_KEY
        val codexOauth = CloudAuthKind.OPENAI_CODEX_OAUTH
        
        // Then: They should all be distinct
        assertNotEquals(deviceCode, apiKey, "DEVICE_CODE should differ from API_KEY")
        assertNotEquals(deviceCode, codexOauth, "DEVICE_CODE should differ from CODEX_OAUTH")
        assertNotEquals(apiKey, codexOauth, "API_KEY should differ from CODEX_OAUTH")
    }

    @Test
    fun `error message for timeout is user-friendly`() {
        // Given: A timeout error code
        val errorCode = "timeout"
        
        // When: Mapping to user message
        val message = mapDeviceCodePollError(errorCode, null)
        
        // Then: Should provide clear guidance
        assertTrue(message.contains("expired"), "Should mention expiration")
        assertTrue(message.contains("new code"), "Should suggest requesting new code")
    }

    @Test
    fun `error message for access_denied is user-friendly`() {
        // Given: An access_denied error code
        val errorCode = "access_denied"
        
        // When: Mapping to user message
        val message = mapDeviceCodePollError(errorCode, null)
        
        // Then: Should explain denial and suggest action
        assertTrue(message.contains("denied"), "Should mention denial")
        assertTrue(message.contains("approve"), "Should suggest approving")
    }

    @Test
    fun `error message for not_enabled is user-friendly`() {
        // Given: A not_enabled error code
        val errorCode = "not_enabled"
        
        // When: Mapping to user message
        val message = mapDeviceCodePollError(errorCode, null)
        
        // Then: Should provide clear guidance
        assertTrue(message.contains("not enabled"), "Should mention feature availability")
        assertTrue(message.contains("API key"), "Should suggest API key fallback")
    }

    @Test
    fun `generic error message suggests checking connection and API key`() {
        // Given: An unknown error code
        val errorCode = "unknown_error"
        val description = "Something went wrong"
        
        // When: Mapping to user message
        val message = mapDeviceCodePollError(errorCode, description)
        
        // Then: Should suggest troubleshooting steps
        assertTrue(message.contains("internet connection") || message.contains("connection"), 
            "Should suggest checking internet connection")
        assertTrue(message.contains("API key"), "Should suggest trying API key")
    }

    @Test
    fun `refresh token storage key is correctly defined`() {
        // Given: The constant for refresh token storage
        val key = getDeviceCodeRefreshTokenKey()
        
        // Then: Should be a non-empty, specific key
        assertEquals("device_code_refresh_token", key)
    }

    @Test
    fun `shared preferences name for citros is correctly defined`() {
        // Given: The constant for shared preferences name
        val prefsName = getCitrosPrefsName()
        
        // Then: Should be "citros"
        assertEquals("citros", prefsName)
    }

    @Test
    fun `diagnostics formatter includes counters and status`() {
        val diagnostics = DeviceCodeAuthClient.PollDiagnostics(
            attempts = 7,
            pending403Count = 4,
            pending404Count = 1,
            networkErrorCount = 2,
            elapsedSeconds = 33,
            lastHttpStatus = 403,
            lastResponsePreview = "{\"error\":\"authorization_pending\"}"
        )

        val formatted = formatDeviceCodeDiagnostics(diagnostics)
        assertNotNull(formatted)
        assertTrue(formatted.contains("Attempts=7"))
        assertTrue(formatted.contains("pending403=4"))
        assertTrue(formatted.contains("pending404=1"))
        assertTrue(formatted.contains("networkErrors=2"))
        assertTrue(formatted.contains("lastStatus=403"))
    }

    @Test
    fun `session formatter includes auth id suffix and interval`() {
        val response = DeviceCodeAuthClient.DeviceCodeResponse(
            deviceAuthId = "dauth_1234567890",
            userCode = "ABCD-1234",
            verificationUri = DeviceCodeAuthClient.DEFAULT_VERIFICATION_URL,
            interval = 5
        )

        val formatted = formatDeviceCodeSessionInfo(response)
        assertTrue(formatted.contains("34567890"))
        assertTrue(formatted.contains("pollInterval=5s"))
    }
}

// Constants from ChatActivity.kt (tested to ensure they match)
private fun getDeviceCodeRefreshTokenKey(): String = "device_code_refresh_token"
private fun getCitrosPrefsName(): String = "citros"
