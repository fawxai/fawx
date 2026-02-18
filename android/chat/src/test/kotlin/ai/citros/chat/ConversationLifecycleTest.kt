package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for [ConversationLifecycle] idle timeout and daily reset logic.
 */
class ConversationLifecycleTest {

    // ========== Idle Timeout ==========

    @Test
    fun `idle timeout triggers when elapsed time exceeds threshold`() {
        val lastActivity = 1000L
        val now = lastActivity + 31 * 60 * 1000 // 31 minutes later
        val timeout = 30L * 60 * 1000 // 30 minute threshold

        assertTrue(ConversationLifecycle.shouldClearForIdleTimeout(lastActivity, now, timeout))
    }

    @Test
    fun `idle timeout does not trigger when within threshold`() {
        val lastActivity = 1000L
        val now = lastActivity + 20 * 60 * 1000 // 20 minutes later
        val timeout = 30L * 60 * 1000

        assertFalse(ConversationLifecycle.shouldClearForIdleTimeout(lastActivity, now, timeout))
    }

    @Test
    fun `idle timeout triggers at exact threshold boundary`() {
        val lastActivity = 1000L
        val timeout = 30L * 60 * 1000
        val now = lastActivity + timeout // exactly 30 minutes

        assertTrue(ConversationLifecycle.shouldClearForIdleTimeout(lastActivity, now, timeout))
    }

    @Test
    fun `idle timeout disabled when set to NEVER`() {
        val lastActivity = 1000L
        val now = lastActivity + 999 * 60 * 1000 // way over any threshold

        assertFalse(
            ConversationLifecycle.shouldClearForIdleTimeout(
                lastActivity, now, ConversationLifecycle.TIMEOUT_NEVER
            )
        )
    }

    @Test
    fun `idle timeout does not trigger with zero last activity`() {
        assertFalse(
            ConversationLifecycle.shouldClearForIdleTimeout(0L, System.currentTimeMillis(), 30 * 60 * 1000)
        )
    }

    @Test
    fun `idle timeout works with 15 minute threshold`() {
        val lastActivity = 1000L
        val timeout = 15L * 60 * 1000
        val now = lastActivity + 16 * 60 * 1000

        assertTrue(ConversationLifecycle.shouldClearForIdleTimeout(lastActivity, now, timeout))
    }

    @Test
    fun `idle timeout works with 1 hour threshold`() {
        val lastActivity = 1000L
        val timeout = 60L * 60 * 1000
        val now = lastActivity + 59 * 60 * 1000 // 59 minutes — under 1 hour

        assertFalse(ConversationLifecycle.shouldClearForIdleTimeout(lastActivity, now, timeout))
    }

    // ========== Daily Reset ==========

    @Test
    fun `daily reset triggers when date changes`() {
        assertTrue(ConversationLifecycle.shouldClearForNewDay("2026-02-15", "2026-02-16"))
    }

    @Test
    fun `daily reset does not trigger on same day`() {
        assertFalse(ConversationLifecycle.shouldClearForNewDay("2026-02-16", "2026-02-16"))
    }

    @Test
    fun `daily reset does not trigger when no previous date`() {
        assertFalse(ConversationLifecycle.shouldClearForNewDay(null, "2026-02-16"))
    }

    @Test
    fun `daily reset does not trigger when previous date is blank`() {
        assertFalse(ConversationLifecycle.shouldClearForNewDay("", "2026-02-16"))
    }

    @Test
    fun `daily reset triggers across month boundary`() {
        assertTrue(ConversationLifecycle.shouldClearForNewDay("2026-01-31", "2026-02-01"))
    }

    @Test
    fun `daily reset triggers across year boundary`() {
        assertTrue(ConversationLifecycle.shouldClearForNewDay("2025-12-31", "2026-01-01"))
    }

    // ========== Date Formatting ==========

    @Test
    fun `todayDateString formats correctly`() {
        // Feb 16, 2026 12:00 UTC (roughly)
        val cal = java.util.Calendar.getInstance().apply {
            set(2026, 1, 16, 12, 0, 0) // month is 0-based
        }
        val result = ConversationLifecycle.todayDateString(cal.timeInMillis)
        assertEquals("2026-02-16", result)
    }

    // ========== Timeout Options ==========

    @Test
    fun `timeout options contains expected values`() {
        assertEquals(4, ConversationLifecycle.TIMEOUT_OPTIONS.size)
        assertEquals("Never", ConversationLifecycle.TIMEOUT_OPTIONS.last().first)
        assertEquals(ConversationLifecycle.TIMEOUT_NEVER, ConversationLifecycle.TIMEOUT_OPTIONS.last().second)
    }

    @Test
    fun `labelForTimeout returns correct label`() {
        assertEquals("30 minutes", ConversationLifecycle.labelForTimeout(30L * 60 * 1000))
        assertEquals("Never", ConversationLifecycle.labelForTimeout(ConversationLifecycle.TIMEOUT_NEVER))
        assertEquals("1 hour", ConversationLifecycle.labelForTimeout(60L * 60 * 1000))
    }

    @Test
    fun `labelForTimeout returns default for unknown value`() {
        assertEquals("30 minutes", ConversationLifecycle.labelForTimeout(99999L))
    }
}
