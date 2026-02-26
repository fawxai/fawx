package ai.citros.chat.onboarding

class ApiKeyValidator(
    private val validationGateway: ApiKeyValidationGateway
) {
    sealed class ValidationResult {
        data object Valid : ValidationResult()
        data class Invalid(val reason: String) : ValidationResult()
        data class ProviderDetected(val provider: OnboardingProvider) : ValidationResult()
        data class ProviderUnknown(val hint: String) : ValidationResult()
        data class NetworkError(val message: String) : ValidationResult()
    }

    suspend fun validate(key: String): ValidationResult {
        val trimmedKey = key.trim()
        if (trimmedKey.isBlank()) {
            return ValidationResult.Invalid("Key is empty")
        }

        val detectedProvider = detectProviderFromPrefix(trimmedKey)
            ?: return ValidationResult.ProviderUnknown(
                "Unrecognized key prefix. Choose provider manually and try validation."
            )

        return when (val response = validationGateway.validate(detectedProvider, trimmedKey)) {
            is ApiKeyValidationGateway.ValidationResponse.Valid -> {
                ValidationResult.ProviderDetected(detectedProvider)
            }
            is ApiKeyValidationGateway.ValidationResponse.Invalid -> {
                ValidationResult.Invalid(response.reason)
            }
            is ApiKeyValidationGateway.ValidationResponse.NetworkError -> {
                ValidationResult.NetworkError(response.message)
            }
        }
    }

    suspend fun validateWithProvider(
        key: String,
        provider: OnboardingProvider
    ): ValidationResult {
        val trimmedKey = key.trim()
        if (trimmedKey.isBlank()) {
            return ValidationResult.Invalid("Key is empty")
        }

        return when (val response = validationGateway.validate(provider, trimmedKey)) {
            is ApiKeyValidationGateway.ValidationResponse.Valid -> ValidationResult.Valid
            is ApiKeyValidationGateway.ValidationResponse.Invalid -> ValidationResult.Invalid(response.reason)
            is ApiKeyValidationGateway.ValidationResponse.NetworkError -> ValidationResult.NetworkError(response.message)
        }
    }

    internal fun detectProviderFromPrefix(key: String): OnboardingProvider? {
        return when {
            key.startsWith("sk-ant-") -> OnboardingProvider.ANTHROPIC
            key.startsWith("sk-or-") -> OnboardingProvider.OPENROUTER
            // Intentional broad fallback: plain `sk-` keys are treated as OpenAI.
            // Ordering matters so more specific `sk-ant-`/`sk-or-` prefixes win first.
            key.startsWith("sk-proj-") || key.startsWith("sk-") -> OnboardingProvider.OPENAI
            key.startsWith("gsk_") -> OnboardingProvider.GROQ
            key.startsWith("xai-") -> OnboardingProvider.XAI
            else -> null
        }
    }
}

interface ApiKeyValidationGateway {
    sealed class ValidationResponse {
        data object Valid : ValidationResponse()
        data class Invalid(val reason: String) : ValidationResponse()
        data class NetworkError(val message: String) : ValidationResponse()
    }

    suspend fun validate(provider: OnboardingProvider, key: String): ValidationResponse
}
