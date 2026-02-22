package ai.citros.core

/**
 * Manages packages whose screen content must be hidden from the agent.
 */
interface PrivacyList {
    fun isPrivate(packageName: String): Boolean
    fun getAll(): Set<String>
    fun add(packageName: String)
    fun remove(packageName: String)
}
