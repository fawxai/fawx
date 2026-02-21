package ai.citros.chat

import android.content.ComponentName
import android.content.Context
import android.content.SharedPreferences
import android.content.pm.PackageManager
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.mockito.ArgumentMatchers.anyInt
import org.mockito.kotlin.any
import org.mockito.kotlin.argumentCaptor
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.times
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class LauncherIconManagerTest {

    @Test
    fun `syncLauncherIconWithPreferences no-op when onboarding incomplete`() {
        val context = mock<Context>()
        val prefs = mock<SharedPreferences>()
        val packageManager = mock<PackageManager>()

        whenever(context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)).thenReturn(prefs)
        whenever(prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false)).thenReturn(false)
        whenever(context.packageManager).thenReturn(packageManager)

        syncLauncherIconWithPreferences(context)

        verify(packageManager, never()).setComponentEnabledSetting(any(), anyInt(), anyInt())
    }

    @Test
    fun `setLauncherIconFlavor enables selected alias and disables others`() {
        val context = mock<Context>()
        val packageManager = mock<PackageManager>()
        val packageName = "ai.citros.chat"

        whenever(context.packageName).thenReturn(packageName)
        whenever(context.packageManager).thenReturn(packageManager)

        setLauncherIconFlavor(context, CitrosFlavor.LEMON)

        val componentCaptor = argumentCaptor<ComponentName>()
        val stateCaptor = argumentCaptor<Int>()
        verify(packageManager, times(6)).setComponentEnabledSetting(
            componentCaptor.capture(),
            stateCaptor.capture(),
            eq(PackageManager.DONT_KILL_APP)
        )

        val statesByAlias = componentCaptor.allValues
            .map { it.className }
            .zip(stateCaptor.allValues)
            .toMap()

        assertEquals(
            setOf(
                "$packageName.LauncherNone",
                "$packageName.LauncherLemon",
                "$packageName.LauncherTangerine",
                "$packageName.LauncherLime",
                "$packageName.LauncherBloodOrange",
                "$packageName.LauncherGrapefruit"
            ),
            statesByAlias.keys
        )
        assertEquals(
            PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
            statesByAlias["$packageName.LauncherLemon"]
        )
        assertTrue(
            statesByAlias
                .filterKeys { it != "$packageName.LauncherLemon" }
                .values
                .all { it == PackageManager.COMPONENT_ENABLED_STATE_DISABLED }
        )
    }
}
