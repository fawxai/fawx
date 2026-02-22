package ai.citros.chat

import ai.citros.core.ScreenReader
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class CitrosAccessibilityServiceTest {

    @Test
    fun `onServiceConnected atomically attaches ScreenReader with privacy list`() {
        ScreenReader.configurePrivacyList(null)
        val controller = Robolectric.buildService(CitrosAccessibilityService::class.java)

        try {
            val service = controller.create().get()
            val method = CitrosAccessibilityService::class.java.getDeclaredMethod("onServiceConnected")
            method.isAccessible = true
            method.invoke(service)
            assertTrue(ScreenReader.privacyList is SharedPrefsPrivacyList)
            assertTrue(ScreenReader.isAttached())
        } finally {
            ScreenReader.configurePrivacyList(null)
            ScreenReader.detach()
            controller.destroy()
        }
    }
}
