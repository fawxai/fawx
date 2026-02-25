package ai.citros.test

/**
 * Marks tests that are currently timing- or infra-sensitive.
 *
 * Use `issue` as either a GitHub issue reference (e.g. `#751`) or a full issue URL.
 *
 * CI behavior is controlled by [FlakyTestRule]:
 * - default: flaky tests run
 * - set JVM system property `citros.runFlakyTests=false` to skip flaky tests
 */
@Target(AnnotationTarget.FUNCTION, AnnotationTarget.CLASS)
@Retention(AnnotationRetention.RUNTIME)
annotation class Flaky(val issue: String)
