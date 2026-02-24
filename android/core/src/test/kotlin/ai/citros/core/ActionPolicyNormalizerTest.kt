package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ActionPolicyNormalizerTest {

    @Test
    fun `normalize app identifier prefers package then app name`() {
        assertEquals(
            "com.example.app",
            ActionPolicyNormalizer.normalizeAppIdentifier("  COM.EXAMPLE.APP  ", "Example")
        )
        assertEquals(
            "app_name:example app",
            ActionPolicyNormalizer.normalizeAppIdentifier("   ", " Example App ")
        )
        assertNull(ActionPolicyNormalizer.normalizeAppIdentifier("", "  "))
    }

    @Test
    fun `matches any package handles family matching and whitespace`() {
        val allowed = setOf("com.whatsapp", "org.telegram.messenger")
        assertTrue(ActionPolicyNormalizer.matchesAnyPackage(" com.whatsapp.beta ", allowed))
        assertTrue(ActionPolicyNormalizer.matchesAnyPackage("org.telegram.messenger.web", allowed))
        assertFalse(ActionPolicyNormalizer.matchesAnyPackage("com.signal", allowed))
    }
}
