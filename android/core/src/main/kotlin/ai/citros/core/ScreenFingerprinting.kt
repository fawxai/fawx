package ai.citros.core

import java.security.MessageDigest
import kotlin.math.abs
import kotlin.math.max

/** Structural fingerprinting utilities for stable/replayable UI matching. */
object ScreenFingerprinting {
    fun compute(screen: ScreenContent): ScreenFingerprint {
        val structural = screen.elements
            .map { element ->
                StructuralNode(
                    className = element.className ?: "Unknown",
                    isClickable = element.isClickable,
                    isEditable = element.isEditable,
                    depth = element.depth
                )
            }
            .sortedWith(compareBy({ it.depth }, { it.className }, { it.isClickable }, { it.isEditable }))

        val hashInput = if (structural.isEmpty()) {
            // Empty screen trees are common while UI is loading. Include package to avoid
            // collapsing all-empty screens across apps into a single global fingerprint.
            "EMPTY:${screen.packageName.orEmpty()}"
        } else {
            structural.joinToString("|") {
                "${it.depth}:${it.className}:${it.isClickable}:${it.isEditable}"
            }
        }

        return ScreenFingerprint(
            structuralHash = sha256(hashInput),
            packageName = screen.packageName,
            // TODO(playbooks): plumb activity name from accessibility events into ScreenContent.
            activityName = null,
            interactiveCount = screen.elements.count { it.isClickable || it.isEditable },
            maxDepth = screen.elements.maxOfOrNull { it.depth } ?: 0,
            classSignature = structural.map { it.className }.distinct()
        )
    }

    fun similarity(a: ScreenFingerprint, b: ScreenFingerprint): Float {
        if (a.packageName != b.packageName) return 0f
        if (a.structuralHash == b.structuralHash) return 1f

        val classOverlap = jaccard(a.classSignature.toSet(), b.classSignature.toSet())
        val countSimilarity = 1f - (
            abs(a.interactiveCount - b.interactiveCount).toFloat() /
                max(a.interactiveCount, b.interactiveCount).coerceAtLeast(1)
            )
        // Base structural similarity intentionally tops out at 0.8. The final 0.2 is reserved for
        // activity-name agreement once activity capture is wired through ScreenContent.
        val activityBonus = if (a.activityName != null && a.activityName == b.activityName) 0.2f else 0f

        return (classOverlap * 0.5f + countSimilarity * 0.3f + activityBonus).coerceIn(0f, 1f)
    }

    private fun sha256(value: String): String {
        val bytes = MessageDigest.getInstance("SHA-256").digest(value.toByteArray())
        return bytes.joinToString("") { "%02x".format(it) }
    }

    private fun jaccard(a: Set<String>, b: Set<String>): Float {
        val union = a union b
        if (union.isEmpty()) return 0f
        val intersection = a intersect b
        return intersection.size.toFloat() / union.size
    }

    private data class StructuralNode(
        val className: String,
        val isClickable: Boolean,
        val isEditable: Boolean,
        val depth: Int
    )
}
