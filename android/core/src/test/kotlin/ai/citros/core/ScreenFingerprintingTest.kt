package ai.citros.core

import android.graphics.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ScreenFingerprintingTest {
    @Test
    fun sameStructureDifferentText_producesSameHash() {
        val a = screen(
            pkg = "com.messages",
            elements = listOf(
                element(1, "Mom", "android.widget.TextView", clickable = true, depth = 1),
                element(2, "hello", "android.widget.EditText", editable = true, depth = 2)
            )
        )
        val b = screen(
            pkg = "com.messages",
            elements = listOf(
                element(1, "Dad", "android.widget.TextView", clickable = true, depth = 1),
                element(2, "bye", "android.widget.EditText", editable = true, depth = 2)
            )
        )

        val fpA = ScreenFingerprinting.compute(a)
        val fpB = ScreenFingerprinting.compute(b)

        assertEquals(fpA.structuralHash, fpB.structuralHash)
    }

    @Test
    fun differentStructure_producesDifferentHash() {
        val a = screen("com.messages", listOf(element(1, "Send", "android.widget.Button", clickable = true, depth = 1)))
        val b = screen("com.messages", listOf(element(1, "Search", "android.widget.EditText", editable = true, depth = 1)))

        val fpA = ScreenFingerprinting.compute(a)
        val fpB = ScreenFingerprinting.compute(b)

        assertNotEquals(fpA.structuralHash, fpB.structuralHash)
    }

    @Test
    fun similarity_highForNearIdentical_zeroForDifferentApps() {
        val base = ScreenFingerprint(
            structuralHash = "h1",
            packageName = "com.messages",
            activityName = "Main",
            interactiveCount = 10,
            maxDepth = 4,
            classSignature = listOf("A", "B", "C")
        )
        val near = base.copy(structuralHash = "h2", interactiveCount = 9, classSignature = listOf("A", "B", "C", "D"))
        val otherApp = base.copy(packageName = "com.maps", structuralHash = "h3")

        assertTrue(ScreenFingerprinting.similarity(base, near) > 0.7f)
        assertEquals(0f, ScreenFingerprinting.similarity(base, otherApp))
    }

    @Test
    fun similarity_withoutActivityName_bonusCeilingIsPointEight() {
        val a = ScreenFingerprint(
            structuralHash = "h1",
            packageName = "com.messages",
            activityName = null,
            interactiveCount = 10,
            maxDepth = 4,
            classSignature = listOf("A", "B", "C")
        )
        val b = a.copy(structuralHash = "h2")

        assertEquals(0.8f, ScreenFingerprinting.similarity(a, b), 0.0001f)
    }

    @Test
    fun screenFingerprint_intConstructor_preservesLegacySourceCompatibility() {
        val legacy = ScreenFingerprint(12345, "com.messages")

        assertEquals("12345", legacy.structuralHash)
        assertEquals("com.messages", legacy.packageName)
    }

    @Test
    fun compute_emptyElements_includesPackageInHashInput() {
        val a = screen(pkg = "com.messages", elements = emptyList())
        val b = screen(pkg = "com.maps", elements = emptyList())

        val fpA = ScreenFingerprinting.compute(a)
        val fpB = ScreenFingerprinting.compute(b)

        assertNotEquals(fpA.structuralHash, fpB.structuralHash)
        assertEquals(0, fpA.interactiveCount)
        assertEquals(0, fpA.maxDepth)
        assertTrue(fpA.classSignature.isEmpty())
    }

    private fun screen(pkg: String, elements: List<ScreenElement>) =
        ScreenContent(elements = elements, packageName = pkg)

    private fun element(
        id: Int,
        text: String,
        cls: String,
        clickable: Boolean = false,
        editable: Boolean = false,
        depth: Int = 0
    ) = ScreenElement(
        id = id,
        text = text,
        contentDescription = null,
        className = cls,
        isClickable = clickable,
        isEditable = editable,
        bounds = Rect(0, 0, 10, 10),
        depth = depth
    )
}
