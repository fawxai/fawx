package ai.citros.core

/**
 * Prompt assembly mode for the phone agent.
 */
enum class PromptMode {
    /** Full initial system prompt with comprehensive policy and workflow guidance. */
    FULL,

    /** Compact action-loop prompt with focused reminders for in-progress execution. */
    MINIMAL,

    /**
     * Ultra-minimal prompt: identity-only baseline with no operational sections.
     *
     * NONE excludes all security guidance and operational rules. Only use for
     * non-agentic contexts (e.g. pure Q&A, identity probing) where the model
     * does NOT take tool actions on the user's behalf.
     */
    NONE
}
