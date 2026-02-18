package ai.citros.core

/**
 * Structured exception for provider API errors.
 *
 * Provides type-safe error handling instead of relying on string parsing.
 *
 * @param provider The provider that generated the error
 * @param statusCode HTTP status code, or null for non-HTTP errors (network issues, timeouts)
 * @param message Human-readable error message
 * @param isAuthFailure True if this is an authentication/authorization failure (401, 403, invalid key)
 * @param cause The underlying exception, if any
 */
class ProviderException(
    val provider: Provider,
    val statusCode: Int?,
    message: String,
    val isAuthFailure: Boolean,
    cause: Throwable? = null
) : Exception("${provider.name} API error: $message", cause) {

    companion object {
        /**
         * Check if a Result contains an authentication failure.
         */
        fun isAuthFailure(result: Result<*>): Boolean {
            val exception = result.exceptionOrNull()
            return exception is ProviderException && exception.isAuthFailure
        }
    }
}
