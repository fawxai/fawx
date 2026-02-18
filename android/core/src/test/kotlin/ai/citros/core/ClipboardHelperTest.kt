package ai.citros.core

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference

/**
 * Tests for ClipboardHelper.
 * Uses Robolectric for Android ClipboardManager access.
 */
@RunWith(RobolectricTestRunner::class)
class ClipboardHelperTest {

    private lateinit var clipboardManager: ClipboardManager

    @Before
    fun setUp() {
        val context = RuntimeEnvironment.getApplication()
        ClipboardHelper.attach(context)
        clipboardManager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }

    @After
    fun tearDown() {
        ClipboardHelper.detach()
    }

    // ========== Clipboard Read Tests (#339) ==========

    @Test
    fun `read returns null when clipboard is empty`() {
        // Fresh clipboard should be empty
        clipboardManager.clearPrimaryClip()
        val result = ClipboardHelper.read()
        assertNull(result)
    }

    @Test
    fun `read returns text from clipboard`() {
        clipboardManager.setPrimaryClip(ClipData.newPlainText("test", "Hello World"))
        val result = ClipboardHelper.read()
        assertEquals("Hello World", result)
    }

    @Test
    fun `read returns null when not attached`() {
        ClipboardHelper.detach()
        val result = ClipboardHelper.read()
        assertNull(result)
    }

    // ========== Clipboard Write Tests (#339) ==========

    @Test
    fun `write places text on clipboard`() {
        val success = ClipboardHelper.write("Test content")
        assertTrue(success)

        val clip = clipboardManager.primaryClip
        assertNotNull(clip)
        assertEquals("Test content", clip?.getItemAt(0)?.text?.toString())
    }

    @Test
    fun `write with custom label`() {
        val success = ClipboardHelper.write("Content", label = "MyLabel")
        assertTrue(success)

        val clip = clipboardManager.primaryClip
        assertNotNull(clip)
        assertEquals("MyLabel", clip?.description?.label?.toString())
    }

    @Test
    fun `write returns false when not attached`() {
        ClipboardHelper.detach()
        val success = ClipboardHelper.write("Test")
        assertFalse(success)
    }

    @Test
    fun `write overwrites existing clipboard content`() {
        ClipboardHelper.write("First")
        ClipboardHelper.write("Second")

        val result = ClipboardHelper.read()
        assertEquals("Second", result)
    }

    // ========== Attach/Detach Tests (#339) ==========

    @Test
    fun `isAttached returns true when attached`() {
        assertTrue(ClipboardHelper.isAttached())
    }

    @Test
    fun `isAttached returns false when detached`() {
        ClipboardHelper.detach()
        assertFalse(ClipboardHelper.isAttached())
    }

    @Test
    fun `read and write round-trip preserves text`() {
        val testText = "Unicode: 🎉 Ñoño café"
        ClipboardHelper.write(testText)
        val result = ClipboardHelper.read()
        assertEquals(testText, result)
    }

    @Test
    fun `write handles empty string`() {
        val success = ClipboardHelper.write("")
        assertTrue(success)
        val result = ClipboardHelper.read()
        assertEquals("", result)
    }

    // ========== Paste Failure Mode Tests (#339) ==========

    @Test
    fun `writeAndPaste returns false when accessibility service detached`() {
        ScreenReader.detach()
        // write() succeeds (clipboard is attached), but paste fails (no accessibility service)
        val result = ClipboardHelper.writeAndPaste("test")
        assertFalse(result)
        // Verify text was still written to clipboard despite paste failure
        assertEquals("test", ClipboardHelper.read())
    }

    @Test
    fun `writeAndPaste returns false when clipboard detached`() {
        ClipboardHelper.detach()
        val result = ClipboardHelper.writeAndPaste("test")
        assertFalse(result)
    }

    @Test
    fun `write handles long text`() {
        val longText = "x".repeat(10_000)
        val success = ClipboardHelper.write(longText)
        assertTrue(success)
        val result = ClipboardHelper.read()
        assertEquals(longText, result)
    }

