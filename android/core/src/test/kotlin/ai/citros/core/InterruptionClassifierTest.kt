package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

class InterruptionClassifierTest {

    @Test
    fun `classifyWindowChange returns null when isAgentAction is true`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.example.app",
            expectedPackage = "com.other.app",
            isAgentAction = true
        )
        assertNull(result)
    }

    @Test
    fun `classifyWindowChange returns null when package matches expected`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.example.app",
            expectedPackage = "com.example.app",
            isAgentAction = false
        )
        assertNull(result)
    }

    @Test
    fun `classifyWindowChange returns AppSwitch for unexpected package`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.example.app",
            expectedPackage = "com.other.app",
            isAgentAction = false
        )
        assertTrue(result is InterruptionEvent.AppSwitch)
        val appSwitch = result as InterruptionEvent.AppSwitch
        assertEquals("com.other.app", appSwitch.previousApp)
        assertEquals("com.example.app", appSwitch.newApp)
    }

    @Test
    fun `classifyWindowChange returns ExternalInterrupt for all dialer packages`() {
        val dialerPackages = listOf(
            "com.android.dialer",
            "com.google.android.dialer",
            "com.android.incallui",
            "com.samsung.android.incallui"
        )
        for (pkg in dialerPackages) {
            val result = InterruptionClassifier.classifyWindowChange(
                newPackage = pkg,
                expectedPackage = "com.example.app",
                isAgentAction = false
            )
            assertTrue("Expected ExternalInterrupt for $pkg", result is InterruptionEvent.ExternalInterrupt)
        }
    }

    @Test
    fun `classifyWindowChange returns ExternalInterrupt for system packages`() {
        val systemPackages = listOf("android", "com.android.systemui")
        for (pkg in systemPackages) {
            val result = InterruptionClassifier.classifyWindowChange(
                newPackage = pkg,
                expectedPackage = "com.example.app",
                isAgentAction = false
            )
            assertTrue("Expected ExternalInterrupt for $pkg", result is InterruptionEvent.ExternalInterrupt)
        }
    }

    @Test
    fun `classifyWindowChange uses unknown as previous when expectedPackage is null`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.example.app",
            expectedPackage = null,
            isAgentAction = false
        )
        assertTrue(result is InterruptionEvent.AppSwitch)
        assertEquals("unknown", (result as InterruptionEvent.AppSwitch).previousApp)
    }

    @Test
    fun `classifyWindowChange message for incoming call contains description`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.android.dialer",
            expectedPackage = "com.example.app",
            isAgentAction = false
        ) as InterruptionEvent.ExternalInterrupt
        assertTrue(result.description.contains("phone call", ignoreCase = true))
    }

    @Test
    fun `classifyWindowChange message for system dialog contains description`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.android.systemui",
            expectedPackage = "com.example.app",
            isAgentAction = false
        ) as InterruptionEvent.ExternalInterrupt
        assertTrue(result.description.contains("system", ignoreCase = true))
    }

    @Test
    fun `classifyWindowChange returns null for all known keyboard packages`() {
        val keyboardPackages = listOf(
            "com.google.android.inputmethod.latin",
            "com.samsung.android.honeyboard",
            "com.swiftkey",
            "com.touchtype.swiftkey",
            "com.baidu.input",
            "com.iflytek.inputmethod",
            "com.android.inputmethod.latin"
        )

        keyboardPackages.forEach { pkg ->
            val result = InterruptionClassifier.classifyWindowChange(
                newPackage = pkg,
                expectedPackage = "com.google.android.gm",
                isAgentAction = false
            )
            assertNull("Expected no interruption for keyboard package $pkg", result)
        }
    }

    @Test
    fun `classifyWindowChange returns null for keyboard package when expectedPackage is null`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.google.android.inputmethod.latin",
            expectedPackage = null,
            isAgentAction = false
        )

        assertNull(result)
    }

    @Test
    fun `classifyWindowChange prioritizes agent actions over keyboard package checks`() {
        val result = InterruptionClassifier.classifyWindowChange(
            newPackage = "com.google.android.inputmethod.latin",
            expectedPackage = "com.google.android.gm",
            isAgentAction = true
        )

        assertNull(result)
    }
}
