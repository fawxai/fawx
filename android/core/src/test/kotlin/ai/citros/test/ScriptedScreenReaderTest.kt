package ai.citros.test

import ai.citros.core.ScreenContent
import ai.citros.core.ScreenElement
import android.graphics.Rect
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class ScriptedScreenReaderTest {

    @Test
    fun `constructor rejects empty screens`() {
        assertFailsWith<IllegalArgumentException> {
            ScriptedScreenReader(emptyList())
        }
    }

    @Test
    fun `nextScreen advances and clamps at last screen then reset restarts`() {
        val first = screen("com.android.settings", "Settings")
        val second = screen("com.google.android.apps.maps", "Maps")
        val reader = ScriptedScreenReader(listOf(first, second))

        assertEquals(first, reader.nextScreen())
        assertEquals(second, reader.nextScreen())
        assertEquals(second, reader.nextScreen(), "reader should clamp to last screen after inputs are exhausted")

        reader.reset()
        assertEquals(first, reader.nextScreen())
    }

    private fun screen(packageName: String, text: String): ScreenContent = ScreenContent(
        elements = listOf(
            ScreenElement(
                id = 1,
                text = text,
                contentDescription = null,
                className = "android.widget.TextView",
                isClickable = false,
                isEditable = false,
                bounds = Rect(0, 0, 100, 100)
            )
        ),
        packageName = packageName
    )
}
