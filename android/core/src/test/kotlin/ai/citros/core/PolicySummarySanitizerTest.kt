package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PolicySummarySanitizerTest {

    @Test
    fun `sanitize redacts card otp email and normalizes url`() {
        val raw = "Card 4111 1111 1111 1111 otp 123456 email a@b.com link https://Example.com/pay?id=1"
        val sanitized = PolicySummarySanitizer.sanitize(raw).orEmpty()

        assertFalse(sanitized.contains("4111"))
        assertFalse(sanitized.contains("123456"))
        assertFalse(sanitized.contains("a@b.com"))
        assertTrue(sanitized.contains("URL_HOST:example.com"))
    }

    @Test
    fun `sanitize redacts alphanumeric otp with at least one digit`() {
        val sanitized = PolicySummarySanitizer.sanitize("Code A1B2C3")
        assertEquals("Code [REDACTED]", sanitized)
    }

    @Test
    fun `sanitize redacts lowercase alphanumeric otp with at least one digit`() {
        val sanitized = PolicySummarySanitizer.sanitize("Code ab12cd")
        assertEquals("Code [REDACTED]", sanitized)
    }

    @Test
    fun `sanitize redacts mixed-case alphanumeric otp with at least one digit`() {
        val sanitized = PolicySummarySanitizer.sanitize("Code aB12Cd")
        assertEquals("Code [REDACTED]", sanitized)
    }

    @Test
    fun `sanitize does not redact plain words without digits`() {
        val sanitized = PolicySummarySanitizer.sanitize("Your greeting is HELLO")
        assertEquals("Your greeting is HELLO", sanitized)
    }

    @Test
    fun `sanitize redacts phrase prefix otp pattern`() {
        val sanitized = PolicySummarySanitizer.sanitize("Your verification code is ZX9K2Q")
        assertEquals("Your verification code is [REDACTED]", sanitized)
    }

    @Test
    fun `sanitize redacts phrase suffix otp pattern`() {
        val sanitized = PolicySummarySanitizer.sanitize("Q8W7E6 is your otp")
        assertEquals("[REDACTED] is your otp", sanitized)
    }

    @Test
    fun `sanitize redacts dash separated otp`() {
        val sanitized = PolicySummarySanitizer.sanitize("Use code 1234-5678 now")
        assertEquals("Use code [REDACTED] now", sanitized)
    }

    @Test
    fun `sanitize keeps existing regressions for numeric otp and email`() {
        val sanitized = PolicySummarySanitizer.sanitize("OTP 876543 email test@example.com")
        assertEquals("OTP [REDACTED] email [REDACTED]", sanitized)
    }

    @Test
    fun `sanitize trims and collapses whitespace`() {
        val sanitized = PolicySummarySanitizer.sanitize("  hello\n\nworld\t ")
        assertEquals("hello world", sanitized)
    }
}
