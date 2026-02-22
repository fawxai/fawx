package ai.citros.core

import android.graphics.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ScreenContentTest {

    @Test
    fun `privacy mode formatting returns hidden message`() {
        val content = ScreenContent(
            elements = emptyList(),
            packageName = "com.chase.sig.android",
            privacyMode = true
        )

        assertEquals(
            "SCREEN: [Privacy mode — screen content hidden for private_app. Ask the user for guidance if needed.]",
            content.toToolResult()
        )
    }

    @Test
    fun `normal formatting remains unchanged when privacy mode is false`() {
        val content = ScreenContent(
            elements = listOf(
                ScreenElement(
                    id = 0,
                    text = "Pay",
                    contentDescription = null,
                    className = "Button",
                    isClickable = true,
                    isEditable = false,
                    bounds = Rect(0, 0, 100, 100),
                    depth = 0
                )
            ),
            packageName = "com.example.wallet",
            privacyMode = false
        )

        val result = content.toToolResult()
        assertTrue(result.startsWith("SCREEN:"))
        assertTrue(result.contains("App: com.example.wallet"))
        assertTrue(result.contains("[0] \"Pay\" [click]"))
    }

    @Test
    fun `privacy message redacts package name`() {
        val pkg = "com.example.private"
        val content = ScreenContent(
            elements = emptyList(),
            packageName = pkg,
            privacyMode = true
        )

        val result = content.toToolResult()
        assertFalse(result.contains(pkg))
        assertTrue(result.contains("private_app"))
    }

    @Test
    fun `toToolResult keeps read_screen prefix contract`() {
        val content = ScreenContent(
            elements = emptyList(),
            packageName = "com.example.wallet",
            privacyMode = false
        )

        val readScreenResult = "Screen refreshed:\n${content.toToolResult()}"
        assertTrue(readScreenResult.startsWith("Screen refreshed:\nSCREEN:\nApp:"))
    }
}
