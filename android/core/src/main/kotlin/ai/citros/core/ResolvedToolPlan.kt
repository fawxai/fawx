package ai.citros.core

/**
 * Result of tool grouping policy resolution for a single turn.
 * See docs/specs/h2-3-tool-grouping-spec.md Section 7.2.
 *
 * [activeCategories] is ordered in canonical category order (Section 5.2.2)
 * and contains unique entries only.
 */
data class ResolvedToolPlan(
    val activeCategories: List<ToolCategory>,
    val toolNames: Set<String>,
    val reasonCodes: List<ReasonCode>,
    val estimatedToolCount: Int
) {
    init {
        // Invariant: no duplicate categories
        require(activeCategories.size == activeCategories.toSet().size) {
            "activeCategories must contain unique entries"
        }
        // Invariant: canonical ordering
        val indices = activeCategories.map { CANONICAL_ORDER.indexOf(it) }
        require(indices == indices.sorted()) {
            "activeCategories must be in canonical order"
        }
        // Invariant: CORE always present
        require(ToolCategory.CORE in activeCategories) {
            "activeCategories must always include CORE"
        }
    }

    companion object {
        /** Canonical category ordering per Section 5.2.2. */
        val CANONICAL_ORDER: List<ToolCategory> = listOf(
            ToolCategory.CORE,
            ToolCategory.NAVIGATION,
            ToolCategory.INTERACTION,
            ToolCategory.OBSERVATION,
            ToolCategory.NOTIFICATION,
            ToolCategory.CLIPBOARD,
            ToolCategory.MEMORY,
            ToolCategory.RESEARCH,
            ToolCategory.PLANNING
        )

        /** Sort a set of categories into canonical order. */
        fun canonicalOrder(categories: Set<ToolCategory>): List<ToolCategory> =
            CANONICAL_ORDER.filter { it in categories }
    }
}
