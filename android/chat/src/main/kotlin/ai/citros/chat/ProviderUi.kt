package ai.citros.chat

import androidx.compose.ui.graphics.Color
import ai.citros.core.Provider

/**
 * UI utilities for provider-specific display elements.
 * Centralizes provider icons, colors, labels, and key formats.
 */
internal object ProviderUi {
    fun displayName(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> "Anthropic"
        Provider.OPENAI -> "OpenAI"
        Provider.OPENROUTER -> "OpenRouter"
    }

    fun icon(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> "🟠"
        Provider.OPENAI -> "🟢"
        Provider.OPENROUTER -> "🔷"
    }

    fun brandColor(provider: Provider): Color = when (provider) {
        Provider.ANTHROPIC -> Color(0xFFD97757)
        Provider.OPENAI -> Color(0xFF10A37F)
        Provider.OPENROUTER -> Color(0xFF6366F1)
    }

    fun keyPlaceholder(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> "sk-ant-..."
        Provider.OPENAI -> "sk-..."
        Provider.OPENROUTER -> "sk-or-..."
    }

    @Deprecated("Use icon() instead", ReplaceWith("icon(provider)"))
    fun glyph(provider: Provider): String = icon(provider)

    @Deprecated("Use brandColor() instead", ReplaceWith("brandColor(provider)"))
    fun accent(provider: Provider): Color = brandColor(provider)
}
