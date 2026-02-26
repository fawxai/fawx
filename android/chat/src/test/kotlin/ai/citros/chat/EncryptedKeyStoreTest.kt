package ai.citros.chat

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.security.GeneralSecurityException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class EncryptedKeyStoreTest {

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    @Test
    fun `constructor throws when encrypted prefs init fails and fallback is disabled`() {
        assertFailsWith<GeneralSecurityException> {
            EncryptedKeyStore(
                context = context,
                allowPlaintextFallbackForTests = false,
                encryptedPrefsFactory = { throw GeneralSecurityException("keystore unavailable") },
                isRobolectricRuntime = { true }
            )
        }
    }

    @Test
    fun `constructor falls back to plaintext only when explicitly enabled for Robolectric`() {
        val keyStore = EncryptedKeyStore(
            context = context,
            allowPlaintextFallbackForTests = true,
            encryptedPrefsFactory = { throw GeneralSecurityException("keystore unavailable") },
            isRobolectricRuntime = { true }
        )

        runOnMainThread {
            keyStore.put("k1", "value")
            assertEquals("value", keyStore.get("k1"))
        }
        val fallbackPrefs = context.getSharedPreferences("citros_keystore_robolectric", Context.MODE_PRIVATE)
        assertEquals("value", fallbackPrefs.getString("k1", null))
    }

    @Test
    fun `constructor does not fallback outside Robolectric even when flag is enabled`() {
        assertFailsWith<GeneralSecurityException> {
            EncryptedKeyStore(
                context = context,
                allowPlaintextFallbackForTests = true,
                encryptedPrefsFactory = { throw GeneralSecurityException("keystore unavailable") },
                isRobolectricRuntime = { false }
            )
        }
    }

    @Test
    fun `constructor does not swallow unexpected runtime failures`() {
        assertFailsWith<IllegalArgumentException> {
            EncryptedKeyStore(
                context = context,
                allowPlaintextFallbackForTests = true,
                encryptedPrefsFactory = { throw IllegalArgumentException("bad input") },
                isRobolectricRuntime = { true }
            )
        }
    }

    private fun runOnMainThread(block: () -> Unit) {
        if (Looper.getMainLooper().isCurrentThread) {
            block()
            return
        }

        val latch = CountDownLatch(1)
        var failure: Throwable? = null
        Handler(Looper.getMainLooper()).post {
            try {
                block()
            } catch (t: Throwable) {
                failure = t
            } finally {
                latch.countDown()
            }
        }
        assertTrue(latch.await(5, TimeUnit.SECONDS), "Timed out waiting for main-thread execution")
        failure?.let { throw it }
    }
}
