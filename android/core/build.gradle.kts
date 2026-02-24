import org.gradle.api.tasks.testing.Test

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "ai.citros.core"
    compileSdk = 35

    defaultConfig {
        minSdk = 28
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    testOptions {
        unitTests {
            isReturnDefaultValues = true
            isIncludeAndroidResources = true
        }
    }
}

tasks.register<Test>("phoneAgentApiSensorCiTest") {
    group = "verification"
    description = "Runs PhoneAgentApiTest for CI sensor timeout/concurrency coverage"

    val debugUnitTest = tasks.named<Test>("testDebugUnitTest").get()
    testClassesDirs = debugUnitTest.testClassesDirs
    classpath = debugUnitTest.classpath
    shouldRunAfter(debugUnitTest)

    filter {
        includeTestsMatching("ai.citros.core.PhoneAgentApiTest.*sensor*")
        includeTestsMatching("ai.citros.core.PhoneAgentApiTest.*Sensor*")
        includeTestsMatching("ai.citros.core.PhoneAgentApiTest.*timeout*")
        includeTestsMatching("ai.citros.core.PhoneAgentApiTest.*concurrent*")
    }
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    api("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jsoup:jsoup:1.18.3")
    // NanoHTTPD 2.3.1 is the final release (project archived). No security advisories.
    implementation("org.nanohttpd:nanohttpd:2.3.1")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("com.k2fsa:sherpa-onnx:1.12.25")

    // Testing
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation("org.jetbrains.kotlin:kotlin-test:2.1.0")
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("androidx.test:core:1.6.1")
    testImplementation("androidx.test.ext:junit:1.2.1")
    testImplementation("org.mockito.kotlin:mockito-kotlin:5.4.0")
}
