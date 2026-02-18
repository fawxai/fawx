package ai.citros.core

import android.graphics.Bitmap
import android.os.Build
import android.util.Base64
import android.view.accessibility.AccessibilityNodeInfo
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Tests for ScreenReader screenshot utilities.
 *
 * Note: Full screenshot capture (takeScreenshot) requires a real AccessibilityService
 * and cannot be unit-tested. These tests cover the image processing utilities:
 * scaling, encoding, and edge cases.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenReaderTest {

    // ========== Screenshot Utility Tests (#338) ==========

    @Test
    fun `scaleBitmap scales down to target width maintaining aspect ratio`() {
        val bitmap = Bitmap.createBitmap(1080, 2400, Bitmap.Config.ARGB_8888)
        val scaled = ScreenReader.scaleBitmap(bitmap, 720)

        assertEquals(720, scaled.width)
        // Aspect ratio: 2400/1080 = 2.222... -> 720 * 2.222 = 1600
        assertEquals(1600, scaled.height)

        bitmap.recycle()
        scaled.recycle()
    }

    @Test
    fun `scaleBitmap returns original when already at target width`() {
        val bitmap = Bitmap.createBitmap(720, 1280, Bitmap.Config.ARGB_8888)
        val scaled = ScreenReader.scaleBitmap(bitmap, 720)

        // Should return the same instance, not a copy
        assertSame(bitmap, scaled)

        bitmap.recycle()
    }

    @Test
    fun `scaleBitmap returns original when below target width`() {
        val bitmap = Bitmap.createBitmap(480, 800, Bitmap.Config.ARGB_8888)
        val scaled = ScreenReader.scaleBitmap(bitmap, 720)

        assertSame(bitmap, scaled)

        bitmap.recycle()
    }

    @Test
    fun `encodeBitmapToBase64Png returns valid base64 string`() {
        val bitmap = Bitmap.createBitmap(100, 100, Bitmap.Config.ARGB_8888)
        val base64 = ScreenReader.encodeBitmapToBase64Png(bitmap)

        // Should be non-empty
        assertTrue(base64.isNotEmpty())

        // Should be valid base64 (decodable)
        val decoded = Base64.decode(base64, Base64.NO_WRAP)
        assertTrue(decoded.isNotEmpty())

        // PNG magic bytes: 137 80 78 71 13 10 26 10
        assertEquals(0x89.toByte(), decoded[0])
        assertEquals(0x50.toByte(), decoded[1]) // 'P'
        assertEquals(0x4E.toByte(), decoded[2]) // 'N'
        assertEquals(0x47.toByte(), decoded[3]) // 'G'

        bitmap.recycle()
    }

    @Test
    fun `encodeBitmapToBase64Png has no line wrapping`() {
        val bitmap = Bitmap.createBitmap(100, 100, Bitmap.Config.ARGB_8888)
        val base64 = ScreenReader.encodeBitmapToBase64Png(bitmap)

        // NO_WRAP means no newlines
        assertFalse(base64.contains("\n"))
        assertFalse(base64.contains("\r"))

        bitmap.recycle()
    }

    @Test
    fun `takeScreenshot returns null when not attached`() {
        // ScreenReader.service is null when not attached
        ScreenReader.detach()
        val result = kotlinx.coroutines.runBlocking { ScreenReader.takeScreenshot() }
        assertNull(result)
    }

    @Test
    fun `isAttached returns false when detached`() {
        ScreenReader.detach()
        assertFalse(ScreenReader.isAttached())
    }

    @Test
    fun `scaleBitmap handles square bitmaps`() {
        val bitmap = Bitmap.createBitmap(1080, 1080, Bitmap.Config.ARGB_8888)
        val scaled = ScreenReader.scaleBitmap(bitmap, 720)

        assertEquals(720, scaled.width)
        assertEquals(720, scaled.height)

        bitmap.recycle()
        scaled.recycle()
    }

    // ========== API < 30 Guard Tests (#356) ==========

    @Test
    @Config(sdk = [Build.VERSION_CODES.Q]) // API 29
    fun `takeScreenshot returns null on API below 30`() {
        // Even with a mock service attached, API < 30 should short-circuit
        // We can't attach a real AccessibilityService in Robolectric, but the
        // API check happens before the service is used, so detached is fine
        // for testing the guard.
        ScreenReader.detach()
        val result = kotlinx.coroutines.runBlocking { ScreenReader.takeScreenshot() }
        assertNull("takeScreenshot should return null on API 29", result)
    }

    @Test
    @Config(sdk = [Build.VERSION_CODES.P]) // API 28 (minSdk)
    fun `takeScreenshot returns null on API 28`() {
        ScreenReader.detach()
        val result = kotlinx.coroutines.runBlocking { ScreenReader.takeScreenshot() }
        assertNull("takeScreenshot should return null on API 28", result)
    }

    @Test
    fun `scaleBitmap handles landscape bitmaps`() {
        val bitmap = Bitmap.createBitmap(2400, 1080, Bitmap.Config.ARGB_8888)
        val scaled = ScreenReader.scaleBitmap(bitmap, 720)

        assertEquals(720, scaled.width)
        // Aspect ratio: 1080/2400 = 0.45 -> 720 * 0.45 = 324
        assertEquals(324, scaled.height)

        bitmap.recycle()
        scaled.recycle()
    }

    // --- waitForAttachment tests ---

    // --- waitForAttachment tests ---

    /**
     * Simulate attachment by calling attach() with a Robolectric-provided service.
     * Uses buildService() which creates the service without needing manifest registration.
     */
    private fun simulateAttach() {
        val controller = org.robolectric.Robolectric.buildService(StubAccessibilityService::class.java)
        val service = controller.create().get()
        ScreenReader.attach(service)
    }

    /** Minimal concrete subclass for Robolectric instantiation. */
    class StubAccessibilityService : android.accessibilityservice.AccessibilityService() {
        override fun onAccessibilityEvent(event: android.view.accessibility.AccessibilityEvent?) {}
        override fun onInterrupt() {}
    }

    @Test
    fun `waitForAttachment returns true immediately when already attached`() = runBlocking {
        try {
            simulateAttach()
            assertTrue(ScreenReader.isAttached())
            assertTrue(ScreenReader.waitForAttachment(timeoutMs = 100))
        } finally {
            ScreenReader.detach()
        }
    }

    @Test
    fun `waitForAttachment returns false after timeout when detached`() = runBlocking {
        ScreenReader.detach()
        val start = System.currentTimeMillis()
        val result = ScreenReader.waitForAttachment(timeoutMs = 300, pollIntervalMs = 50)
        val elapsed = System.currentTimeMillis() - start
        assertFalse(result)
        assertTrue("Should wait close to 300ms but took ${elapsed}ms", elapsed in 250..450)
    }

    @Test
    fun `waitForAttachment succeeds when service attaches mid-wait`() = runBlocking {
        ScreenReader.detach()
        try {
            launch {
                delay(150)
                simulateAttach()
            }
            val result = ScreenReader.waitForAttachment(timeoutMs = 2000, pollIntervalMs = 50)
            assertTrue(result)
        } finally {
            ScreenReader.detach()
        }
    }

    // ========== Window Filtering Tests (#431) ==========

    /** Helper to create an AccessibilityNodeInfo with a package name. */
    private fun createNodeWithPackage(pkg: String): AccessibilityNodeInfo {
        val node = AccessibilityNodeInfo.obtain()
        node.packageName = pkg
        return node
    }

    /**
     * Helper that runs [pickBestWindow] and ensures all created nodes are recycled
     * even if assertions fail. The returned result (if non-null) is recycled after [block].
     */
    private fun runPickBestWindow(
        candidates: List<ScreenReader.WindowCandidate>,
        selfPackage: String = "ai.citros.chat",
        block: (AccessibilityNodeInfo?) -> Unit
    ) {
        val allRoots = candidates.mapNotNull { it.root }
        try {
            val result = ScreenReader.pickBestWindow(candidates, selfPackage)
            try {
                block(result)
            } finally {
                result?.recycle()
            }
        } catch (e: Throwable) {
            // Recycle any nodes that pickBestWindow didn't get to process
            allRoots.forEach { try { it.recycle() } catch (_: Exception) {} }
            throw e
        }
    }

    @Test
    fun `pickBestWindow filters out self package`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("ai.citros.chat", isActive = true, isFocused = true, root = createNodeWithPackage("ai.citros.chat")),
                ScreenReader.WindowCandidate("com.google.calendar", isActive = false, isFocused = false, root = createNodeWithPackage("com.google.calendar"))
            )
        ) { result ->
            assertNotNull(result)
            assertEquals("com.google.calendar", result!!.packageName?.toString())
        }
    }

    @Test
    fun `pickBestWindow prefers active window`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("com.app.one", isActive = false, isFocused = false, root = createNodeWithPackage("com.app.one")),
                ScreenReader.WindowCandidate("com.app.two", isActive = true, isFocused = false, root = createNodeWithPackage("com.app.two"))
            )
        ) { result ->
            assertNotNull(result)
            assertEquals("com.app.two", result!!.packageName?.toString())
        }
    }

    @Test
    fun `pickBestWindow prefers focused window`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("com.app.one", isActive = false, isFocused = false, root = createNodeWithPackage("com.app.one")),
                ScreenReader.WindowCandidate("com.app.two", isActive = false, isFocused = true, root = createNodeWithPackage("com.app.two"))
            )
        ) { result ->
            assertNotNull(result)
            assertEquals("com.app.two", result!!.packageName?.toString())
        }
    }

    @Test
    fun `pickBestWindow returns null for empty candidates`() {
        runPickBestWindow(emptyList()) { result ->
            assertNull(result)
        }
    }

    @Test
    fun `pickBestWindow returns null when all candidates are self`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("ai.citros.chat", isActive = true, isFocused = true, root = createNodeWithPackage("ai.citros.chat"))
            )
        ) { result ->
            assertNull(result)
        }
    }

    @Test
    fun `pickBestWindow handles null root in candidate`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("com.app.null", isActive = true, isFocused = true, root = null),
                ScreenReader.WindowCandidate("com.app.one", isActive = false, isFocused = false, root = createNodeWithPackage("com.app.one"))
            )
        ) { result ->
            assertNotNull(result)
            assertEquals("com.app.one", result!!.packageName?.toString())
        }
    }

    @Test
    fun `pickBestWindow returns first when no window is active`() {
        runPickBestWindow(
            listOf(
                ScreenReader.WindowCandidate("com.app.one", isActive = false, isFocused = false, root = createNodeWithPackage("com.app.one")),
                ScreenReader.WindowCandidate("com.app.two", isActive = false, isFocused = false, root = createNodeWithPackage("com.app.two"))
            )
        ) { result ->
            assertNotNull(result)
            assertEquals("com.app.one", result!!.packageName?.toString())
        }
    }

    // ========== Screenshot Overlay Hook Tests (#436) ==========

    @Test
    fun `takeScreenshot does not call hooks when no service attached`() = runBlocking {
        ScreenReader.detach() // No service — takeScreenshot returns null early
        val callOrder = mutableListOf<String>()

        ScreenReader.screenshotOverlayHook = { callOrder.add("hide") }
        ScreenReader.screenshotOverlayRestoreHook = { callOrder.add("restore") }

        try {
            val result = ScreenReader.takeScreenshot()
            assertNull(result) // No service attached, returns null
            // Hooks should not be called since we bail before them (no service)
            assertTrue("Hooks should not fire without service", callOrder.isEmpty())
        } finally {
            ScreenReader.screenshotOverlayHook = null
            ScreenReader.screenshotOverlayRestoreHook = null
        }
    }

    @Test
    @Config(sdk = [30])
    fun `takeScreenshot calls overlay hooks when service is attached`() = runBlocking {
        simulateAttach()
        val callOrder = mutableListOf<String>()

        ScreenReader.screenshotOverlayHook = { callOrder.add("hide") }
        ScreenReader.screenshotOverlayRestoreHook = { callOrder.add("restore") }

        try {
            // captureScreen will fail in Robolectric (no real display), but
            // hooks should still fire: hide before capture, restore in finally
            val result = ScreenReader.takeScreenshot()
            assertNull(result) // Capture fails in test environment
            assertEquals(
                "Hooks should fire in order: hide then restore",
                listOf("hide", "restore"),
                callOrder
            )
        } finally {
            ScreenReader.screenshotOverlayHook = null
            ScreenReader.screenshotOverlayRestoreHook = null
            ScreenReader.detach()
        }
    }

    @Test
    @Config(sdk = [30])
    fun `takeScreenshot calls restore hook even when capture throws`() = runBlocking {
        simulateAttach()
        val callOrder = mutableListOf<String>()

        ScreenReader.screenshotOverlayHook = { callOrder.add("hide") }
        ScreenReader.screenshotOverlayRestoreHook = { callOrder.add("restore") }

        try {
            // Even if captureScreen throws internally, the finally block
            // should always call the restore hook
            val result = ScreenReader.takeScreenshot()
            assertNull(result)
            assertTrue("Restore hook must always be called", callOrder.contains("restore"))
        } finally {
            ScreenReader.screenshotOverlayHook = null
            ScreenReader.screenshotOverlayRestoreHook = null
            ScreenReader.detach()
        }
    }

    @Test
    fun `screenshot hooks are cleared properly`() {
        ScreenReader.screenshotOverlayHook = { }
        ScreenReader.screenshotOverlayRestoreHook = { }

        assertNotNull(ScreenReader.screenshotOverlayHook)
        assertNotNull(ScreenReader.screenshotOverlayRestoreHook)

        ScreenReader.screenshotOverlayHook = null
        ScreenReader.screenshotOverlayRestoreHook = null

        assertNull(ScreenReader.screenshotOverlayHook)
        assertNull(ScreenReader.screenshotOverlayRestoreHook)
    }

    @Test
    fun `SCREENSHOT_OVERLAY_HIDE_DELAY_MS is reasonable`() {
        // Sanity check: delay should be between 50ms and 1000ms
        assertTrue(ScreenReader.SCREENSHOT_OVERLAY_HIDE_DELAY_MS in 50L..1000L)
    }

    @Test
    fun `getScreenContent returns empty when not attached`() {
        ScreenReader.detach()
        val content = ScreenReader.getScreenContent()
        assertTrue(content.elements.isEmpty())
        assertNull(content.packageName)
    }

    // ========== Self-Read Fallback Rejection Tests (#431 reopened) ==========

    @Test
    fun `toPromptText returns navigation hint when no target app visible`() {
        val content = ScreenContent(elements = emptyList(), packageName = null)
        val text = content.toPromptText()
        assertTrue(text.contains("No target app is visible"))
        assertTrue(text.contains("open_app"))
    }

    @Test
    fun `getScreenContent returns empty with navigation hint when only self window exists`() {
        // Integration test: verify full path from getScreenContent() → self-rejection → navigation hint
        simulateAttach()
        try {
            // With a real Robolectric service, rootInActiveWindow returns null by default
            // and getWindows() returns empty — simulating "no non-self app visible"
            val content = ScreenReader.getScreenContent()
            // Should get empty content (no target app)
            assertTrue("Should have no elements", content.elements.isEmpty())
            assertNull("Package should be null", content.packageName)
            // toPromptText should return navigation hint
            val prompt = content.toPromptText()
            assertTrue("Should contain navigation hint", prompt.contains("No target app is visible"))
            assertTrue("Should mention open_app", prompt.contains("open_app"))
        } finally {
            ScreenReader.detach()
        }
    }

    @Test
    fun `toPromptText returns normal content when app is visible`() {
        val content = ScreenContent(
            elements = listOf(
                ScreenElement(0, "Hello", null, "TextView", false, false, android.graphics.Rect(0, 0, 100, 50))
            ),
            packageName = "com.example.app"
        )
        val text = content.toPromptText()
        assertTrue(text.contains("App: com.example.app"))
        assertTrue(text.contains("Hello"))
    }

    @Test
    fun `waitForAttachment succeeds when service attaches just before deadline`() = runBlocking {
        ScreenReader.detach()
        try {
            launch {
                delay(250) // Attach at t=250ms, just before 300ms deadline
                simulateAttach()
            }
            val result = ScreenReader.waitForAttachment(timeoutMs = 300, pollIntervalMs = 50)
            assertTrue("Should return true when service attaches before deadline", result)
        } finally {
            ScreenReader.detach()
        }
    }

    @Test
    fun `toPromptText respects custom elementCap`() {
        val elements = (0..99).map {
            ScreenElement(it, "Button $it", null, null, true, false, android.graphics.Rect(), depth = 0)
        }
        val content = ScreenContent(elements, "com.test")

        val text = content.toPromptText(elementCap = 10)

        assertTrue(text.contains("(90 more elements hidden)"))
        // Only 10 elements should appear
        val lines = text.lines().filter { it.contains("[click]") }
        assertEquals(10, lines.size)
    }

    @Test
    fun `toPromptText clamps elementCap to valid range`() {
        val elements = (0..4).map {
            ScreenElement(it, "Item $it", null, null, false, false, android.graphics.Rect(), depth = 0)
        }
        val content = ScreenContent(elements, "com.test")

        // Zero should be clamped to 1
        val textMin = content.toPromptText(elementCap = 0)
        val linesMin = textMin.lines().filter { it.trimStart().startsWith("[") }
        assertEquals(1, linesMin.size)

        // Negative should also be clamped to 1
        val textNeg = content.toPromptText(elementCap = -5)
        val linesNeg = textNeg.lines().filter { it.trimStart().startsWith("[") }
        assertEquals(1, linesNeg.size)
    }

    @Test
    fun `toPromptText includes depth indentation`() {
        val elements = listOf(
            ScreenElement(0, "Root", null, null, true, false, android.graphics.Rect(), depth = 0),
            ScreenElement(1, "Child", null, null, true, false, android.graphics.Rect(), depth = 1),
            ScreenElement(2, "Grandchild", null, null, true, false, android.graphics.Rect(), depth = 2),
            ScreenElement(3, "Deep", null, null, true, false, android.graphics.Rect(), depth = 5)
        )
        val content = ScreenContent(elements, "com.test")
        val text = content.toPromptText()
        val lines = text.lines()

        // Root at depth 0: no indent
        assertTrue(lines.any { it.startsWith("[0]") })
        // Child at depth 1: 2 spaces
        assertTrue(lines.any { it.startsWith("  [1]") })
        // Grandchild at depth 2: 4 spaces
        assertTrue(lines.any { it.startsWith("    [2]") })
        // Deep at depth 5: clamped to 4 levels = 8 spaces
        assertTrue(lines.any { it.startsWith("        [3]") })
    }
}
