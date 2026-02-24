package ai.citros.chat.onboarding

import android.content.Context
import android.provider.Settings
import androidx.test.core.app.ApplicationProvider
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest

@RunWith(RobolectricTestRunner::class)
class AccessibilitySetupHelperTest {

    private lateinit var context: Context
    private lateinit var helper: AccessibilitySetupHelper

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        helper = AccessibilitySetupHelper(context)
    }

    @Test
    fun `isAccessibilityEnabled returns false when no services enabled`() {
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ""
        )
        assertFalse(helper.isAccessibilityEnabled())
    }

    @Test
    fun `isAccessibilityEnabled returns false when other service enabled`() {
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            "com.other.app/com.other.app.SomeService"
        )
        assertFalse(helper.isAccessibilityEnabled())
    }

    @Test
    fun `isAccessibilityEnabled returns true when citros service enabled`() {
        val component = "${context.packageName}/${AccessibilitySetupHelper.DEFAULT_SERVICE_CLASS}"
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            component
        )
        assertTrue(helper.isAccessibilityEnabled())
    }

    @Test
    fun `isAccessibilityEnabled returns true when citros among multiple services`() {
        val component = "${context.packageName}/${AccessibilitySetupHelper.DEFAULT_SERVICE_CLASS}"
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            "com.other/com.other.Service:$component"
        )
        assertTrue(helper.isAccessibilityEnabled())
    }

    @Test
    fun `waitForPermission returns true when granted mid-poll`() = runTest {
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ""
        )

        val component = "${context.packageName}/${AccessibilitySetupHelper.DEFAULT_SERVICE_CLASS}"
        launch {
            kotlinx.coroutines.delay(200)
            Settings.Secure.putString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
                component
            )
        }

        val deferred = async { helper.waitForPermission(timeoutMs = 5_000, pollIntervalMs = 100) }
        advanceTimeBy(300)
        assertTrue(deferred.await())
    }

    @Test
    fun `waitForPermission returns false on timeout`() = runTest {
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ""
        )

        // Use very short timeout so test doesn't hang
        val result = helper.waitForPermission(timeoutMs = 1, pollIntervalMs = 1)
        assertFalse(result)
    }
}
