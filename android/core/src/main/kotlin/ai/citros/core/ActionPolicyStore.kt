package ai.citros.core

enum class PolicyOverrideLevel {
    CONFIRM,
    DENY
}

interface ActionPolicyStore {
    fun getOverride(toolName: String): PolicyOverrideLevel?
    fun setOverride(toolName: String, decision: PolicyOverrideLevel?)
    fun getAllOverrides(): Map<String, PolicyOverrideLevel>
}
