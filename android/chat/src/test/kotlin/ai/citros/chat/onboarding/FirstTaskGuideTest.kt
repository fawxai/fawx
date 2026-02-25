package ai.citros.chat.onboarding

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class FirstTaskGuideTest {

    @Test
    fun `suggestedTasks contains expected entries in order`() {
        val guide = FirstTaskGuide()

        assertEquals(
            listOf(
                SuggestedTask("Open the weather app", "🌤️"),
                SuggestedTask("Take a screenshot", "📸"),
                SuggestedTask("What's on my screen?", "👀"),
                SuggestedTask("What time is it?", "🕐"),
                SuggestedTask("Open Settings", "⚙️"),
                SuggestedTask("Search for pizza near me", "🍕"),
                SuggestedTask("Set a timer for 5 minutes", "⏱️")
            ),
            guide.suggestedTasks
        )
    }

    @Test
    fun `firstTaskConfig has expected defaults`() {
        val config = FirstTaskGuide().firstTaskConfig()

        assertEquals(15, config.maxToolSteps)
        assertTrue(config.verboseProgress)
    }

    @Test
    fun `successMessage includes capabilities and cta`() {
        val message = FirstTaskGuide().successMessage()

        assertTrue(message.contains("Nice — your first task"))
        assertTrue(message.contains("Control any app on your phone"))
        assertTrue(message.contains("Search the web and summarize results"))
        assertTrue(message.contains("Remember things for later"))
        assertTrue(message.contains("Take screenshots and read your screen"))
        assertTrue(message.contains("Just ask me anything!"))
    }
}
