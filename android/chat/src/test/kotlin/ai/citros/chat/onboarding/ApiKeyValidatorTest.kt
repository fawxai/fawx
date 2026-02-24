package ai.citros.chat.onboarding

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

class ApiKeyValidatorTest {

    @Test
    fun `prefix detection for each known provider`() = runTest {
        val validator = ApiKeyValidator(FakeValidationGateway())

        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.ANTHROPIC),
            validator.validate("sk-ant-12345678901234567890")
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.OPENROUTER),
            validator.validate("sk-or-12345678901234567890")
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.OPENAI),
            validator.validate("sk-proj-12345678901234567890")
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.OPENAI),
            validator.validate("sk-12345678901234567890")
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.GROQ),
            validator.validate("gsk_12345678901234567890")
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.ProviderDetected(OnboardingProvider.XAI),
            validator.validate("xai-12345678901234567890")
        )
    }

    @Test
    fun `direct prefix detection for known and unknown patterns`() {
        val validator = ApiKeyValidator(FakeValidationGateway())

        assertEquals(OnboardingProvider.ANTHROPIC, validator.detectProviderFromPrefix("sk-ant-foo"))
        assertEquals(OnboardingProvider.OPENROUTER, validator.detectProviderFromPrefix("sk-or-foo"))
        assertEquals(OnboardingProvider.OPENAI, validator.detectProviderFromPrefix("sk-proj-foo"))
        assertEquals(OnboardingProvider.OPENAI, validator.detectProviderFromPrefix("sk-foo"))
        assertEquals(OnboardingProvider.GROQ, validator.detectProviderFromPrefix("gsk_foo"))
        assertEquals(OnboardingProvider.XAI, validator.detectProviderFromPrefix("xai-foo"))
        assertEquals(null, validator.detectProviderFromPrefix("unknown-foo"))
    }

    @Test
    fun `provider unknown for unrecognized formats`() = runTest {
        val validator = ApiKeyValidator(FakeValidationGateway())

        val result = validator.validate("abc-123456789")
        assertIs<ApiKeyValidator.ValidationResult.ProviderUnknown>(result)
    }

    @Test
    fun `validate returns invalid for empty and whitespace-only key`() = runTest {
        val validator = ApiKeyValidator(FakeValidationGateway())

        assertEquals(ApiKeyValidator.ValidationResult.Invalid("Key is empty"), validator.validate(""))
        assertEquals(ApiKeyValidator.ValidationResult.Invalid("Key is empty"), validator.validate("   "))
    }

    @Test
    fun `validateWithProvider returns valid when gateway accepts key`() = runTest {
        val gateway = FakeValidationGateway(isValid = true)
        val validator = ApiKeyValidator(gateway)

        val result = validator.validateWithProvider("  sk-ant-12345678901234567890  ", OnboardingProvider.ANTHROPIC)

        assertEquals(ApiKeyValidator.ValidationResult.Valid, result)
        assertEquals(OnboardingProvider.ANTHROPIC, gateway.lastProvider)
        assertEquals("sk-ant-12345678901234567890", gateway.lastKey)
    }

    @Test
    fun `validateWithProvider returns invalid when gateway rejects key`() = runTest {
        val gateway = FakeValidationGateway(isValid = false, invalidReason = "rejected")
        val validator = ApiKeyValidator(gateway)

        val result = validator.validateWithProvider("sk-or-12345678901234567890", OnboardingProvider.OPENROUTER)

        assertEquals(ApiKeyValidator.ValidationResult.Invalid("rejected"), result)
        assertEquals(OnboardingProvider.OPENROUTER, gateway.lastProvider)
        assertEquals("sk-or-12345678901234567890", gateway.lastKey)
    }

    @Test
    fun `validateWithProvider returns invalid for empty and whitespace-only key`() = runTest {
        val gateway = FakeValidationGateway()
        val validator = ApiKeyValidator(gateway)

        assertEquals(
            ApiKeyValidator.ValidationResult.Invalid("Key is empty"),
            validator.validateWithProvider("", OnboardingProvider.OPENAI)
        )
        assertEquals(
            ApiKeyValidator.ValidationResult.Invalid("Key is empty"),
            validator.validateWithProvider("   ", OnboardingProvider.OPENAI)
        )
        assertEquals(null, gateway.lastProvider)
        assertEquals(null, gateway.lastKey)
    }

    @Test
    fun `validateWithProvider returns network error when gateway fails`() = runTest {
        val gateway = FakeValidationGateway(networkMessage = "timeout")
        val validator = ApiKeyValidator(gateway)

        val result = validator.validateWithProvider("sk-proj-12345678901234567890", OnboardingProvider.OPENAI)

        assertEquals(ApiKeyValidator.ValidationResult.NetworkError("timeout"), result)
        assertEquals(OnboardingProvider.OPENAI, gateway.lastProvider)
        assertEquals("sk-proj-12345678901234567890", gateway.lastKey)
    }

    @Test
    fun `network error handling timeout and server error`() = runTest {
        val timeoutValidator = ApiKeyValidator(FakeValidationGateway(networkMessage = "timeout"))
        val timeoutResult = timeoutValidator.validate("sk-ant-12345678901234567890")
        assertEquals(ApiKeyValidator.ValidationResult.NetworkError("timeout"), timeoutResult)

        val serverValidator = ApiKeyValidator(FakeValidationGateway(networkMessage = "500 server error"))
        val serverResult = serverValidator.validate("sk-ant-12345678901234567890")
        assertEquals(ApiKeyValidator.ValidationResult.NetworkError("500 server error"), serverResult)
    }

    @Test
    fun `invalid result when provider rejects key`() = runTest {
        val validator = ApiKeyValidator(FakeValidationGateway(isValid = false, invalidReason = "rejected"))

        val result = validator.validate("sk-or-12345678901234567890")
        assertEquals(ApiKeyValidator.ValidationResult.Invalid("rejected"), result)
    }

    private class FakeValidationGateway(
        private val isValid: Boolean = true,
        private val invalidReason: String = "invalid",
        private val networkMessage: String? = null
    ) : ApiKeyValidationGateway {
        var lastProvider: OnboardingProvider? = null
        var lastKey: String? = null

        override suspend fun validate(
            provider: OnboardingProvider,
            key: String
        ): ApiKeyValidationGateway.ValidationResponse {
            lastProvider = provider
            lastKey = key
            networkMessage?.let {
                return ApiKeyValidationGateway.ValidationResponse.NetworkError(it)
            }
            return if (isValid) {
                ApiKeyValidationGateway.ValidationResponse.Valid
            } else {
                ApiKeyValidationGateway.ValidationResponse.Invalid(invalidReason)
            }
        }
    }
}
