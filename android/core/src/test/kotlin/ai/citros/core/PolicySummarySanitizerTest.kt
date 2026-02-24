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
    fun `sanitize trims and collapses whitespace`() {
        val sanitized = PolicySummarySanitizer.sanitize("  hello\n\nworld\t ")
        assertEquals("hello world", sanitized)
    }
}
