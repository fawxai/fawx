package ai.citros.core

/**
 * Capability tier for LLM models.
 *
 * Used for model floor enforcement in the action loop:
 * - FLAGSHIP and STANDARD are permitted
 * - SMALL is prohibited (security risk with untrusted screen content)
 */
enum class ModelTier {
    /** Highest capability: Opus, o1, o3, GPT-5.x */
    FLAGSHIP,
    /** Standard capability: Sonnet, GPT-4o — minimum for action loop */
    STANDARD,
    /** Small/fast models: Haiku, GPT-4o-mini, o1-mini — prohibited in action loop */
    SMALL
}

/**
 * Classifies model IDs into capability tiers using pattern matching.
 *
 * This is the security-critical classification used by the model floor policy.
 * The action loop processes untrusted screen content from arbitrary apps,
 * so only STANDARD tier and above are permitted.
 *
 * Classification strategy:
 * 1. Check SMALL patterns first (most restrictive — "-mini" and "haiku" override everything)
 * 2. Check FLAGSHIP patterns (opus, o1, o3, gpt-5)
 * 3. Check STANDARD patterns (sonnet, gpt-4o)
 * 4. Default to STANDARD for unknown models (permissive — new models from major providers
 *    are typically mid-tier or above; defaulting to SMALL would block legitimate new models
 *    until the classifier is updated)
 *
 * Provider-agnostic: works on both direct API IDs ("claude-sonnet-4-5-20250929")
 * and OpenRouter-prefixed IDs ("anthropic/claude-sonnet-4.5").
 */
object ModelClassifier {

    /**
     * Classify a model ID into a capability tier.
     *
     * @param modelId The model ID string (any provider format)
     * @return The classified [ModelTier]
     */
    fun classify(modelId: String): ModelTier {
        val id = modelId.lowercase()

        // 1. SMALL — check first because "-mini" overrides base model name
        //    e.g. "o1-mini" contains "o1" but is still small
        if (isSmallModel(id)) return ModelTier.SMALL

        // 2. FLAGSHIP
        if (isFlagshipModel(id)) return ModelTier.FLAGSHIP

        // 3. STANDARD
        if (isStandardModel(id)) return ModelTier.STANDARD

        // 4. Unknown models default to STANDARD (permissive)
        return ModelTier.STANDARD
    }

    /**
     * Check if a model meets the minimum capability floor for the action loop.
     *
     * @param modelId The model ID to check
     * @return true if the model is STANDARD tier or above
     */
    fun isAboveFloor(modelId: String): Boolean {
        return classify(modelId) != ModelTier.SMALL
    }

    /**
     * Small/fast models: Haiku variants, -mini variants, lightweight models.
     * These are explicitly prohibited in the action loop.
     */
    private fun isSmallModel(id: String): Boolean {
        return id.contains("haiku") ||     // claude-haiku-*, claude-3-5-haiku-*
               id.contains("-mini") ||     // gpt-4o-mini, o1-mini, o3-mini
               id.contains("_mini")        // future naming variants
    }

    /**
     * Flagship/highest-capability models: Opus, o1/o3 (non-mini), GPT-5.x.
     */
    private fun isFlagshipModel(id: String): Boolean {
        if (id.contains("opus")) return true    // claude-opus-*, anthropic/claude-opus-*
        if (id.contains("gpt-5")) return true   // gpt-5, gpt-5-turbo, etc.

        // o1/o3 reasoning models (without -mini, already filtered)
        // Match both direct IDs ("o1", "o1-preview") and OpenRouter ("openai/o1")
        val baseId = id.substringAfterLast("/")  // strip provider prefix
        if (baseId.startsWith("o1") || baseId.startsWith("o3")) return true

        return false
    }

    /**
     * Standard capability models: Sonnet, GPT-4o (without -mini), GPT-4-turbo.
     */
    private fun isStandardModel(id: String): Boolean {
        return id.contains("sonnet") ||        // claude-sonnet-*, anthropic/claude-sonnet-*
               id.contains("gpt-4o") ||        // gpt-4o (mini already filtered above)
               id.contains("gpt-4-turbo")      // gpt-4-turbo
    }
}
