package ai.citros.core

import java.text.Normalizer

/**
 * Pure policy resolver for tool grouping.
 * Follows the normative pseudocode in Section 5.2.1 exactly.
 * See docs/specs/h2-3-tool-grouping-spec.md.
 *
 * Thread-safety: this object is stateless and thread-safe. Callers should pass an immutable
 * [UserToolCategorySettings] snapshot for per-call consistency under concurrent updates.
 */
object ToolGroupingPolicy {

    /**
     * Capability flags for the current turn.
     */
    data class Capabilities(
        val accessibilityAttached: Boolean = true,
        val hasTinyFishKey: Boolean = true
    )

    /** Categories blocked by accessibility-detached state (phone-control categories). */
    private val ACCESSIBILITY_BLOCKED_CATEGORIES: Set<ToolCategory> = setOf(
        ToolCategory.NAVIGATION,
        ToolCategory.INTERACTION,
        ToolCategory.OBSERVATION
    )

    /** Imperative verbs for action-oriented fallback trigger (Section 5.2.1). */
    private val SINGLE_WORD_ACTION_VERBS = setOf(
        "open", "tap", "type", "send", "enable", "disable", "launch"
    )
    private val MULTI_WORD_ACTION_VERBS = setOf(
        "turn on", "turn off"
    )

    /** Map from ToolCategory to user_disabled reason code. */
    private val USER_DISABLED_REASON: Map<ToolCategory, ReasonCode> = mapOf(
        ToolCategory.NAVIGATION to ReasonCode.user_disabled_navigation,
        ToolCategory.INTERACTION to ReasonCode.user_disabled_interaction,
        ToolCategory.OBSERVATION to ReasonCode.user_disabled_observation,
        ToolCategory.NOTIFICATION to ReasonCode.user_disabled_notification,
        ToolCategory.CLIPBOARD to ReasonCode.user_disabled_clipboard,
        ToolCategory.MEMORY to ReasonCode.user_disabled_memory,
        ToolCategory.RESEARCH to ReasonCode.user_disabled_research,
        ToolCategory.PLANNING to ReasonCode.user_disabled_planning
    )

    /**
     * Resolve the tool plan for a single turn.
     *
     * @param messageText Current user message text
     * @param modelTier Model tier for this turn
     * @param capabilities Runtime capability flags
     * @param userSettings User category enable/disable preferences
     * @return Resolved tool plan with active categories, tool names, and reason codes
     */
    fun resolve(
        messageText: String,
        modelTier: ModelTier,
        capabilities: Capabilities,
        userSettings: UserToolCategorySettings
    ): ResolvedToolPlan {
        val reasons = mutableListOf<ReasonCode>()

        // Step 1: Build allow_set (Section 5.2.1)
        val allowSet = ToolCategory.entries.toMutableSet()

        if (modelTier == ModelTier.SMALL) {
            allowSet -= ToolCategory.RESEARCH
        }

        if (!capabilities.accessibilityAttached) {
            allowSet -= ACCESSIBILITY_BLOCKED_CATEGORIES
        }

        val userDisabled = userSettings.disabledCategories()
        allowSet -= userDisabled
        allowSet += ToolCategory.CORE

        // Step 2: Context selection
        val resolverResult = ContextCategoryResolver.resolve(messageText)
        val resolverCandidates = resolverResult.candidates

        val activeSet = (resolverCandidates intersect allowSet).toMutableSet()

        if (ToolCategory.CORE !in resolverCandidates) {
            reasons += ReasonCode.core_forced_required
        }
        activeSet += ToolCategory.CORE

        // Step 3: Fallback (Section 5.2.1)
        val actionTrigger = isActionOrientedTrigger(messageText, resolverResult.actionIntent)

        if (activeSet == setOf(ToolCategory.CORE) && actionTrigger) {
            for (c in listOf(ToolCategory.NAVIGATION, ToolCategory.INTERACTION, ToolCategory.OBSERVATION)) {
                if (c in allowSet) {
                    activeSet += c
                } else if (c in userDisabled) {
                    // Fallback wanted to add this category but user disabled it
                    USER_DISABLED_REASON[c]?.let { reasons += it }
                }
            }
            reasons += ReasonCode.fallback_action_intent
        }

        // Intentional ordering per spec: action-intent fallback is evaluated first,
        // then empty-candidate fallback reason is emitted when resolver had no candidates.
        if (activeSet == setOf(ToolCategory.CORE) && resolverCandidates.isEmpty()) {
            reasons += ReasonCode.fallback_empty_candidate_set
        }

        val activeOrdered = ResolvedToolPlan.canonicalOrder(activeSet)

        val allToolsForCategories = PhoneTools.getToolsForCategories(activeSet, modelTier)
        val toolNames = allToolsForCategories.map { it.name }.toMutableSet()

        if (!capabilities.hasTinyFishKey && "web_browse" in toolNames) {
            toolNames -= "web_browse"
            reasons += ReasonCode.capability_missing_tinyfish_blocks_web_browse
        }

        if (!capabilities.accessibilityAttached) {
            reasons += ReasonCode.capability_missing_accessibility_blocks_phone_control
        }

        for (candidate in resolverCandidates) {
            if (candidate in activeSet) continue
            if (candidate == ToolCategory.RESEARCH && modelTier == ModelTier.SMALL) {
                reasons += ReasonCode.tier_small_blocks_research
            }
            if (candidate in userDisabled) {
                USER_DISABLED_REASON[candidate]?.let { reasons += it }
            }
        }

        val dedupedReasons = reasons.distinct()

        return ResolvedToolPlan(
            activeCategories = activeOrdered,
            toolNames = toolNames,
            reasonCodes = dedupedReasons,
            estimatedToolCount = toolNames.size
        )
    }

    /**
     * Action-oriented fallback trigger per Section 5.2.1 item 5.
     * True when resolver emits action_intent=true OR message contains
     * imperative verb from the normative list.
     */
    internal fun isActionOrientedTrigger(messageText: String, resolverActionIntent: Boolean): Boolean {
        if (resolverActionIntent) return true

        val normalized = Normalizer.normalize(messageText, Normalizer.Form.NFKC)
            .lowercase(java.util.Locale.ROOT)
            .replace(Regex("\\s+"), " ")
            .trim()

        for (verb in MULTI_WORD_ACTION_VERBS) {
            if (normalized.contains(verb)) return true
        }

        for (verb in SINGLE_WORD_ACTION_VERBS) {
            if (Regex("\\b${Regex.escape(verb)}\\b").containsMatchIn(normalized)) return true
        }

        return false
    }
}
