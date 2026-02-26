package ai.citros.core

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

@RunWith(RobolectricTestRunner::class)
class SttManagerTest {

    private lateinit var context: Context
    private lateinit var prefs: SharedPreferences

    @Before
    fun setUp() {
        context = RuntimeEnvironment.getApplication()
        prefs = context.getSharedPreferences("test_stt_manager_prefs", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
    }

    @Test
    fun `SherpaOnnx available returns SHERPA_READY`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = true)

        val state = manager.initialize()

        assertEquals(SttState.SHERPA_READY, state)
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `Sherpa fails Android available UNASKED returns AWAITING_CONSENT`() {
        val manager = managerWithMutableSherpa(
            androidAvailable = true,
            initialSherpaSuccess = false,
            consent = CloudSttConsent.UNASKED
        )

        assertEquals(SttState.AWAITING_CONSENT, manager.initialize())
    }

    @Test
    fun `Sherpa fails Android available ACCEPTED returns ANDROID_FALLBACK`() {
        val manager = managerWithMutableSherpa(
            androidAvailable = true,
            initialSherpaSuccess = false,
            consent = CloudSttConsent.ACCEPTED
        )

        assertEquals(SttState.ANDROID_FALLBACK, manager.initialize())
        assertTrue(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `Sherpa fails Android available DECLINED_TYPE returns FAILED`() {
        val manager = managerWithMutableSherpa(
            androidAvailable = true,
            initialSherpaSuccess = false,
            consent = CloudSttConsent.DECLINED_TYPE
        )

        assertEquals(SttState.FAILED, manager.initialize())
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `Sherpa fails Android unavailable returns FAILED`() {
        val manager = managerWithMutableSherpa(androidAvailable = false, initialSherpaSuccess = false)

        assertEquals(SttState.FAILED, manager.initialize())
    }

    @Test
    fun `handleConsentChoice ACCEPTED returns ANDROID_FALLBACK`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)
        manager.initialize()

        assertEquals(SttState.ANDROID_FALLBACK, manager.handleConsentChoice(CloudSttConsent.ACCEPTED))
        assertEquals(CloudSttConsent.ACCEPTED, manager.cloudConsent)
        assertTrue(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `handleConsentChoice DECLINED_WAIT returns FAILED and hides cloud indicator`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)
        manager.initialize()

        assertEquals(SttState.FAILED, manager.handleConsentChoice(CloudSttConsent.DECLINED_WAIT))
        assertEquals(CloudSttConsent.DECLINED_WAIT, manager.cloudConsent)
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `handleConsentChoice DECLINED_TYPE returns FAILED and hides cloud indicator`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)
        manager.initialize()

        assertEquals(SttState.FAILED, manager.handleConsentChoice(CloudSttConsent.DECLINED_TYPE))
        assertEquals(CloudSttConsent.DECLINED_TYPE, manager.cloudConsent)
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `handleConsentChoice UNASKED throws`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)

