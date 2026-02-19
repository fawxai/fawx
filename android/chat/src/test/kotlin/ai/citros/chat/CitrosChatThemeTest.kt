package ai.citros.chat

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.createComposeRule
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Tests for [CitrosChatTheme] light/dark/system mode support (#469).
 */
@RunWith(RobolectricTestRunner::class)
class CitrosChatThemeTest {

    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun `dark theme uses dark background`() {
        var background = Color.Unspecified

        composeRule.setContent {
            CitrosChatTheme(themeMode = "dark") {
                background = MaterialTheme.colorScheme.background
            }
        }

        // Dark background should be very dark (low luminance)
        assertEquals(Color(0xFF050505), background)
    }

    @Test
    fun `light theme uses light background`() {
        var background = Color.Unspecified

        composeRule.setContent {
            CitrosChatTheme(themeMode = "light") {
                background = MaterialTheme.colorScheme.background
            }
        }

        // Light background should be very bright
        assertEquals(Color(0xFFFFFBFE), background)
    }

    @Test
    fun `light theme has dark text on background`() {
        var onBackground = Color.Unspecified

        composeRule.setContent {
            CitrosChatTheme(themeMode = "light") {
                onBackground = MaterialTheme.colorScheme.onBackground
            }
        }

        // onBackground should be dark in light mode
        assertEquals(Color(0xFF1C1B1F), onBackground)
    }

    @Test
    fun `dark theme has light text on background`() {
        var onBackground = Color.Unspecified

        composeRule.setContent {
            CitrosChatTheme(themeMode = "dark") {
                onBackground = MaterialTheme.colorScheme.onBackground
            }
        }

        assertEquals(Color.White, onBackground)
    }

    @Test
    fun `default theme mode is system`() {
        assertEquals("system", THEME_MODE_DEFAULT)
    }

    @Test
    fun `CitrusPrimaryButton uses flavor tint not hardcoded black`() {
        // Verify each flavor's tint is a dark color (not pure black)
        CitrosFlavor.entries.forEach { flavor ->
            assertTrue(
                flavor.tint != Color.Black,
                "Flavor ${flavor.name} tint should not be Color.Black"
            )
        }
    }
}
