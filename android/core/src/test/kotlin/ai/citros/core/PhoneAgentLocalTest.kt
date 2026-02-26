package ai.citros.core

import android.graphics.Rect
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import org.junit.Test

class PhoneAgentLocalTest {
    private class StubLocalClient : LocalLLMClient() {
        override suspend fun chat(message: String): Result<String> = Result.success("ok")
    }

    private val screen = ScreenContent(
        packageName = "com.example.app",
        elements = listOf(
            ScreenElement(
                id = 7,
                text = "Settings",
                contentDescription = null,
                className = "android.widget.TextView",
                isClickable = true,
                isEditable = false,
                bounds = Rect(0, 0, 10, 10)
            )
        )
    )

    @Test
    fun `executeAction click_text reports cause-accurate outcomes`() {
        val cases = listOf(
            ScreenReader.ElementActionResult.Success to "Clicked \"Settings\"",
            ScreenReader.ElementActionResult.PrivacyBlocked to
                "Failed: click_text: blocked by privacy mode for private_app",
            ScreenReader.ElementActionResult.ServiceUnavailable to
                "Failed: click_text: accessibility service unavailable",
            ScreenReader.ElementActionResult.GestureDispatchFailed to
                "Failed: click_text: gesture dispatch failed",
            ScreenReader.ElementActionResult.ElementNotFound to
                "Could not find element with text \"Settings\""
        )

        for ((actionResult, expected) in cases) {
            val agent = PhoneAgentLocal(
                llmClient = StubLocalClient(),
                clickElement = { actionResult }
            )
            val result = agent.executeAction(PhoneAction.ClickText("Settings"), screen)
            assertEquals(expected, result)
            assertFalse(result.contains("com.bank.app"))
        }
    }

    @Test
    fun `executeAction click_text returns privacy blocked when screen content is privacy mode`() {
        val agent = PhoneAgentLocal(
            llmClient = StubLocalClient(),
            clickElement = { ScreenReader.ElementActionResult.Success }
        )
        val privacyScreen = ScreenContent(
            packageName = PrivacyRedaction.APP_PLACEHOLDER,
            elements = emptyList(),
            privacyMode = true
        )

        val result = agent.executeAction(PhoneAction.ClickText("Settings"), privacyScreen)

        assertEquals("Failed: click_text: blocked by privacy mode for private_app", result)
    }

    @Test
    fun `executeAction click reports cause-accurate outcomes`() {
        val cases = listOf(
            ScreenReader.ElementActionResult.Success to "Clicked element 7",
            ScreenReader.ElementActionResult.PrivacyBlocked to
                "Failed: click: blocked by privacy mode for private_app",
            ScreenReader.ElementActionResult.ServiceUnavailable to
                "Failed: click: accessibility service unavailable",
            ScreenReader.ElementActionResult.GestureDispatchFailed to
                "Failed: click: gesture dispatch failed",
            ScreenReader.ElementActionResult.ElementNotFound to
                "Failed to click element 7"
        )

        for ((actionResult, expected) in cases) {
            val agent = PhoneAgentLocal(
                llmClient = StubLocalClient(),
                clickElement = { actionResult }
            )
            val result = agent.executeAction(PhoneAction.Click(7), screen)
            assertEquals(expected, result)
            assertFalse(result.contains("com.bank.app"))
        }
    }

    @Test
    fun `executeAction click_text returns not-found when no matching element`() {
        val agent = PhoneAgentLocal(
            llmClient = StubLocalClient(),
            clickElement = { ScreenReader.ElementActionResult.Success }
        )
        val result = agent.executeAction(
            PhoneAction.ClickText("Missing"),
            screenContent = screen
        )
        assertEquals("Could not find element with text \"Missing\"", result)
    }
}
