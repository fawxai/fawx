package ai.citros.test

import java.io.File
import java.util.Locale

class RegressionXmlReporter {
    fun toXml(results: List<RegressionResult>): String {
        val failures = results.count { !it.passed }
        val totalTimeSeconds = results.sumOf { it.elapsedMs }.toDouble() / 1000.0

        val body = results.joinToString(separator = "") { result ->
            val caseTimeSeconds = result.elapsedMs.toDouble() / 1000.0
            if (result.passed) {
                """
                <testcase name="${xmlEscape(result.taskName)}" classname="regression" time="${formatSeconds(caseTimeSeconds)}"/>
                """.trimIndent()
            } else {
                val failedCriteria = result.criteriaResults.filter { !it.passed }
                val failureMessage = failedCriteria.firstOrNull()?.detail ?: "Regression criteria failed"
                val failureText = failedCriteria.joinToString(separator = "\n") {
                    xmlEscape(it.detail)
                }.ifBlank { "Regression failed" }

                """
                <testcase name="${xmlEscape(result.taskName)}" classname="regression" time="${formatSeconds(caseTimeSeconds)}">
                  <failure message="${xmlEscape(failureMessage)}">$failureText</failure>
                </testcase>
                """.trimIndent()
            }
        }

        return """
            <?xml version="1.0" encoding="UTF-8"?>
            <testsuite name="CitrosRegression" tests="${results.size}" failures="$failures" time="${formatSeconds(totalTimeSeconds)}">
            $body
            </testsuite>
        """.trimIndent()
    }

    fun write(results: List<RegressionResult>, outputFile: File): File {
        outputFile.parentFile?.mkdirs()
        outputFile.writeText(toXml(results))
        return outputFile
    }

    private fun formatSeconds(seconds: Double): String = String.format(Locale.US, "%.3f", seconds)

    private fun xmlEscape(value: String): String = value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}
