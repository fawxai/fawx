package ai.citros.test

import org.junit.Assume.assumeTrue
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runners.model.Statement

/**
 * Enables runtime handling for [Flaky]-annotated tests.
 *
 * Set `-Dcitros.runFlakyTests=false` to skip flaky tests during a run.
 */
class FlakyTestRule(
    private val runFlakyTests: Boolean = System.getProperty(PROP_RUN_FLAKY_TESTS, "true").toBoolean()
) : TestRule {

    override fun apply(base: Statement, description: Description): Statement {
        return object : Statement() {
            override fun evaluate() {
                val flaky = description.getAnnotation(Flaky::class.java)
                    ?: description.testClass?.getAnnotation(Flaky::class.java)

                if (flaky != null && !runFlakyTests) {
                    assumeTrue(
                        "Skipping @Flaky test ${description.className}.${description.methodName} (issue=${flaky.issue})",
                        false
                    )
                }
                base.evaluate()
            }
        }
    }

    companion object {
        const val PROP_RUN_FLAKY_TESTS = "citros.runFlakyTests"
    }
}
