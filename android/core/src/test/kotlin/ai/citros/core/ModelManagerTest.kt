package ai.citros.core

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class ModelManagerTest {

    private lateinit var context: Context
    private lateinit var manager: ModelManager

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        manager = ModelManager(context)
        // Clean up any leftover state
        File(manager.modelDir).deleteRecursively()
        File(manager.ttsModelDir).deleteRecursively()
    }

    @Test
    fun `modelDir returns correct path`() {
        val expected = File(context.filesDir, "models/${ModelManager.STT_ASSET_DIR}").absolutePath
        assertEquals(expected, manager.modelDir)
    }

    @Test
    fun `isExtracted returns false when directory does not exist`() {
        assertFalse(manager.isExtracted)
    }

    @Test
    fun `isExtracted returns false when version file missing`() {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        ModelManager.REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        assertFalse(manager.isExtracted)
    }

    @Test
    fun `isExtracted returns false when version mismatches`() {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        ModelManager.REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ".version").writeText("old-version")
        assertFalse(manager.isExtracted)
    }

    @Test
    fun `isExtracted returns true when all files present and version matches`() {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        ModelManager.REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ".version").writeText(ModelManager.MODEL_VERSION)
        assertTrue(manager.isExtracted)
    }

    @Test
    fun `ensureExtracted returns false when already extracted`() = runTest {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        ModelManager.REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ".version").writeText(ModelManager.MODEL_VERSION)
        assertFalse(manager.ensureExtracted())
    }

    @Test
    fun `isExtracted returns false when some files missing`() {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        // Only create first file
        File(dir, ModelManager.REQUIRED_FILES.first()).writeText("fake")
        File(dir, ".version").writeText(ModelManager.MODEL_VERSION)
        assertFalse(manager.isExtracted)
    }

    @Test
    fun `ensureExtracted cleans up directory on extraction failure`() = runTest {
        // Deterministic failure setup: create a file where the extraction directory should be.
        // Writing the version marker to <file>/.version throws, and extractAssetDir must clean up.
        val blockingFile = File(manager.modelDir)
        blockingFile.parentFile?.mkdirs()
        blockingFile.writeText("not-a-directory")

        try {
            manager.ensureExtracted()
            fail("Expected extraction failure when modelDir path is occupied by a file")
        } catch (_: Exception) {
            // Expected
        }

        assertFalse(
            "Partial extraction state should be cleaned up on failure",
            File(manager.modelDir).exists()
        )
    }

    @Test
    fun `ensureExtracted triggers re-extraction on version mismatch`() = runTest {
        val dir = File(manager.modelDir)
        dir.mkdirs()
        ModelManager.REQUIRED_FILES.forEach { File(dir, it).writeText("stale") }
        File(dir, ".version").writeText("old-version")

        // Version mismatch means isExtracted is false, so ensureExtracted will
        // attempt re-extraction. Robolectric's AssetManager returns empty list,
        // so it succeeds with 0 extracted files + new .version.
        // NOTE: This tests the re-extraction trigger, but not failure cleanup.
        // See `ensureExtracted cleans up directory on extraction failure` for that.
        val result = manager.ensureExtracted()
        assertTrue("Should re-extract on version mismatch", result)
        // Stale files should be replaced — directory recreated
        assertTrue(
            "Model directory should exist after re-extraction",
            File(manager.modelDir).exists()
        )
    }

    @Test
    fun `REQUIRED_FILES contains all expected model files`() {
        val expected = setOf(
            "silero_vad.onnx",
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt"
        )
        assertEquals(expected, ModelManager.REQUIRED_FILES.toSet())
    }

    // ── TTS model management tests ──

    @Test
    fun `ttsModelDir returns correct path`() {
        val expected = File(context.filesDir, "models/${ModelManager.TTS_ASSET_DIR}").absolutePath
        assertEquals(expected, manager.ttsModelDir)
    }

    @Test
    fun `isTtsExtracted returns false when directory does not exist`() {
        assertFalse(manager.isTtsExtracted)
    }

    @Test
    fun `isTtsExtracted returns false when version file missing`() {
        val dir = File(manager.ttsModelDir)
        dir.mkdirs()
        ModelManager.TTS_REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ModelManager.ESPEAK_NG_DATA_DIR).mkdirs()
        assertFalse(manager.isTtsExtracted)
    }

    @Test
    fun `isTtsExtracted returns false when espeak-ng-data missing`() {
        val dir = File(manager.ttsModelDir)
        dir.mkdirs()
        ModelManager.TTS_REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ".version").writeText(ModelManager.TTS_MODEL_VERSION)
        assertFalse(manager.isTtsExtracted)
    }

    @Test
    fun `isTtsExtracted returns true when all files present and version matches`() {
        val dir = File(manager.ttsModelDir)
        dir.mkdirs()
        ModelManager.TTS_REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ModelManager.ESPEAK_NG_DATA_DIR).mkdirs()
        File(dir, ".version").writeText(ModelManager.TTS_MODEL_VERSION)
        assertTrue(manager.isTtsExtracted)
    }

    @Test
    fun `ensureTtsExtracted returns false when already extracted`() = runTest {
        val dir = File(manager.ttsModelDir)
        dir.mkdirs()
        ModelManager.TTS_REQUIRED_FILES.forEach { File(dir, it).writeText("fake") }
        File(dir, ModelManager.ESPEAK_NG_DATA_DIR).mkdirs()
        File(dir, ".version").writeText(ModelManager.TTS_MODEL_VERSION)
        assertFalse(manager.ensureTtsExtracted())
    }

    @Test
    fun `ensureTtsExtracted cleans up on failure`() = runTest {
        try {
            manager.ensureTtsExtracted()
            fail("Expected exception — test APK has no bundled TTS model assets")
        } catch (_: Exception) {
            // Expected
        }
        assertFalse(
            "Partial TTS extraction should be cleaned up on failure",
            File(manager.ttsModelDir).exists()
        )
    }

    @Test
    fun `TTS_REQUIRED_FILES contains expected files`() {
        val expected = setOf(
            "en_US-lessac-high.onnx",
            "en_US-lessac-high.onnx.json",
            "tokens.txt"
        )
        assertEquals(expected, ModelManager.TTS_REQUIRED_FILES.toSet())
    }
}
