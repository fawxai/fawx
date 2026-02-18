package ai.citros.core

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Manages extraction of bundled ONNX model assets to internal storage.
 *
 * Models are shipped inside the APK as assets and extracted on first launch
 * (or when the model version changes). Supports both STT and TTS model sets.
 *
 * **Note:** Model directories are computed eagerly at construction time, so the
 * [Context] must have a valid `filesDir` when this class is instantiated
 * (i.e., do not construct before `Application.onCreate` completes).
 */
class ModelManager(private val context: Context) {

    companion object {
        /** Asset sub-directory containing STT model files. */
        const val STT_ASSET_DIR = "sherpa-onnx-stt"

        /** Asset sub-directory containing TTS model files. */
        const val TTS_ASSET_DIR = "sherpa-onnx-tts"

        /** @deprecated Use [STT_ASSET_DIR] instead. */
        @Deprecated("Use STT_ASSET_DIR", replaceWith = ReplaceWith("STT_ASSET_DIR"))
        const val ASSET_DIR = STT_ASSET_DIR

        /** Bump when bundled STT model files change to trigger re-extraction. */
        const val MODEL_VERSION = "parakeet-tdt-0.6b-v2-int8-v1"

        /** Bump when bundled TTS model files change to trigger re-extraction. */
        const val TTS_MODEL_VERSION = "piper-lessac-high-v1"

        internal val REQUIRED_FILES = listOf(
            "silero_vad.onnx",
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt"
        )

        /** Top-level TTS model files (excludes espeak-ng-data directory tree). */
        internal val TTS_REQUIRED_FILES = listOf(
            "en_US-lessac-high.onnx",
            "en_US-lessac-high.onnx.json",
            "tokens.txt"
        )

        /** espeak-ng-data directory required by Piper VITS models. */
        internal const val ESPEAK_NG_DATA_DIR = "espeak-ng-data"

        private const val VERSION_FILE = ".version"
    }

    private val sttExtractionMutex = Mutex()
    private val ttsExtractionMutex = Mutex()

    /** Absolute path to the directory where STT models are extracted. */
    val modelDir: String =
        File(context.filesDir, "models/$STT_ASSET_DIR").absolutePath

    /** Absolute path to the directory where TTS models are extracted. */
    val ttsModelDir: String =
        File(context.filesDir, "models/$TTS_ASSET_DIR").absolutePath

    /** Returns `true` when all required STT files are present and the version marker matches. */
    val isExtracted: Boolean
        get() = REQUIRED_FILES.all { File(modelDir, it).exists() } &&
                versionMatches(modelDir, MODEL_VERSION)

    /** Returns `true` when all required TTS files are present and the version marker matches. */
    val isTtsExtracted: Boolean
        get() = TTS_REQUIRED_FILES.all { File(ttsModelDir, it).exists() } &&
                File(ttsModelDir, ESPEAK_NG_DATA_DIR).isDirectory &&
                versionMatches(ttsModelDir, TTS_MODEL_VERSION)

    /**
     * Extracts STT model assets to internal storage if not already present or if the
     * version has changed. Cleans up partial state on failure.
     *
     * Thread-safe: concurrent calls are serialized via a [Mutex].
     *
     * @return `true` if extraction was performed, `false` if models were already current.
     */
    suspend fun ensureExtracted(): Boolean = sttExtractionMutex.withLock {
        withContext(Dispatchers.IO) {
            if (isExtracted) return@withContext false
            extractAssetDir(STT_ASSET_DIR, modelDir, MODEL_VERSION)
            true
        }
    }

    /**
     * Extracts TTS model assets to internal storage if not already present or if the
     * version has changed. Handles the espeak-ng-data directory tree recursively.
     *
     * Thread-safe: concurrent calls are serialized via a [Mutex].
     *
     * @return `true` if extraction was performed, `false` if models were already current.
     */
    suspend fun ensureTtsExtracted(): Boolean = ttsExtractionMutex.withLock {
        withContext(Dispatchers.IO) {
            if (isTtsExtracted) return@withContext false
            extractAssetDirRecursive(TTS_ASSET_DIR, ttsModelDir, TTS_MODEL_VERSION)
            true
        }
    }

    /**
     * Extracts a flat asset directory (no subdirectories) to the target path.
     */
    private fun extractAssetDir(assetDir: String, targetPath: String, version: String) {
        val targetDir = File(targetPath)
        try {
            targetDir.mkdirs()
            val files = context.assets.list(assetDir) ?: emptyArray()
            for (filename in files) {
                context.assets.open("$assetDir/$filename").use { input ->
                    File(targetDir, filename).outputStream().use { output ->
                        input.copyTo(output)
                    }
                }
            }
            File(targetDir, VERSION_FILE).writeText(version)
        } catch (e: Exception) {
            targetDir.deleteRecursively()
            throw e
        }
    }

    /**
     * Recursively extracts an asset directory tree to the target path.
     * Handles nested directories like espeak-ng-data/.
     */
    private fun extractAssetDirRecursive(assetDir: String, targetPath: String, version: String) {
        val targetDir = File(targetPath)
        try {
            targetDir.mkdirs()
            extractAssetTreeRecursive(assetDir, targetDir)
            File(targetDir, VERSION_FILE).writeText(version)
        } catch (e: Exception) {
            targetDir.deleteRecursively()
            throw e
        }
    }

    /**
     * Recursively copies all files from an asset path into the target directory.
     * Directories are detected by listing their contents (files return empty/null).
     * Note: empty directories in assets also return empty arrays and would be treated
     * as files, causing an IOException. This is acceptable for the current model set
     * which has no empty directories.
     */
    private fun extractAssetTreeRecursive(assetPath: String, targetDir: File) {
        val children = context.assets.list(assetPath) ?: emptyArray()
        if (children.isEmpty()) {
            // It's a file — copy it
            context.assets.open(assetPath).use { input ->
                targetDir.outputStream().use { output ->
                    input.copyTo(output)
                }
            }
        } else {
            // It's a directory — recurse
            targetDir.mkdirs()
            for (child in children) {
                extractAssetTreeRecursive(
                    "$assetPath/$child",
                    File(targetDir, child)
                )
            }
        }
    }

    private fun versionMatches(dir: String, expectedVersion: String): Boolean {
        val versionFile = File(dir, VERSION_FILE)
        return versionFile.exists() && versionFile.readText().trim() == expectedVersion
    }
}
