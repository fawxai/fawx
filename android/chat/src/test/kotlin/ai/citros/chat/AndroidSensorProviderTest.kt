package ai.citros.chat

import ai.citros.core.NetworkType
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class AndroidSensorProviderTest {

    @Test
    fun `classifyNetwork maps ethernet and vpn to online`() {
        val ethernet = AndroidSensorProvider.classifyNetwork(
            hasWifi = false,
            hasCellular = false,
            hasEthernet = true,
            hasVpn = false,
            hasInternetCapability = true
        )
        val vpn = AndroidSensorProvider.classifyNetwork(
            hasWifi = false,
            hasCellular = false,
            hasEthernet = false,
            hasVpn = true,
            hasInternetCapability = true
        )
        assertEquals(NetworkType.WIFI, ethernet)
        assertEquals(NetworkType.WIFI, vpn)
    }

    @Test
    fun `classifyNetwork treats wifi without internet capability as offline`() {
        val result = AndroidSensorProvider.classifyNetwork(
            hasWifi = true,
            hasCellular = false,
            hasEthernet = false,
            hasVpn = false,
            hasInternetCapability = false
        )

        assertEquals(NetworkType.OFFLINE, result)
    }

    @Test
    fun `classifyNetwork treats cellular without internet capability as offline`() {
        val result = AndroidSensorProvider.classifyNetwork(
            hasWifi = false,
            hasCellular = true,
            hasEthernet = false,
            hasVpn = false,
            hasInternetCapability = false
        )

        assertEquals(NetworkType.OFFLINE, result)
    }

    @Test
    fun `location freshness guard drops stale location`() {
        val stale = AndroidSensorProvider.isLocationFresh(
            locationTimeMillis = 1_000L,
            nowMillis = 10_000L,
            maxAgeMillis = 5_000L
        )
        val fresh = AndroidSensorProvider.isLocationFresh(
            locationTimeMillis = 8_000L,
            nowMillis = 10_000L,
            maxAgeMillis = 5_000L
        )
        assertEquals(false, stale)
        assertEquals(true, fresh)
    }

    @Test
    fun `formatCoarseLocation falls back locality admin then country`() {
        assertEquals(
            "Denver, Colorado",
            AndroidSensorProvider.formatCoarseLocation(locality = "Denver", admin = "Colorado", country = "USA")
        )
        assertEquals(
            "Colorado",
            AndroidSensorProvider.formatCoarseLocation(locality = "", admin = "Colorado", country = "USA")
        )
        assertEquals(
            "USA",
            AndroidSensorProvider.formatCoarseLocation(locality = "", admin = "", country = "USA")
        )
        assertNull(
            AndroidSensorProvider.formatCoarseLocation(locality = "", admin = "", country = "")
        )
    }

    @Test
    fun `snapshot returns null fields when sensor context disabled`() = runTest {
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
    fun `snapshot omits location when permission denied`() = runTest {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val provider = AndroidSensorProvider(
            context = context,
            sensorContextEnabled = { true },
            locationPermissionGranted = { false }
        )

        val snapshot = provider.snapshot()

        assertNull(snapshot.location)
    }

    @Test
    fun `runWithGeocoderTimeout returns null when work exceeds latency budget`() = runTest {
        val result = AndroidSensorProvider.runWithGeocoderTimeout {
            delay(AndroidSensorProvider.GEOCODER_TIMEOUT_MS + 25)
            "slow"
        }

        assertNull(result)
    }

    @Test
    fun `runWithGeocoderTimeout returns result when work is within latency budget`() = runTest {
        val result = AndroidSensorProvider.runWithGeocoderTimeout {
            delay(10)
            "ok"
        }

        assertEquals("ok", result)
        assertTrue(AndroidSensorProvider.GEOCODER_TIMEOUT_MS <= 15L)
    }

    @Test
    fun `runWithLocationTimeout returns null when work exceeds latency budget`() = runTest {
        val result = AndroidSensorProvider.runWithLocationTimeout {
            delay(AndroidSensorProvider.LOCATION_TIMEOUT_MS + 25)
            "slow-location"
        }

        assertNull(result)
    }

    @Test
    fun `runWithLocationTimeout returns result when work is within latency budget`() = runTest {
        val result = AndroidSensorProvider.runWithLocationTimeout {
            delay(10)
            "ok-location"
        }

        assertEquals("ok-location", result)
        assertTrue(AndroidSensorProvider.LOCATION_TIMEOUT_MS <= 15L)
    }
}
