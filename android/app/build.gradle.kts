plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val sttCacheDir = "$rootDir/models-cache/sherpa-onnx-stt"
val ttsCacheDir = "$rootDir/models-cache/sherpa-onnx-tts"

data class ModelFile(
    val url: String,
    val filename: String,
    val expectedSize: Long
)

val sherpaModelFiles = listOf(
    ModelFile(
        url = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main/encoder.int8.onnx",
        filename = "encoder.int8.onnx",
        expectedSize = 652_184_296L
    ),
    ModelFile(
        url = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main/decoder.int8.onnx",
        filename = "decoder.int8.onnx",
        expectedSize = 7_257_753L
    ),
    ModelFile(
        url = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main/joiner.int8.onnx",
        filename = "joiner.int8.onnx",
        expectedSize = 1_739_080L
    ),
    ModelFile(
        url = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main/tokens.txt",
        filename = "tokens.txt",
        expectedSize = 9_384L
    ),
    ModelFile(
        url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        filename = "silero_vad.onnx",
        expectedSize = 643_854L
    )
)

val ttsModelFiles = listOf(
    ModelFile(
        url = "https://huggingface.co/csukuangfj/vits-piper-en_US-lessac-high/resolve/main/en_US-lessac-high.onnx",
        filename = "en_US-lessac-high.onnx",
        expectedSize = 114_005_053L
    ),
    ModelFile(
        url = "https://huggingface.co/csukuangfj/vits-piper-en_US-lessac-high/resolve/main/en_US-lessac-high.onnx.json",
        filename = "en_US-lessac-high.onnx.json",
        expectedSize = 5_046L
    ),
    ModelFile(
        url = "https://huggingface.co/csukuangfj/vits-piper-en_US-lessac-high/resolve/main/tokens.txt",
        filename = "tokens.txt",
        expectedSize = 2_945L
    ),
    ModelFile(
        url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-piper-en_US-lessac-high.tar.bz2",
        filename = "vits-piper-en_US-lessac-high.tar.bz2",
        expectedSize = 114_292_230L
    )
)

tasks.register("downloadSherpaModels") {
    description = "Downloads Sherpa ONNX model files for speech-to-text and text-to-speech"
    group = "setup"

    outputs.dir(sttCacheDir)
    outputs.dir(ttsCacheDir)

    doLast {
        // --- STT models ---
        val sttDir = file(sttCacheDir)
        sttDir.mkdirs()

        sherpaModelFiles.forEach { model ->
            val targetFile = File(sttDir, model.filename)
            if (targetFile.exists() && targetFile.length() == model.expectedSize) {
                logger.lifecycle("  ✓ ${model.filename} already cached (${targetFile.length()} bytes)")
                return@forEach
            }
            logger.lifecycle("  ⬇ Downloading ${model.filename} ...")
            exec {
                commandLine("curl", "-fSL", "--retry", "3", "-o", targetFile.absolutePath, model.url)
            }
            check(targetFile.exists() && targetFile.length() == model.expectedSize) {
                "Download failed for ${model.filename}: expected ${model.expectedSize} bytes, got ${targetFile.length()}"
            }
            logger.lifecycle("  ✓ ${model.filename} downloaded (${targetFile.length()} bytes)")
        }

        // --- TTS models ---
        val ttsDir = file(ttsCacheDir)
        ttsDir.mkdirs()

        ttsModelFiles.forEach { model ->
            val targetFile = File(ttsDir, model.filename)
            if (targetFile.exists() && targetFile.length() == model.expectedSize) {
                logger.lifecycle("  ✓ ${model.filename} already cached (${targetFile.length()} bytes)")
                return@forEach
            }
            logger.lifecycle("  ⬇ Downloading ${model.filename} ...")
            exec {
                commandLine("curl", "-fSL", "--retry", "3", "-o", targetFile.absolutePath, model.url)
            }
            check(targetFile.exists() && targetFile.length() == model.expectedSize) {
                "Download failed for ${model.filename}: expected ${model.expectedSize} bytes, got ${targetFile.length()}"
            }
            logger.lifecycle("  ✓ ${model.filename} downloaded (${targetFile.length()} bytes)")
        }

        // Extract espeak-ng-data from tar.bz2
        val espeakDir = File(ttsDir, "espeak-ng-data")
        if (!espeakDir.isDirectory) {
            val tarFile = File(ttsDir, "vits-piper-en_US-lessac-high.tar.bz2")
            if (tarFile.exists()) {
                logger.lifecycle("  ⚙ Extracting espeak-ng-data from tar.bz2...")
                exec {
                    commandLine(
                        "tar", "xjf", tarFile.absolutePath,
                        "--strip-components=1",
                        "-C", ttsDir.absolutePath,
                        "vits-piper-en_US-lessac-high/espeak-ng-data"
                    )
                }
                check(espeakDir.isDirectory) {
                    "Failed to extract espeak-ng-data directory"
                }
                logger.lifecycle("  ✓ espeak-ng-data extracted")
            }
        } else {
            logger.lifecycle("  ✓ espeak-ng-data already extracted")
        }
    }
}

if (project.hasProperty("downloadModels")) {
    tasks.named("preBuild") {
        dependsOn("downloadSherpaModels")
    }
}

android {
    namespace = "ai.citros.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "ai.citros.app"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets {
        getByName("main") {
            assets.srcDirs("src/main/assets", "$rootDir/models-cache")
        }
    }

    aaptOptions {
        // Don't compress model files — they're already compressed and
        // aapt2 re-compression wastes CPU + breaks mmap-based loading.
        noCompress += listOf("onnx", "json")
    }

    buildFeatures {
        compose = true
    }
}

dependencies {
    implementation(project(":core"))

    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    debugImplementation("androidx.compose.ui:ui-tooling")
}