        try {
            manager.handleConsentChoice(CloudSttConsent.UNASKED)
            throw AssertionError("Expected IllegalArgumentException")
        } catch (e: IllegalArgumentException) {
            assertEquals("UNASKED is not a valid consent choice", e.message)
        }
    }

    @Test
    fun `consent persisted across SttManager reinitialization`() {
        val manager1 = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)
        manager1.handleConsentChoice(CloudSttConsent.ACCEPTED)

        val manager2 = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)
        assertEquals(CloudSttConsent.ACCEPTED, manager2.cloudConsent)
        assertEquals(SttState.ANDROID_FALLBACK, manager2.initialize())
    }

    @Test
    fun `corrupted consent value falls back to UNASKED`() {
        prefs.edit().putString(SttManager.KEY_CLOUD_STT_CONSENT, "CORRUPTED").commit()

        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)

        assertEquals(CloudSttConsent.UNASKED, manager.cloudConsent)
        assertEquals(SttState.AWAITING_CONSENT, manager.initialize())
    }

    @Test
    fun `onModelsDownloaded upgrades ANDROID_FALLBACK to SHERPA_READY`() {
        val fixture = managerFixture(androidAvailable = true, initialSherpaSuccess = false, consent = CloudSttConsent.ACCEPTED)
        val manager = fixture.manager
        assertEquals(SttState.ANDROID_FALLBACK, manager.initialize())
        assertTrue(manager.cloudIndicatorVisible.value)

        fixture.sherpaSucceeds = true

        assertEquals(SttState.SHERPA_READY, manager.onModelsDownloaded())
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `onModelsDownloaded upgrades AWAITING_CONSENT to SHERPA_READY`() {
        val fixture = managerFixture(androidAvailable = true, initialSherpaSuccess = false, consent = CloudSttConsent.UNASKED)
        val manager = fixture.manager
        assertEquals(SttState.AWAITING_CONSENT, manager.initialize())

        fixture.sherpaSucceeds = true

        assertEquals(SttState.SHERPA_READY, manager.onModelsDownloaded())
        assertFalse(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `onModelsDownloaded upgrades FAILED to SHERPA_READY`() {
        val fixture = managerFixture(androidAvailable = false, initialSherpaSuccess = false)
        val manager = fixture.manager
        assertEquals(SttState.FAILED, manager.initialize())

        fixture.sherpaSucceeds = true

        assertEquals(SttState.SHERPA_READY, manager.onModelsDownloaded())
    }

    @Test
    fun `onModelsDownloaded during INITIALIZING is no-op`() {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)

        assertEquals(SttState.INITIALIZING, manager.state.value)
        assertEquals(SttState.INITIALIZING, manager.onModelsDownloaded())
        assertEquals(SttState.INITIALIZING, manager.state.value)
    }

    @Test
    fun `onModelsDownloaded when reinit fails keeps current state`() {
        val fixture = managerFixture(androidAvailable = true, initialSherpaSuccess = false, consent = CloudSttConsent.ACCEPTED)
        val manager = fixture.manager
        assertEquals(SttState.ANDROID_FALLBACK, manager.initialize())

        fixture.sherpaSucceeds = false

        assertEquals(SttState.ANDROID_FALLBACK, manager.onModelsDownloaded())
        assertTrue(manager.cloudIndicatorVisible.value)
    }

    @Test
    fun `startListening dispatches to correct engine per state`() {
        val callback = RecordingCallback()

        val sherpaFixture = managerFixture(androidAvailable = true, initialSherpaSuccess = true)
        val sherpaManager = sherpaFixture.manager
        sherpaManager.initialize()
        sherpaManager.startListening(callback)
        assertEquals(1, sherpaFixture.sherpaCalls)
        assertEquals(0, sherpaFixture.androidCalls)
        assertEquals(listOf("partial"), callback.partialResults)
        assertEquals(listOf("final"), callback.results)

        val androidFixture = managerFixture(
            androidAvailable = true,
            initialSherpaSuccess = false,
            consent = CloudSttConsent.ACCEPTED
        )
        val androidManager = androidFixture.manager
        androidManager.initialize()
        androidManager.startListening(callback)
        assertEquals(0, androidFixture.sherpaCalls)
        assertEquals(1, androidFixture.androidCalls)
        assertEquals(listOf("partial", "a-partial"), callback.partialResults)
        assertEquals(listOf("final", "a-final"), callback.results)

        val consentFixture = managerFixture(
            androidAvailable = true,
            initialSherpaSuccess = false,
            consent = CloudSttConsent.UNASKED
        )
        val consentManager = consentFixture.manager
        consentManager.initialize()
        consentManager.startListening(callback)
        assertTrue(callback.lastError?.contains("consent") == true)
    }

    @Test
    fun `startListening in FAILED calls onError`() {
        val callback = RecordingCallback()
        val manager = managerWithMutableSherpa(androidAvailable = false, initialSherpaSuccess = false)
        manager.initialize()

        manager.startListening(callback)

        assertEquals("No speech recognition available", callback.lastError)
    }

    @Test
    fun `startListening in INITIALIZING calls onError`() {
        val callback = RecordingCallback()
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = false)

        manager.startListening(callback)

        assertEquals("STT not initialized", callback.lastError)
    }

    @Test
    fun `initialize kicks off model download when sherpa unavailable`() {
        val fixture = managerFixture(androidAvailable = true, initialSherpaSuccess = false)

        fixture.manager.initialize()

        assertTrue(fixture.downloadKickedOff)
    }

    @Test
    fun `stateflow emits INITIALIZING before final state during initialize`() = runBlocking {
        val manager = managerWithMutableSherpa(androidAvailable = true, initialSherpaSuccess = true)
        val observed = mutableListOf<SttState>()
        val job = launch {
            manager.state.take(2).toList(observed)
        }

        manager.initialize()
        job.join()

        assertEquals(listOf(SttState.INITIALIZING, SttState.SHERPA_READY), observed)
    }

    private fun managerWithMutableSherpa(
        androidAvailable: Boolean,
        initialSherpaSuccess: Boolean,
        consent: CloudSttConsent = CloudSttConsent.UNASKED,
    ): SttManager = managerFixture(androidAvailable, initialSherpaSuccess, consent).manager

    private fun managerFixture(
        androidAvailable: Boolean,
        initialSherpaSuccess: Boolean,
        consent: CloudSttConsent = CloudSttConsent.UNASKED,
    ): Fixture {
        prefs.edit().putString(SttManager.KEY_CLOUD_STT_CONSENT, consent.name).commit()
        val fixture = Fixture(sherpaSucceeds = initialSherpaSuccess)
        fixture.manager = SttManager(
            prefs = prefs,
            initializeSherpa = {
                if (!fixture.sherpaSucceeds) throw IllegalStateException("sherpa unavailable")
            },
            isAndroidSttAvailable = { androidAvailable },
            startSherpaListening = { callback ->
                fixture.sherpaCalls += 1
                callback.onPartialResult("partial")
                callback.onResult("final")
            },
            startAndroidListening = { callback ->
                fixture.androidCalls += 1
                callback.onPartialResult("a-partial")
                callback.onResult("a-final")
            },
            kickOffModelDownload = { fixture.downloadKickedOff = true },
        )
        return fixture
    }

    private class Fixture(
        var sherpaSucceeds: Boolean,
        var sherpaCalls: Int = 0,
        var androidCalls: Int = 0,
        var downloadKickedOff: Boolean = false,
    ) {
        lateinit var manager: SttManager
    }

    private class RecordingCallback : SttCallback {
        var lastError: String? = null
        val results = mutableListOf<String>()
        val partialResults = mutableListOf<String>()

        override fun onResult(text: String) {
            results += text
        }

        override fun onPartialResult(text: String) {
            partialResults += text
        }

        override fun onError(message: String) {
            lastError = message
        }
    }
}
