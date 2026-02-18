package ai.citros.core

import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * Tests for NotificationHelper.
 *
 * Note: Full notification interaction tests require a real NotificationListenerService
 * which cannot be unit-tested. These tests cover parsing, formatting, and detach behavior.
 */
class NotificationHelperTest {

    @Before
    fun setUp() {
        NotificationHelper.detach()
    }

    @After
    fun tearDown() {
        NotificationHelper.detach()
    }

    // ========== Attach/Detach Tests (#340) ==========

    @Test
    fun `isAttached returns false when detached`() {
        assertFalse(NotificationHelper.isAttached())
    }

    @Test
    fun `getActiveNotifications returns empty when detached`() {
        val result = NotificationHelper.getActiveNotifications()
        assertTrue(result.isEmpty())
    }

    @Test
    fun `tapNotification returns false when detached`() {
        assertFalse(NotificationHelper.tapNotification("some_key"))
    }

    @Test
    fun `dismissNotification returns false when detached`() {
        assertFalse(NotificationHelper.dismissNotification("some_key"))
    }

    @Test
    fun `replyToNotification returns false when detached`() {
        assertFalse(NotificationHelper.replyToNotification("some_key", "test"))
    }

    // ========== Formatting Tests (#340) ==========

    @Test
    fun `formatForPrompt returns 'No notifications' for empty list`() {
        val result = NotificationHelper.formatForPrompt(emptyList())
        assertEquals("No notifications", result)
    }

    @Test
    fun `formatForPrompt formats single notification with key`() {
        val notifications = listOf(
            ParsedNotification(
                key = "0|com.example.app|123|null|10001",
                packageName = "com.example.app",
                appName = "Example App",
                title = "New Message",
                text = "Hello there!",
                subText = null,
                postTime = 1707700000000,
                isOngoing = false,
                actions = emptyList()
            )
        )

        val result = NotificationHelper.formatForPrompt(notifications)
        assertTrue(result.contains("[0|com.example.app|123|null|10001]"))
        assertTrue(result.contains("Example App: New Message"))
        assertTrue(result.contains("Hello there!"))
        assertTrue(result.contains("Notifications (1):"))
    }

    @Test
    fun `formatForPrompt shows actions with reply marker`() {
        val notifications = listOf(
            ParsedNotification(
                key = "msg_key",
                packageName = "com.example.msg",
                appName = "Messages",
                title = "John",
                text = "Hey!",
                subText = null,
                postTime = 1707700000000,
                isOngoing = false,
                actions = listOf(
                    NotificationAction(0, "Reply", hasRemoteInput = true),
                    NotificationAction(1, "Mark read", hasRemoteInput = false)
                )
            )
        )

        val result = NotificationHelper.formatForPrompt(notifications)
        assertTrue(result.contains("Reply [reply]"))
        assertTrue(result.contains("Mark read"))
        assertFalse(result.contains("Mark read [reply]"))
    }

    @Test
    fun `formatForPrompt handles null title`() {
        val notifications = listOf(
            ParsedNotification(
                key = "key1",
                packageName = "com.example.app",
                appName = "App",
                title = null,
                text = "Some text",
                subText = null,
                postTime = 1707700000000,
                isOngoing = false,
                actions = emptyList()
            )
        )

        val result = NotificationHelper.formatForPrompt(notifications)
        assertTrue(result.contains("(no title)"))
    }

    @Test
    fun `formatForPrompt truncates long text at 100 chars`() {
        val longText = "x".repeat(200)
        val notifications = listOf(
            ParsedNotification(
                key = "key1",
                packageName = "com.example.app",
                appName = "App",
                title = "Title",
                text = longText,
                subText = null,
                postTime = 1707700000000,
                isOngoing = false,
                actions = emptyList()
            )
        )

        val result = NotificationHelper.formatForPrompt(notifications)
        assertFalse(result.contains(longText))
        assertTrue(result.contains("x".repeat(100)))
    }

    @Test
    fun `formatForPrompt handles multiple notifications`() {
        val notifications = listOf(
            ParsedNotification(
                key = "k1", packageName = "com.a", appName = "App A",
                title = "First", text = null, subText = null,
                postTime = 2000, isOngoing = false, actions = emptyList()
            ),
            ParsedNotification(
                key = "k2", packageName = "com.b", appName = "App B",
                title = "Second", text = null, subText = null,
                postTime = 1000, isOngoing = false, actions = emptyList()
            )
        )

        val result = NotificationHelper.formatForPrompt(notifications)
        assertTrue(result.contains("Notifications (2):"))
        assertTrue(result.contains("[k1] App A: First"))
        assertTrue(result.contains("[k2] App B: Second"))
    }

    // ========== Data Class Tests (#340) ==========

    @Test
    fun `ParsedNotification data class properties`() {
        val action = NotificationAction(0, "Reply", true)
        val notification = ParsedNotification(
            key = "test_key",
            packageName = "com.test",
            appName = "Test",
            title = "Title",
            text = "Body",
            subText = "Sub",
            postTime = 12345L,
            isOngoing = false,
            actions = listOf(action)
        )

        assertEquals("test_key", notification.key)
        assertEquals("com.test", notification.packageName)
        assertEquals("Test", notification.appName)
        assertEquals("Title", notification.title)
        assertEquals("Body", notification.text)
        assertEquals("Sub", notification.subText)
        assertEquals(12345L, notification.postTime)
        assertFalse(notification.isOngoing)
        assertEquals(1, notification.actions.size)
        assertTrue(notification.actions[0].hasRemoteInput)
    }

    @Test
    fun `NotificationAction data class properties`() {
        val action = NotificationAction(2, "Share", false)
        assertEquals(2, action.index)
        assertEquals("Share", action.title)
        assertFalse(action.hasRemoteInput)
    }

    @Test
    fun `formatForPrompt handles null text`() {
        val notifications = listOf(
            ParsedNotification(
                key = "key1",
                packageName = "com.app",
                appName = "App",
                title = "Title",
                text = null,
                subText = null,
                postTime = 1000,
                isOngoing = false,
                actions = emptyList()
            )
        )
        val result = NotificationHelper.formatForPrompt(notifications)
        assertTrue(result.contains("[key1] App: Title"))
        // Should not have an indented text line
        val lines = result.lines().filter { it.startsWith("    ") }
        assertTrue(lines.isEmpty())
    }
}
