package ai.citros.core

import org.junit.After
import org.junit.Assert.*
import org.junit.Test

/**
 * Unit tests for [FeatureFlags].
 */
class FeatureFlagsTest {

    @After
    fun cleanup() {
        FeatureFlags.resetToDefaults()
    }

    @Test
    fun `useServiceArchitecture defaults to true`() {
        assertTrue(FeatureFlags.useServiceArchitecture)
    }

    @Test
    fun `useServiceArchitecture can be toggled off`() {
        FeatureFlags.useServiceArchitecture = false
        assertFalse(FeatureFlags.useServiceArchitecture)
    }

    @Test
    fun `resetToDefaults restores useServiceArchitecture`() {
        FeatureFlags.useServiceArchitecture = false
        FeatureFlags.resetToDefaults()
        assertTrue(FeatureFlags.useServiceArchitecture)
    }
}
