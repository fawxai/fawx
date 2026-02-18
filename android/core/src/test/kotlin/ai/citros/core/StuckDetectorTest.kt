package ai.citros.core

import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class StuckDetectorTest {

    private lateinit var detector: StuckDetector

    @Before
    fun setup() {
        detector = StuckDetector()
    }

    @Test
    fun `no warning when screen hashes are different`() {
        val state = StuckDetector.State()
        assertNull(detector.check(state, "tap", 100))
        assertNull(detector.check(state, "tap", 200))
        assertNull(detector.check(state, "tap", 300))
    }

    @Test
    fun `warning after threshold identical screen hashes`() {
        val state = StuckDetector.State()
        assertNull(detector.check(state, "tap", 100))
        assertNull(detector.check(state, "tap", 100))
        val warning = detector.check(state, "tap", 100)
        assertNotNull(warning)
        assertTrue(warning.contains("STUCK"))
        assertTrue(warning.contains("not changed in 3 actions"))
    }

    @Test
    fun `different screen hash resets stuck tracking`() {
        val state = StuckDetector.State()
        detector.check(state, "tap", 100)
        detector.check(state, "tap", 100)
        // Third hash is different — should not trigger warning
        assertNull(detector.check(state, "tap", 200))
    }

    @Test
    fun `consecutive waits trigger warning when screen is stuck`() {
        val state = StuckDetector.State()
        // See same screen twice to establish "screen is stuck"
        detector.check(state, "tap", 100)
        detector.check(state, "wait", 100)
        // Second wait with stuck screen triggers warning
        val warning = detector.check(state, "wait", 100)
        assertNotNull(warning)
        assertTrue(warning.contains("Waiting more won't help"))
    }

    @Test
    fun `non-wait tool resets consecutive wait counter`() {
        val state = StuckDetector.State()
        detector.check(state, "wait", 100)
        assertEquals(1, state.consecutiveWaits)
        detector.check(state, "tap", 100)
        assertEquals(0, state.consecutiveWaits)
    }

    @Test
    fun `null screenHash does not crash or trigger warnings`() {
        val state = StuckDetector.State()
        assertNull(detector.check(state, "think", null))
        assertNull(detector.check(state, "think", null))
        assertNull(detector.check(state, "think", null))
        assertEquals(0, state.uniqueScreens)
        assertTrue(state.recentScreenHashes.isEmpty())
    }

    @Test
    fun `uniqueScreens counts correctly`() {
        val state = StuckDetector.State()
        detector.check(state, "tap", 100)
        detector.check(state, "tap", 200)
        detector.check(state, "tap", 300)
        assertEquals(3, state.uniqueScreens)
    }

    @Test
    fun `screen stuck warning takes precedence over wait warning`() {
        val state = StuckDetector.State()
        detector.check(state, "wait", 100)
        detector.check(state, "wait", 100)
        // This triggers both: 3 same hashes AND 3 consecutive waits
        val warning = detector.check(state, "wait", 100)
        assertNotNull(warning)
        // Screen stuck warning should take precedence
        assertTrue(warning.contains("STUCK"))
    }

    @Test
    fun `warning includes unique screen count`() {
        val state = StuckDetector.State()
        detector.check(state, "tap", 100)  // unique 1
        detector.check(state, "tap", 200)  // unique 2
        detector.check(state, "tap", 200)
        val warning = detector.check(state, "tap", 200)
        assertNotNull(warning)
        assertTrue(warning.contains("2 unique screens"))
    }

    @Test
    fun `custom thresholds work`() {
        val custom = StuckDetector(screenThreshold = 2, waitThreshold = 1)
        val state = StuckDetector.State()
        assertNull(custom.check(state, "tap", 100))
        // Threshold of 2 — should trigger on second identical hash
        val warning = custom.check(state, "tap", 100)
        assertNotNull(warning)
        assertTrue(warning.contains("STUCK"))
    }

    @Test
    fun `fresh state does not carry over between loops`() {
        val state1 = StuckDetector.State()
        detector.check(state1, "tap", 100)
        detector.check(state1, "wait", 100)
        assertEquals(1, state1.consecutiveWaits)

        // New state for a new loop — starts clean
        val state2 = StuckDetector.State()
        assertEquals(0, state2.consecutiveWaits)
        assertEquals(0, state2.uniqueScreens)
        assertNull(detector.check(state2, "tap", 100))
    }
}
