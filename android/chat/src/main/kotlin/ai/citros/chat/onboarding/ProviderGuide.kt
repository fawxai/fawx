package ai.citros.chat.onboarding

enum class OnboardingProvider {
    ANTHROPIC,
    OPENROUTER,
    OPENAI,
    GROQ,
    XAI
}

data class ProviderGuide(
    val provider: OnboardingProvider,
    val displayName: String,
    val description: String,
    val keyPageUrl: String,
    val recommendation: String?,
    val hasFreeCredits: Boolean
)

val PROVIDER_GUIDES = listOf(
    ProviderGuide(
        provider = OnboardingProvider.ANTHROPIC,
        displayName = "Anthropic (Claude)",
        description = "Best phone control quality. Pay-as-you-go.",
        keyPageUrl = "https://console.anthropic.com/settings/keys",
        recommendation = "Recommended — Claude Sonnet is our best-tested model",
        hasFreeCredits = false
    ),
    ProviderGuide(
        provider = OnboardingProvider.OPENROUTER,
        displayName = "OpenRouter",
        description = "Access many models with one key. Some free models available.",
        keyPageUrl = "https://openrouter.ai/keys",
        recommendation = null,
        hasFreeCredits = true
    ),
    ProviderGuide(
        provider = OnboardingProvider.OPENAI,
        displayName = "OpenAI",
        description = "Direct GPT access. Pay-as-you-go.",
        keyPageUrl = "https://platform.openai.com/api-keys",
        recommendation = null,
        hasFreeCredits = false
    ),
    ProviderGuide(
        provider = OnboardingProvider.GROQ,
        displayName = "Groq",
        description = "Very fast inference. Free tier available.",
        keyPageUrl = "https://console.groq.com/keys",
        recommendation = null,
        hasFreeCredits = true
    ),
    ProviderGuide(
        provider = OnboardingProvider.XAI,
        displayName = "xAI",
        description = "Grok models via xAI API.",
        keyPageUrl = "https://console.x.ai/",
        recommendation = null,
        hasFreeCredits = false
    )
)
