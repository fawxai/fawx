package ai.citros.chat

import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
import org.junit.Test
import org.mockito.Mockito.mock
import org.mockito.Mockito.`when`
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Unit tests for OverlayPermission.
 *
 * Requires Robolectric for Android framework APIs (Uri.parse, Intent construction)
 * that aren't available in standard JVM unit tests.
 */
@RunWith(RobolectricTestRunner::class)
class OverlayPermissionTest {

    @Test
    fun `buildPermissionIntent returns intent with correct action`() {
        val context = mock(Context::class.java)
        `when`(context.packageName).thenReturn("ai.citros.chat")

        val intent = OverlayPermission.buildPermissionIntent(context)

        assertNotNull(intent)
        // On API 23+ (which unit tests run on), should be manage overlay permission
        assertEquals(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, intent.action)
        assertNotNull(intent.data)
        assertEquals("package:ai.citros.chat", intent.data.toString())
    }

    @Test
    fun `buildPermissionIntent data includes package name`() {
        val context = mock(Context::class.java)
        `when`(context.packageName).thenReturn("com.example.test")

        val intent = OverlayPermission.buildPermissionIntent(context)

        assertTrue(intent.data.toString().contains("com.example.test"))
    }
}
