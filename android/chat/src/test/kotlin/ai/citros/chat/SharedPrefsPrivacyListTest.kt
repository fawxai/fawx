package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import androidx.test.core.app.ApplicationProvider
import ai.citros.core.ScreenReader
import org.mockito.kotlin.any
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.whenever
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.shadows.ShadowLog
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class SharedPrefsPrivacyListTest {

    private lateinit var context: Context
    private lateinit var list: SharedPrefsPrivacyList

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        clearPrivacyListPrefs()
        list = SharedPrefsPrivacyList(
            prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE),
            isMainThread = { false },
            isDebugBuild = { true }
        )
    }

    private fun clearPrivacyListPrefs() {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
            .edit()
            .remove(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST)
            .commit()
    }

    @Test
    fun `default list is empty`() {
        assertTrue(list.getAll().isEmpty())
        assertFalse(list.isPrivate("com.bank.app"))
    }

    @Test
    fun `add and remove package persists in shared preferences`() {
        list.add("com.bank.app")
        assertTrue(list.isPrivate("com.bank.app"))

        val reloaded = SharedPrefsPrivacyList(
            prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE),
            isMainThread = { false },
            isDebugBuild = { true }
        )
        assertTrue(reloaded.isPrivate("com.bank.app"))
        assertEquals(setOf("com.bank.app"), reloaded.getAll())

        reloaded.remove("com.bank.app")
        assertFalse(reloaded.isPrivate("com.bank.app"))
        assertTrue(reloaded.getAll().isEmpty())
    }

    @Test
    fun `ChatActivity helper configures ScreenReader with SharedPrefsPrivacyList`() {
        ScreenReader.configurePrivacyList(null)

        try {
            ChatActivity.configureScreenReaderPrivacyList(context)
            assertTrue(ScreenReader.privacyList is SharedPrefsPrivacyList)
        } finally {
            ScreenReader.configurePrivacyList(null)
        }
    }

    @Test
    fun `ChatActivity onCreate wires privacy list configuration`() {
        ScreenReader.configurePrivacyList(null)
        walletDependenciesFactoryForTests = { appContext ->
            createTestWalletDependencies(appContext.applicationContext)
        }

        val controller = Robolectric.buildActivity(ChatActivity::class.java)
        try {
            controller.setup()
            assertTrue(ScreenReader.privacyList is SharedPrefsPrivacyList)
        } finally {
            walletDependenciesFactoryForTests = null
            controller.pause().stop().destroy()
            ScreenReader.configurePrivacyList(null)
        }
    }

    @Test
    fun `concurrent adds preserve all entries`() {
        repeat(20) { round ->
            clearPrivacyListPrefs()
            val workerCount = 24
            val startGate = CountDownLatch(1)
            val doneGate = CountDownLatch(workerCount)
            val executor = Executors.newFixedThreadPool(8)

            try {
                repeat(workerCount) { index ->
                    executor.execute {
                        startGate.await()
                        list.add("com.concurrent.add.$index")
                        doneGate.countDown()
                    }
                }

                startGate.countDown()
                assertTrue(doneGate.await(5, TimeUnit.SECONDS), "workers timed out in round $round")
            } finally {
                executor.shutdownNow()
            }

            assertEquals(
                workerCount,
                list.getAll().size,
                "lost updates detected in add round $round"
            )
        }
    }

    @Test
    fun `concurrent adds across multiple instances preserve all entries`() {
        repeat(30) { round ->
            clearPrivacyListPrefs()
            val listA = SharedPrefsPrivacyList(context)
            val listB = SharedPrefsPrivacyList(context)
            val workerCount = 32
            val startGate = CountDownLatch(1)
            val doneGate = CountDownLatch(workerCount)
            val executor = Executors.newFixedThreadPool(8)

            try {
                repeat(workerCount) { index ->
                    executor.execute {
                        startGate.await()
                        val writer = if (index % 2 == 0) listA else listB
                        writer.add("com.concurrent.multi.$index")
                        doneGate.countDown()
                    }
                }

                startGate.countDown()
                assertTrue(doneGate.await(5, TimeUnit.SECONDS), "workers timed out in round $round")
            } finally {
                executor.shutdownNow()
            }

            assertEquals(
                workerCount,
                SharedPrefsPrivacyList(context).getAll().size,
                "lost updates detected for multi-instance add round $round"
            )
        }
    }

    @Test
    fun `concurrent removes clear every entry`() {
        val workerCount = 24
        repeat(workerCount) { index ->
            list.add("com.concurrent.remove.$index")
        }

        repeat(20) { round ->
            val startGate = CountDownLatch(1)
            val doneGate = CountDownLatch(workerCount)
            val executor = Executors.newFixedThreadPool(8)

            try {
                repeat(workerCount) { index ->
                    executor.execute {
                        startGate.await()
                        list.remove("com.concurrent.remove.$index")
                        doneGate.countDown()
                    }
                }

                startGate.countDown()
                assertTrue(doneGate.await(5, TimeUnit.SECONDS), "workers timed out in round $round")
            } finally {
                executor.shutdownNow()
            }

            assertTrue(
                list.getAll().isEmpty(),
                "lost updates detected in remove round $round: ${list.getAll()}"
            )
            if (round < 19) {
                repeat(workerCount) { index ->
                    list.add("com.concurrent.remove.$index")
                }
            }
        }
    }

    @Test
    fun `concurrent removes across multiple instances clear every entry`() {
        val workerCount = 32
        repeat(workerCount) { index ->
            list.add("com.concurrent.multi.remove.$index")
        }

        repeat(30) { round ->
            val listA = SharedPrefsPrivacyList(context)
            val listB = SharedPrefsPrivacyList(context)
            val startGate = CountDownLatch(1)
            val doneGate = CountDownLatch(workerCount)
            val executor = Executors.newFixedThreadPool(8)

            try {
                repeat(workerCount) { index ->
                    executor.execute {
                        startGate.await()
                        val writer = if (index % 2 == 0) listA else listB
                        writer.remove("com.concurrent.multi.remove.$index")
                        doneGate.countDown()
                    }
                }

                startGate.countDown()
                assertTrue(doneGate.await(5, TimeUnit.SECONDS), "workers timed out in round $round")
            } finally {
                executor.shutdownNow()
            }

            assertTrue(
                SharedPrefsPrivacyList(context).getAll().isEmpty(),
                "lost updates detected in multi-instance remove round $round"
            )
            if (round < 29) {
                repeat(workerCount) { index ->
                    list.add("com.concurrent.multi.remove.$index")
                }
            }
        }
    }

    @Test
    fun `add throws in debug mode when called on main thread`() {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val strictList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { true },
            isDebugBuild = { true }
        )

        assertFailsWith<IllegalStateException> {
            strictList.add("com.debug.mainthread")
        }
    }

    @Test
    fun `add only warns in non-debug mode when called on main thread`() {
        ShadowLog.reset()
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        val nonDebugList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { true },
            isDebugBuild = { false }
        )

        nonDebugList.add("com.nondebug.mainthread")
        val warnLog = ShadowLog.getLogsForTag("CitrosPrivacyList")
            .lastOrNull { it.type == android.util.Log.WARN }

        assertTrue(warnLog != null, "expected warning log on non-debug main thread write")
    }

    @Test
    fun `remove throws in debug mode when called on main thread`() {
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        prefs.edit()
            .putStringSet(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST, setOf("com.debug.mainthread"))
            .commit()

        val strictList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { true },
            isDebugBuild = { true }
        )

        assertFailsWith<IllegalStateException> {
            strictList.remove("com.debug.mainthread")
        }
    }

    @Test
    fun `remove only warns in non-debug mode when called on main thread`() {
        ShadowLog.reset()
        val prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
        prefs.edit()
            .putStringSet(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST, setOf("com.nondebug.mainthread"))
            .commit()

        val nonDebugList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { true },
            isDebugBuild = { false }
        )

        nonDebugList.remove("com.nondebug.mainthread")
        val warnLog = ShadowLog.getLogsForTag("CitrosPrivacyList")
            .lastOrNull { it.type == android.util.Log.WARN }

        assertTrue(warnLog != null, "expected warning log on non-debug main thread write")
    }

    @Test
    fun `add throws when shared preferences commit fails`() {
        ShadowLog.reset()
        val prefs = mock<SharedPreferences>()
        val editor = mock<SharedPreferences.Editor>()
        whenever(prefs.getStringSet(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST, emptySet()))
            .thenReturn(emptySet())
        whenever(prefs.edit()).thenReturn(editor)
        whenever(editor.putStringSet(eq(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST), any<Set<String>>()))
            .thenReturn(editor)
        whenever(editor.commit()).thenReturn(false)

        val failingList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { false },
            isDebugBuild = { true }
        )
        assertFailsWith<IllegalStateException> {
            failingList.add("com.bank.app")
        }

        val logs = ShadowLog.getLogsForTag("CitrosPrivacyList")
        val errorLog = logs.lastOrNull { it.type == android.util.Log.ERROR }
        assertTrue(errorLog != null, "expected error log on commit failure")
        assertFalse(
            errorLog!!.msg.contains("com.bank.app"),
            "error logs must redact package names"
        )
    }

    @Test
    fun `remove throws when shared preferences commit fails`() {
        ShadowLog.reset()
        val prefs = mock<SharedPreferences>()
        val editor = mock<SharedPreferences.Editor>()
        whenever(prefs.getStringSet(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST, emptySet()))
            .thenReturn(setOf("com.bank.app"))
        whenever(prefs.edit()).thenReturn(editor)
        whenever(editor.putStringSet(eq(SharedPrefsPrivacyList.KEY_PRIVACY_APP_LIST), any<Set<String>>()))
            .thenReturn(editor)
        whenever(editor.commit()).thenReturn(false)

        val failingList = SharedPrefsPrivacyList(
            prefs = prefs,
            isMainThread = { false },
            isDebugBuild = { true }
        )
        assertFailsWith<IllegalStateException> {
            failingList.remove("com.bank.app")
        }

        val logs = ShadowLog.getLogsForTag("CitrosPrivacyList")
        val errorLog = logs.lastOrNull { it.type == android.util.Log.ERROR }
        assertTrue(errorLog != null, "expected error log on commit failure")
        assertFalse(
            errorLog!!.msg.contains("com.bank.app"),
            "error logs must redact package names"
        )
    }
}
