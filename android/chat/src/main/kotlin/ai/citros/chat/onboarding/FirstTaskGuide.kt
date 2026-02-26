package ai.citros.chat.onboarding

class FirstTaskGuide {
    val suggestedTasks = listOf(
        SuggestedTask("Open the weather app", "🌤️"),
        SuggestedTask("Take a screenshot", "📸"),
        SuggestedTask("What's on my screen?", "👀"),
        SuggestedTask("What time is it?", "🕐"),
        SuggestedTask("Open Settings", "⚙️"),
        SuggestedTask("Search for pizza near me", "🍕"),
        SuggestedTask("Set a timer for 5 minutes", "⏱️")
    )

    fun firstTaskConfig() = FirstTaskConfig(
        maxToolSteps = 15,
        verboseProgress = true
    )

    fun successMessage() = """
        🎉 Nice — your first task! Here's what I can do:
        • Control any app on your phone
        • Search the web and summarize results
        • Remember things for later
        • Take screenshots and read your screen

        Just ask me anything!
    """.trimIndent()
}

data class SuggestedTask(val text: String, val emoji: String)
data class FirstTaskConfig(val maxToolSteps: Int, val verboseProgress: Boolean)
