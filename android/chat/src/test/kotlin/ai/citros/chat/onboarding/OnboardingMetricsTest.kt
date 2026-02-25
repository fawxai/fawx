package ai.citros.chat.onboarding

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertNotNull

@RunWith(RobolectricTestRunner::class)
class OnboardingMetricsTest {

    private lateinit var prefs: android.content.SharedPreferences

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        prefs = context.getSharedPreferences("test_onboarding_metrics", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
    }

    @Test
    fun `persistence round trip saves and restores all fields`() {
        var now = 1000L
        val tracker = OnboardingMetricsTracker(prefs) { now }

        tracker.start()
        tracker.recordStep("WELCOME")
        tracker.recordStep("API_KEY_ENTRY")
        tracker.recordKeyAttempt()
        tracker.recordKeyAttempt()
        tracker.recordProvider("ANTHROPIC")
        tracker.recordModel("claude-sonnet-4-5-latest")
        tracker.recordAccessibilityGrant(4200L)
        tracker.recordFirstTask(success = true)
        now = 9000L
        tracker.complete()

        val loaded = tracker.load()
        assertNotNull(loaded)
        assertEquals(1000L, loaded.startedAt)
        assertEquals(9000L, loaded.completedAt)
        assertEquals(listOf("WELCOME", "API_KEY_ENTRY"), loaded.stepsCompleted)
        assertEquals(2, loaded.keyEntryAttempts)
        assertEquals("ANTHROPIC", loaded.providerSelected)
        assertEquals("claude-sonnet-4-5-latest", loaded.modelSelected)
        assertEquals(4200L, loaded.accessibilityGrantTimeMs)
        assertEquals(true, loaded.firstTaskSuccess)
    }
}
