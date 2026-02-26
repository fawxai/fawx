package ai.citros.chat

import ai.citros.core.Message

/**
 * Process-level in-memory cache for chat UI state.
 *
 * Survives Activity/ViewModel recreation (e.g. appearance changes) within the same
 * app process, but is lost on process death (force-stop, low-memory kill, reboot).
 */
internal object ChatStateCache {
    data class Snapshot(
        val messages: List<Message>,
        val currentToolStatus: String?,
        val unreadCount: Int,
        val queuedMessage: String?,
        val lastActivityTimestamp: Long
    )

    @Volatile
    private var snapshot: Snapshot? = null

    fun read(): Snapshot? = snapshot

    fun write(snapshot: Snapshot) {
        this.snapshot = snapshot
    }

    fun clear() {
        snapshot = null
    }
}