    // ========== Clipboard Listener Tests (#354) ==========

    @Test
    fun `startListening registers callback and fires on clipboard change`() {
        val received = mutableListOf<String?>()
        ClipboardHelper.startListening { received.add(it) }

        assertTrue(ClipboardHelper.isListening())
        clipboardManager.setPrimaryClip(ClipData.newPlainText("test", "listener test"))

        assertEquals(1, received.size)
        assertEquals("listener test", received[0])
    }

    @Test
    fun `stopListening removes listener and prevents further callbacks`() {
        val received = mutableListOf<String?>()
        ClipboardHelper.startListening { received.add(it) }
        ClipboardHelper.stopListening()

        assertFalse(ClipboardHelper.isListening())
        clipboardManager.setPrimaryClip(ClipData.newPlainText("test", "after stop"))

        assertTrue(received.isEmpty())
    }

    @Test
    fun `isListening reflects correct state`() {
        assertFalse(ClipboardHelper.isListening())

        ClipboardHelper.startListening { }
        assertTrue(ClipboardHelper.isListening())

        ClipboardHelper.stopListening()
        assertFalse(ClipboardHelper.isListening())
    }

    @Test
    fun `detach calls stopListening automatically`() {
        ClipboardHelper.startListening { }
        assertTrue(ClipboardHelper.isListening())

        ClipboardHelper.detach()
        assertFalse(ClipboardHelper.isListening())
        assertFalse(ClipboardHelper.isAttached())
    }

    @Test
    fun `second startListening replaces previous listener`() {
        val first = mutableListOf<String?>()
        val second = mutableListOf<String?>()

        ClipboardHelper.startListening { first.add(it) }
        ClipboardHelper.startListening { second.add(it) }

        clipboardManager.setPrimaryClip(ClipData.newPlainText("test", "replaced"))

        assertTrue(first.isEmpty())
        assertEquals(1, second.size)
        assertEquals("replaced", second[0])
    }

    @Test
    fun `startListening with no context is a no-op`() {
        ClipboardHelper.detach()
        ClipboardHelper.startListening { }
        assertFalse(ClipboardHelper.isListening())
    }

    @Test
    fun `listener exception does not crash clipboard write`() {
        ClipboardHelper.startListening { throw RuntimeException("listener failure") }

        val success = ClipboardHelper.write("still works")

        assertTrue(success)
        // Listener remains registered despite callback failure
        assertTrue(ClipboardHelper.isListening())
    }

    @Test
    fun `startListening and stopListening are thread-safe`() {
        val firstError = AtomicReference<Throwable?>(null)
        val done = CountDownLatch(2)

        val t1 = Thread {
            try {
                repeat(200) {
                    ClipboardHelper.startListening { }
                }
            } catch (t: Throwable) {
                firstError.compareAndSet(null, t)
            } finally {
                done.countDown()
            }
        }

        val t2 = Thread {
            try {
                repeat(200) {
                    ClipboardHelper.stopListening()
                }
            } catch (t: Throwable) {
                firstError.compareAndSet(null, t)
            } finally {
                done.countDown()
            }
        }

        t1.start()
        t2.start()
        done.await()

        assertNull(firstError.get())
    }

    @Test
    fun `startListening and detach are thread-safe`() {
        val firstError = AtomicReference<Throwable?>(null)
        val done = CountDownLatch(2)

        val t1 = Thread {
            try {
                repeat(200) {
                    ClipboardHelper.startListening { }
                }
            } catch (t: Throwable) {
                firstError.compareAndSet(null, t)
            } finally {
                done.countDown()
            }
        }

        val t2 = Thread {
            try {
                repeat(200) {
                    ClipboardHelper.detach()
                    ClipboardHelper.attach(RuntimeEnvironment.getApplication())
                }
            } catch (t: Throwable) {
                firstError.compareAndSet(null, t)
            } finally {
                done.countDown()
            }
        }

        t1.start()
        t2.start()
        done.await()

        assertNull(firstError.get())
    }
}
