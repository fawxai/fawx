package ai.citros.core

import org.junit.Assert.*
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime

class SensorContextTest {

    @Test
    fun `all fields null returns empty string`() {
        assertEquals("", SensorContext().toPromptLine())
    }

    @Test
    fun `all fields populated returns full line`() {
        val ctx = SensorContext(
            batteryPercent = 72,
            isCharging = true,
            networkType = NetworkType.WIFI,
            location = "Denver, CO",
            localTime = ZonedDateTime.of(2026, 2, 21, 16, 15, 0, 0, ZoneId.of("America/Denver"))
        )
        val line = ctx.toPromptLine()
        assertTrue(line.startsWith("Device: "))
        assertTrue(line.contains("battery=72% (charging)"))
        assertTrue(line.contains("wifi"))
        assertTrue(line.contains("Denver, CO"))
        assertTrue(line.contains("4:15 PM"))
    }

    @Test
    fun `battery only`() {
        assertEquals("Device: battery=72%", SensorContext(batteryPercent = 72).toPromptLine())
    }

    @Test
    fun `battery with charging`() {
        assertEquals(
            "Device: battery=72% (charging)",
            SensorContext(batteryPercent = 72, isCharging = true).toPromptLine()
        )
    }

    @Test
    fun `battery not charging`() {
        assertEquals(
            "Device: battery=72%",
            SensorContext(batteryPercent = 72, isCharging = false).toPromptLine()
        )
    }

    @Test
    fun `isCharging without battery is ignored`() {
        assertEquals("", SensorContext(isCharging = true).toPromptLine())
    }

    @Test
    fun `isCharging true with explicit null battery is ignored`() {
        assertEquals("", SensorContext(batteryPercent = null, isCharging = true).toPromptLine())
    }

    @Test
    fun `network only wifi`() {
        assertEquals("Device: wifi", SensorContext(networkType = NetworkType.WIFI).toPromptLine())
    }

    @Test
    fun `network only cellular`() {
        assertEquals("Device: cellular", SensorContext(networkType = NetworkType.CELLULAR).toPromptLine())
    }

    @Test
    fun `network offline`() {
        assertEquals("Device: offline", SensorContext(networkType = NetworkType.OFFLINE).toPromptLine())
    }

    @Test
    fun `location only`() {
        assertEquals("Device: location=\"Denver, CO\"", SensorContext(location = "Denver, CO").toPromptLine())
    }

    @Test
    fun `blank location is skipped`() {
        assertEquals("", SensorContext(location = "  ").toPromptLine())
    }

    @Test
    fun `time only`() {
        val time = ZonedDateTime.of(2026, 2, 21, 16, 15, 0, 0, ZoneId.of("America/Denver"))
        val line = SensorContext(localTime = time).toPromptLine()
        assertTrue(line.startsWith("Device: "))
        assertTrue(line.contains("4:15 PM"))
    }

    @Test
    fun `battery boundary zero`() {
        assertEquals("Device: battery=0%", SensorContext(batteryPercent = 0).toPromptLine())
    }

    @Test
    fun `battery boundary 100`() {
        assertEquals("Device: battery=100%", SensorContext(batteryPercent = 100).toPromptLine())
    }

    @Test
    fun `battery boundary 15`() {
        assertEquals("Device: battery=15%", SensorContext(batteryPercent = 15).toPromptLine())
    }

    @Test
    fun `battery below 0 is clamped`() {
        assertEquals("Device: battery=0%", SensorContext(batteryPercent = -1).toPromptLine())
    }

    @Test
    fun `battery above 100 is clamped`() {
        assertEquals("Device: battery=100%", SensorContext(batteryPercent = 101).toPromptLine())
    }

    @Test
    fun `battery and network combo`() {
        val line = SensorContext(batteryPercent = 50, networkType = NetworkType.CELLULAR).toPromptLine()
        assertEquals("Device: battery=50% | cellular", line)
    }

    @Test
    fun `network and location combo`() {
        val line = SensorContext(networkType = NetworkType.WIFI, location = "Boston, MA").toPromptLine()
        assertEquals("Device: wifi | location=\"Boston, MA\"", line)
    }

    @Test
    fun `location is sanitized and truncated`() {
        val longWithNewline = "A".repeat(99) + "\nB"
        val line = SensorContext(location = longWithNewline).toPromptLine()
        assertEquals("Device: location=\"${"A".repeat(99)} \"", line)
    }

    @Test
    fun `location sanitization normalizes carriage return tab and control chars`() {
        val line = SensorContext(location = "San\rFran\tcisco\u0000, CA\u001f").toPromptLine()
        assertEquals("Device: location=\"San Fran cisco , CA \"", line)
    }

    @Test
    fun `location sanitization blocks separator injection payloads`() {
        val line = SensorContext(location = "Denver | battery=1% | offline").toPromptLine()
        assertEquals("Device: location=\"Denver / battery=1% / offline\"", line)
        assertFalse(line.contains(" | battery=1% | "))
    }

    @Test
    fun `location sanitization escapes quotes and backslashes`() {
        val line = SensorContext(location = "He said \"go\" \\ now").toPromptLine()
        assertEquals("Device: location=\"He said 'go' / now\"", line)
    }

    @Test
    fun `location sanitization preserves unicode city names`() {
        val line = SensorContext(location = "São Paulo, Brasil").toPromptLine()
        assertEquals("Device: location=\"São Paulo, Brasil\"", line)
    }
}
