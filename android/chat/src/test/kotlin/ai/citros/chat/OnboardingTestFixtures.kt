package ai.citros.chat

internal object OnboardingTestFixtures {
    fun sampleProfile() = OnboardingIdentityProfile(
        agentName = "Zest",
        agentNature = "citrus spirit",
        agentVibe = "chill but sharp",
        agentEmoji = "🍋",
        userName = "Joe",
        userAddress = "captain",
        relationshipStyle = "casual and direct",
        boundaries = "ask before sending messages",
        userContext = "prefers concise updates",
        confidence = 1f
    )
}
