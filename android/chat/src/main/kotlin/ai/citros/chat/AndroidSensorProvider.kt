package ai.citros.chat

import ai.citros.core.NetworkType
import ai.citros.core.SensorContext
import ai.citros.core.SensorProvider
import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.location.Geocoder
import android.location.Location
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import androidx.core.content.ContextCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.withContext
import java.time.ZonedDateTime
import java.util.Locale

/**
 * Production [SensorProvider] backed by Android system services.
 *
 * Best-effort only: all per-sensor failures degrade to null fields and never throw.
 */
class AndroidSensorProvider(
    private val context: Context,
    private val sensorContextEnabled: () -> Boolean = { true },
    private val locationPermissionGranted: () -> Boolean = {
        val coarse = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION)
        val fine = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION)
        coarse == PackageManager.PERMISSION_GRANTED || fine == PackageManager.PERMISSION_GRANTED
    },
    private val nowMillis: () -> Long = { System.currentTimeMillis() },
    private val nowDateTime: () -> ZonedDateTime = { ZonedDateTime.now() }
) : SensorProvider {

    companion object {
        private const val MAX_LOCATION_AGE_MS = 60L * 60L * 1000L // 1 hour
        internal const val LOCATION_TIMEOUT_MS = 15L
        internal const val GEOCODER_TIMEOUT_MS = 15L

        internal suspend fun <T> runWithLocationTimeout(block: suspend () -> T?): T? {
            return withTimeoutOrNull(LOCATION_TIMEOUT_MS) { block() }
        }

        internal suspend fun <T> runWithGeocoderTimeout(block: suspend () -> T?): T? {
            return withTimeoutOrNull(GEOCODER_TIMEOUT_MS) { block() }
        }

        internal fun classifyNetwork(
            hasWifi: Boolean,
            hasCellular: Boolean,
            hasEthernet: Boolean,
            hasVpn: Boolean,
            hasInternetCapability: Boolean
        ): NetworkType {
            return when {
                hasWifi && hasInternetCapability -> NetworkType.WIFI
                hasCellular && hasInternetCapability -> NetworkType.CELLULAR
                // Ethernet/VPN are "online but non-cellular" and map to WIFI bucket in prompt semantics.
                (hasEthernet || hasVpn) && hasInternetCapability -> NetworkType.WIFI
                else -> NetworkType.OFFLINE
            }
        }

        internal fun isLocationFresh(
            locationTimeMillis: Long,
            nowMillis: Long,
            maxAgeMillis: Long
        ): Boolean {
            if (locationTimeMillis <= 0L) return false
            val ageMillis = nowMillis - locationTimeMillis
            return ageMillis in 0..maxAgeMillis
        }

        internal fun formatCoarseLocation(locality: String?, admin: String?, country: String?): String? {
            val normalizedLocality = locality?.trim().orEmpty()
            val normalizedAdmin = admin?.trim().orEmpty()
            val normalizedCountry = country?.trim().orEmpty()
            return when {
                normalizedLocality.isNotBlank() && normalizedAdmin.isNotBlank() ->
                    "$normalizedLocality, $normalizedAdmin"
                normalizedLocality.isNotBlank() -> normalizedLocality
                normalizedAdmin.isNotBlank() -> normalizedAdmin
                normalizedCountry.isNotBlank() -> normalizedCountry
                else -> null
            }
        }
    }

    override suspend fun snapshot(): SensorContext = withContext(Dispatchers.IO) {
        if (!sensorContextEnabled()) return@withContext SensorContext()
        SensorContext(
            batteryPercent = readBatteryPercent(),
            isCharging = readChargingState(),
            networkType = readNetworkType(),
            location = runWithLocationTimeout { readCoarseLocation() },
            localTime = nowDateTime()
        )
    }

    private fun readBatteryPercent(): Int? {
        return runCatching {
            val battery = context.getSystemService(BatteryManager::class.java) ?: return null
            val value = battery.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
            value.takeIf { it in 0..100 }
        }.getOrNull()
    }

    private fun readChargingState(): Boolean? {
        return runCatching {
            val intent = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
                ?: return null
            val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            when (status) {
                BatteryManager.BATTERY_STATUS_CHARGING,
                BatteryManager.BATTERY_STATUS_FULL -> true
                BatteryManager.BATTERY_STATUS_DISCHARGING,
                BatteryManager.BATTERY_STATUS_NOT_CHARGING -> false
                else -> null
            }
        }.getOrNull()
    }

    private fun readNetworkType(): NetworkType? {
        return runCatching {
            val manager = context.getSystemService(ConnectivityManager::class.java) ?: return null
            val capabilities = manager.getNetworkCapabilities(manager.activeNetwork)
                ?: return NetworkType.OFFLINE
            classifyNetwork(
                hasWifi = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI),
                hasCellular = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR),
                hasEthernet = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET),
                hasVpn = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN),
                hasInternetCapability = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            )
        }.getOrNull()
    }

    private suspend fun readCoarseLocation(): String? {
        if (!locationPermissionGranted()) return null

        val location = runCatching {
            val manager = context.getSystemService(LocationManager::class.java) ?: return null
            val providers = manager.getProviders(true)
            providers.asSequence()
                .mapNotNull { provider ->
                    try {
                        manager.getLastKnownLocation(provider)
                    } catch (_: SecurityException) {
                        null
                    }
                }
                .maxByOrNull(Location::getTime)
        }.getOrNull() ?: return null

        if (!isLocationFresh(location.time, nowMillis(), MAX_LOCATION_AGE_MS)) {
            return null
        }

        return reverseGeocode(location)
    }

    @Suppress("DEPRECATION")
    private suspend fun reverseGeocode(location: Location): String? {
        return runWithGeocoderTimeout {
            runInterruptible {
                runCatching {
                    if (!Geocoder.isPresent()) return@runCatching null
                    val geocoder = Geocoder(context, Locale.getDefault())
                    val address = geocoder.getFromLocation(location.latitude, location.longitude, 1)
                        ?.firstOrNull()
                        ?: return@runCatching null

                    val locality = address.locality?.trim().orEmpty()
                    val admin = address.adminArea
                    val country = address.countryName

                    formatCoarseLocation(locality = locality, admin = admin, country = country)
                }.getOrNull()
            }
        }
    }
}
