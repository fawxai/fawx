package ai.citros.core

import android.util.Log
import kotlin.math.ceil

/**
 * Prompt budget enforcement for model-aware prompt tuning (H2.4 spec Section 4.3).
 *
 * Token estimate: ceil(utf8_char_count / 4).
 * Deterministic trimming order (lowest priority first):
 * 1. verbose_examples
 * 2. communication_style
 * 3. recovery_elaboration
 * 4. tool_parameter_detail
 * 5. strategy_detail
 * 6. disambiguation
 * 7. agent_directives
 * 8. user_context
 * 9. memory_context
 * 10. device_awareness
 * 11. tools
 *
 * Never-trim sections: identity_baseline, security_block, critical_execution_rules,
 * capability_warning, runtime_line.
 */
object PromptBudget {

    /** Canonical section IDs used for trimming and telemetry. */
    object SectionId {
        const val IDENTITY_BASELINE = "identity_baseline"
        const val TOOLS = "tools"
        const val STRATEGY_DETAIL = "strategy_detail"
        const val DEVICE_AWARENESS = "device_awareness"
        const val RECOVERY_ELABORATION = "recovery_elaboration"
        const val COMMUNICATION_STYLE = "communication_style"
        const val DISAMBIGUATION = "disambiguation"
        const val AGENT_DIRECTIVES = "agent_directives"
        const val SECURITY_BLOCK = "security_block"
        const val CRITICAL_EXECUTION_RULES = "critical_execution_rules"
        const val USER_CONTEXT = "user_context"
        const val MEMORY_CONTEXT = "memory_context"
        const val CAPABILITY_WARNING = "capability_warning"
        const val RUNTIME_LINE = "runtime_line"
        const val VERBOSE_EXAMPLES = "verbose_examples"
        const val TOOL_PARAMETER_DETAIL = "tool_parameter_detail"
    }

    /** Budget limits per mode (estimated tokens). */
    data class BudgetLimits(val softBudget: Int, val hardBudget: Int)

    val BUDGETS: Map<PromptMode, BudgetLimits> = mapOf(
        PromptMode.FULL to BudgetLimits(2200, 2600),
        PromptMode.MINIMAL to BudgetLimits(900, 1100),
        PromptMode.NONE to BudgetLimits(40, 60)
    )

    /** Estimate tokens from character count: ceil(chars / 4). */
    fun estimateTokens(charCount: Int): Int = ceil(charCount.toDouble() / 4.0).toInt()

    /**
     * Trimmable section IDs in priority order (lowest priority = trimmed first).
     * These are the only sections eligible for trimming.
     */
    val TRIM_ORDER: List<String> = listOf(
        SectionId.VERBOSE_EXAMPLES,
        SectionId.COMMUNICATION_STYLE,
        SectionId.RECOVERY_ELABORATION,
        SectionId.TOOL_PARAMETER_DETAIL,
        SectionId.STRATEGY_DETAIL,
        SectionId.DISAMBIGUATION,
        SectionId.AGENT_DIRECTIVES,
        SectionId.USER_CONTEXT,
        SectionId.MEMORY_CONTEXT,
        SectionId.DEVICE_AWARENESS,
        SectionId.TOOLS
    )

    /** Sections that must never be trimmed. */
    val NEVER_TRIM: Set<String> = setOf(
        SectionId.IDENTITY_BASELINE,
        SectionId.SECURITY_BLOCK,
        SectionId.CRITICAL_EXECUTION_RULES,
        SectionId.CAPABILITY_WARNING,
        SectionId.RUNTIME_LINE
    )

    /**
     * A labeled prompt section for budget-aware assembly.
     */
    data class LabeledSection(
        val id: String,
        val content: String
    )

    /**
     * Result of budget enforcement.
     */
    data class BudgetResult(
        val finalPrompt: String,
        val charCount: Int,
        val tokenEstimate: Int,
        val trimmed: Boolean,
        val trimmedSections: List<String>,
        val softBudgetExceeded: Boolean
    ) {
        fun withAppendedContent(appendedContent: String, separator: String = "\n\n"): BudgetResult {
            val combined = if (finalPrompt.isBlank()) appendedContent else "$finalPrompt$separator$appendedContent"
            return copy(
                finalPrompt = combined,
                charCount = combined.length,
                tokenEstimate = estimateTokens(combined.length)
            )
        }
    }

    /**
     * Enforce budget on labeled sections.
     *
     * @param sections ordered list of labeled sections
     * @param mode prompt mode for budget lookup
     * @return budget result with trimming applied if needed
     */
    fun enforce(sections: List<LabeledSection>, mode: PromptMode): BudgetResult {
        val limits = BUDGETS[mode] ?: BudgetLimits(Int.MAX_VALUE, Int.MAX_VALUE)

        // Build initial prompt
        var currentSections = sections.toMutableList()
        var prompt = assemblePrompt(currentSections)
        var tokens = estimateTokens(prompt.length)
        val softExceeded = tokens > limits.softBudget
        val trimmedIds = mutableListOf<String>()

        // If over hard budget, trim in order
        if (tokens > limits.hardBudget) {
            for (sectionId in TRIM_ORDER) {
                if (tokens <= limits.hardBudget) break
                val idx = currentSections.indexOfFirst { it.id == sectionId }
                if (idx >= 0) {
                    trimmedIds.add(sectionId)
                    currentSections.removeAt(idx)
                    prompt = assemblePrompt(currentSections)
                    tokens = estimateTokens(prompt.length)
                }
            }
        }

        if (tokens > limits.hardBudget) {
            Log.w(
                "PromptBudget",
                "Prompt remains over hard budget after exhausting trim order: mode=$mode, tokens=$tokens, hard=${limits.hardBudget}, chars=${prompt.length}"
            )
        }

        return BudgetResult(
            finalPrompt = prompt,
            charCount = prompt.length,
            tokenEstimate = tokens,
            trimmed = trimmedIds.isNotEmpty(),
            trimmedSections = trimmedIds,
            softBudgetExceeded = softExceeded
        )
    }

    private fun assemblePrompt(sections: List<LabeledSection>): String =
        sections.filter { it.content.isNotBlank() }
            .joinToString("\n\n") { it.content }
}
