package ai.citros.core

/**
 * Typed reason codes for tool grouping policy decisions.
 * See docs/specs/h2-3-tool-grouping-spec.md Section 7.2.
 *
 * Compatibility policy: new values are additive only. Existing values
 * must not be renamed or repurposed. Clients must ignore unknown values.
 */
enum class ReasonCode {
    tier_small_blocks_research,
    user_disabled_navigation,
    user_disabled_interaction,
    user_disabled_observation,
    user_disabled_notification,
    user_disabled_clipboard,
    user_disabled_memory,
    user_disabled_research,
    user_disabled_planning,
    capability_missing_tinyfish_blocks_web_browse,
    capability_missing_accessibility_blocks_phone_control,
    fallback_action_intent,
    fallback_empty_candidate_set,
    core_forced_required
}
