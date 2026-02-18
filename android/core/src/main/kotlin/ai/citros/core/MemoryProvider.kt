package ai.citros.core

data class MemoryMetadata(
    val tags: List<String> = emptyList(),
    val source: String? = null
)

data class MemoryResult(
    val id: String,
    val content: String,
    val tags: List<String>,
    val source: String?,
    val createdAt: Long
)

data class MemoryFilter(
    val tags: List<String>? = null,
    val since: Long? = null,
    val limit: Int? = null
)

interface MemoryProvider {
    suspend fun store(content: String, metadata: MemoryMetadata = MemoryMetadata()): String
    suspend fun search(query: String, limit: Int = 10): List<MemoryResult>
    suspend fun delete(id: String)
    suspend fun list(filter: MemoryFilter? = null): List<MemoryResult>
}
