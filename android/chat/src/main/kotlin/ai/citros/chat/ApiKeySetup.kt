package ai.citros.chat

import ai.citros.core.Provider

enum class ApiKeyValidationStatus {
    UNKNOWN,
    VALID,
    INVALID,
    EXPIRED
}

fun providerDashboardUrl(provider: Provider): String {
    return when (provider) {
        Provider.ANTHROPIC -> "https://console.anthropic.com/settings/keys"
        Provider.OPENAI -> "https://platform.openai.com/api-keys"
        Provider.OPENROUTER -> "https://openrouter.ai/keys"
    }
}


/**
 * Basic format check for API key prefixes. This is a quick client-side gate;
 * actual key validation happens via the provider's API in [validateApiCredential].
 *
 * Note: sess- and oauth_ prefixes are not provider-specific and are accepted
 * for any provider that supports session-based authentication.
 */
fun isValidKeyFormat(token: String, provider: Provider): Boolean {
    val trimmed = token.trim()
    if (trimmed.isEmpty()) return false

    // Session/OAuth tokens are provider-agnostic
    if (trimmed.startsWith("sess-") || trimmed.startsWith("oauth_")) return true

    return when (provider) {
        Provider.ANTHROPIC -> trimmed.startsWith("sk-ant-")
        Provider.OPENAI -> trimmed.startsWith("sk-") || trimmed.startsWith("oa-")
        Provider.OPENROUTER -> trimmed.startsWith("sk-or-")
    }
}
