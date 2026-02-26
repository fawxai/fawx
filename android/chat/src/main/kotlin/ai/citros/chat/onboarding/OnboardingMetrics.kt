package ai.citros.chat.onboarding

import android.content.SharedPreferences
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

@Serializable
data class OnboardingMetrics(
    val startedAt: Long,
    val completedAt: Long? = null,
    val stepsCompleted: List<String> = emptyList(),
    val keyEntryAttempts: Int = 0,
    val providerSelected: String? = null,
    val modelSelected: String? = null,
    val accessibilityGrantTimeMs: Long? = null,
    val firstTaskSuccess: Boolean? = null
)

class OnboardingMetricsTracker(
    private val prefs: SharedPreferences,
    private val nowMs: () -> Long = { System.currentTimeMillis() }
) {
    private var metrics: OnboardingMetrics? = null

    fun start(): OnboardingMetrics {
        val started = OnboardingMetrics(startedAt = nowMs())
        metrics = started
        save(started)
        return started
    }

    fun recordStep(step: String): OnboardingMetrics = update {
        if (step in it.stepsCompleted) it else it.copy(stepsCompleted = it.stepsCompleted + step)
    }

    fun recordKeyAttempt(): OnboardingMetrics = update {
        it.copy(keyEntryAttempts = it.keyEntryAttempts + 1)
    }

    fun recordProvider(provider: String): OnboardingMetrics = update {
        it.copy(providerSelected = provider)
    }

    fun recordModel(model: String): OnboardingMetrics = update {
        it.copy(modelSelected = model)
    }

    fun recordAccessibilityGrant(timeMs: Long): OnboardingMetrics = update {
        it.copy(accessibilityGrantTimeMs = timeMs)
    }

    fun recordFirstTask(success: Boolean): OnboardingMetrics = update {
        it.copy(firstTaskSuccess = success)
    }

    fun complete(): OnboardingMetrics = update {
        it.copy(completedAt = nowMs())
    }

    fun load(): OnboardingMetrics? {
        val raw = prefs.getString(PREF_KEY, null) ?: return null
        return runCatching { json.decodeFromString<OnboardingMetrics>(raw) }
            .onSuccess { metrics = it }
            .getOrNull()
    }

    fun save(metrics: OnboardingMetrics) {
        prefs.edit().putString(PREF_KEY, json.encodeToString(metrics)).apply()
        this.metrics = metrics
    }

    private fun update(transform: (OnboardingMetrics) -> OnboardingMetrics): OnboardingMetrics {
        val current = metrics ?: load() ?: start()
        val updated = transform(current)
        save(updated)
        return updated
    }

    companion object {
        private const val PREF_KEY = "onboarding_metrics"
        private val json = Json { ignoreUnknownKeys = true }
    }
}
