package ai.citros.core

sealed class ScreenshotResult {
    data class Success(val base64: String) : ScreenshotResult()
    data object PrivacyBlocked : ScreenshotResult()
    data class Failed(val reason: String? = null) : ScreenshotResult()
}
