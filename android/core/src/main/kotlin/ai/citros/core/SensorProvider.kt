package ai.citros.core

/**
 * Provides device sensor context. Production implementation uses
 * Android BatteryManager, ConnectivityManager, and LocationManager.
 * Test implementation returns fixed values.
 *
 * Contract: [snapshot] MUST NOT throw exceptions. All errors are
 * handled internally — failed sensors result in null fields in the
 * returned [SensorContext].
 */
interface SensorProvider {
    /** Gather current sensor snapshot. Should be fast (<100ms). */
    suspend fun snapshot(): SensorContext
}
