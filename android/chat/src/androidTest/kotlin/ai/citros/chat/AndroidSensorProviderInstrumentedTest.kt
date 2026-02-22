package ai.citros.chat

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import ai.citros.core.NetworkType
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidSensorProviderInstrumentedTest {

    @Test
    fun snapshot_withSensorContextDisabled_returnsEmptyContextOnDevice() = runTest {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val provider = AndroidSensorProvider(
            context = context,
            sensorContextEnabled = { false }
        )

        val snapshot = provider.snapshot()

        assertNull(snapshot.batteryPercent)
        assertNull(snapshot.isCharging)
        assertNull(snapshot.networkType)
        assertNull(snapshot.location)
        assertNull(snapshot.localTime)
    }

    @Test
    fun snapshot_onDevice_doesNotThrow_whenPermissionCheckAllowsLocationPath() = runTest {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val provider = AndroidSensorProvider(
            context = context,
            sensorContextEnabled = { true },
            locationPermissionGranted = { true }
        )

        val snapshot = provider.snapshot()

        // Exercise real Android services path. Values are environment-dependent.
        assertNotNull(snapshot)
        assertNotNull(snapshot.localTime)
        snapshot.networkType?.let { networkType ->
            assertTrue(
                networkType == NetworkType.WIFI ||
                    networkType == NetworkType.CELLULAR ||
                    networkType == NetworkType.OFFLINE
            )
        }
        snapshot.batteryPercent?.let { percent ->
            assertTrue(percent in 0..100)
        }
    }

    @Test
    fun snapshot_onDevice_omitsLocation_whenPermissionDenied() = runTest {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val provider = AndroidSensorProvider(
            context = context,
            sensorContextEnabled = { true },
            locationPermissionGranted = { false }
        )

        val snapshot = provider.snapshot()

        assertNull(snapshot.location)
    }
}
