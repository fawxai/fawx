package ai.citros.core

/**
 * Persisted user preferences for non-core tool categories.
 * Core category cannot be disabled.
 * See docs/specs/h2-3-tool-grouping-spec.md Section 5.1.
 */
data class UserToolCategorySettings private constructor(
    private val settings: Map<ToolCategory, Boolean>
) {
    constructor() : this(emptyMap())
    /**
     * Returns true if the category is enabled (default: true for all).
     * CORE always returns true regardless of stored value.
     */
    fun isEnabled(category: ToolCategory): Boolean {
        if (category == ToolCategory.CORE) return true
        return settings[category] ?: true
    }

    /**
     * Returns a new immutable settings object with the category updated.
     * Setting CORE to disabled is ignored.
     */
    fun withEnabled(category: ToolCategory, enabled: Boolean): UserToolCategorySettings {
        if (category == ToolCategory.CORE) return this
        val updated = settings.toMutableMap()
        updated[category] = enabled
        return UserToolCategorySettings(updated.toMap())
    }

    /** Backward-compatible mutator-style alias for existing call sites/tests. */
    fun setEnabled(category: ToolCategory, enabled: Boolean): UserToolCategorySettings =
        withEnabled(category, enabled)

    /** Returns the set of user-disabled non-core categories. */
    fun disabledCategories(): Set<ToolCategory> =
        settings.filter { !it.value && it.key != ToolCategory.CORE }
            .keys
            .toSet()

    /** Create a snapshot copy for atomic reads during resolution. */
    fun snapshot(): UserToolCategorySettings =
        UserToolCategorySettings(settings.toMap())

    companion object {
        /** All categories enabled (default). */
        fun allEnabled(): UserToolCategorySettings = UserToolCategorySettings(emptyMap())

        /** Builder for explicit immutable construction. */
        fun builder(): Builder = Builder()
    }

    class Builder internal constructor() {
        private val mutableSettings = mutableMapOf<ToolCategory, Boolean>()

        fun setEnabled(category: ToolCategory, enabled: Boolean): Builder {
            if (category != ToolCategory.CORE) {
                mutableSettings[category] = enabled
            }
            return this
        }

        fun build(): UserToolCategorySettings = UserToolCategorySettings(mutableSettings.toMap())
    }
}
