package ai.citros.test

import org.junit.Test
import org.w3c.dom.Element
import java.io.ByteArrayInputStream
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class RegressionXmlReporterTest {

    @Test
    fun `toXml emits junit-style suite with pass and fail counts`() {
        val results = listOf(
            RegressionResult(
                taskId = "nav-001",
                taskName = "Open Settings",
                passed = true,
                criteriaResults = emptyList(),
                stepsUsed = 2,
                elapsedMs = 1_200,
                status = RegressionStatus.COMPLETED
            ),
            RegressionResult(
                taskId = "nav-002",
                taskName = "Navigate to Times Square",
                passed = false,
                criteriaResults = listOf(
                    CriterionEvaluation(
                        criterion = SuccessCriterion.AppInForeground("com.google.android.apps.maps"),
                        passed = false,
                        detail = "Expected com.google.android.apps.maps"
                    )
                ),
                stepsUsed = 6,
                elapsedMs = 3_400,
                status = RegressionStatus.FAILED
            )
        )

        val xml = RegressionXmlReporter().toXml(results)
        val doc = parseXml(xml)
        val suite = doc.getElementsByTagName("testsuite").item(0) as Element

        assertEquals("CitrosRegression", suite.getAttribute("name"))
        assertEquals("2", suite.getAttribute("tests"))
        assertEquals("1", suite.getAttribute("failures"))
        assertEquals("4.600", suite.getAttribute("time"))

        val cases = doc.getElementsByTagName("testcase")
        assertEquals(2, cases.length)

        val passingCase = cases.item(0) as Element
        assertEquals("1.200", passingCase.getAttribute("time"))

        val failedCase = cases.item(1) as Element
        assertEquals("3.400", failedCase.getAttribute("time"))
        val failureNodes = failedCase.getElementsByTagName("failure")
        assertEquals(1, failureNodes.length)
        val failure = failureNodes.item(0) as Element
        assertEquals("Expected com.google.android.apps.maps", failure.getAttribute("message"))
        assertEquals("Expected com.google.android.apps.maps", failure.textContent.trim())
    }

    @Test
    fun `toXml escapes xml-sensitive characters in task and failure text`() {
        val rawTaskName = "Name with <tag> & symbols \"double\" and 'single'"
        val rawFailureDetail = "Mismatch: expected <A&B> but got \"C\" and 'D'"
        val results = listOf(
            RegressionResult(
                taskId = "esc-001",
                taskName = rawTaskName,
                passed = false,
                criteriaResults = listOf(
                    CriterionEvaluation(
                        criterion = SuccessCriterion.ResponseContains("A&B"),
                        passed = false,
                        detail = rawFailureDetail
                    )
                ),
                stepsUsed = 1,
                elapsedMs = 250,
                status = RegressionStatus.FAILED
            )
        )

        val xml = RegressionXmlReporter().toXml(results)
        assertTrue(xml.contains("&lt;tag&gt;"))
        assertTrue(xml.contains("&amp;"))
        assertTrue(xml.contains("&quot;double&quot;"))
        assertTrue(xml.contains("&apos;single&apos;"))

        val doc = parseXml(xml)
        val testcase = doc.getElementsByTagName("testcase").item(0) as Element
        assertEquals(rawTaskName, testcase.getAttribute("name"))
        val failure = testcase.getElementsByTagName("failure").item(0) as Element
        assertEquals(rawFailureDetail, failure.getAttribute("message"))
        assertEquals(rawFailureDetail, failure.textContent.trim())
    }

    @Test
    fun `write creates file with junit xml content`() {
        val tempDir = createTempDir(prefix = "regression-xml-test-")
        try {
            val outputFile = File(tempDir, "nested/results.xml")
            val results = listOf(
                RegressionResult(
                    taskId = "nav-001",
                    taskName = "Open Settings",
                    passed = true,
                    criteriaResults = emptyList(),
                    stepsUsed = 2,
                    elapsedMs = 1_200,
                    status = RegressionStatus.COMPLETED
                )
            )

            val written = RegressionXmlReporter().write(results, outputFile)
            assertTrue(written.exists())
            assertEquals(outputFile.absolutePath, written.absolutePath)

            val xml = written.readText()
            val doc = parseXml(xml)
            val suite = doc.getElementsByTagName("testsuite").item(0) as Element
            assertEquals("1", suite.getAttribute("tests"))
            assertEquals("0", suite.getAttribute("failures"))
        } finally {
            tempDir.deleteRecursively()
        }
    }

    private fun parseXml(xml: String) = DocumentBuilderFactory.newInstance()
        .newDocumentBuilder()
        .parse(ByteArrayInputStream(xml.toByteArray()))
}
