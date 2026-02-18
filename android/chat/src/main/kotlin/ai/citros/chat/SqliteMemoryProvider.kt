package ai.citros.chat

import ai.citros.core.MemoryFilter
import ai.citros.core.MemoryMetadata
import ai.citros.core.MemoryProvider
import ai.citros.core.MemoryResult
import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.UUID

/**
 * SQLite-backed [MemoryProvider] for on-device memory storage.
 *
 * Uses FTS5 for full-text search when available, with LIKE-based fallback.
 * All database operations run on [Dispatchers.IO].
 */
class SqliteMemoryProvider(
    private val database: SQLiteDatabase
) : MemoryProvider {

    private val hasFts: Boolean

    init {
        database.execSQL(
            """
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                tags TEXT,
                source TEXT,
                created_at INTEGER NOT NULL
            )
            """.trimIndent()
        )

        database.execSQL(
            "CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at)"
        )

        // Migrate existing tags to normalized format (leading/trailing commas, lowercase)
        // e.g. "Work,Urgent" → ",work,urgent,"
        database.execSQL(
            """
            UPDATE memories SET tags = ',' || LOWER(tags) || ','
            WHERE tags IS NOT NULL AND tags != '' AND tags NOT LIKE ',%'
            """.trimIndent()
        )

        hasFts = runCatching {
            database.execSQL(
                """
                CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts
                USING fts5(content, content='memories', content_rowid='rowid')
                """.trimIndent()
            )
            true
        }.getOrElse { e ->
            Log.w("SqliteMemoryProvider", "FTS5 unavailable, falling back to LIKE search", e)
            false
        }
    }

    override suspend fun store(content: String, metadata: MemoryMetadata): String =
        withContext(Dispatchers.IO) {
            val id = UUID.randomUUID().toString()
            val now = System.currentTimeMillis()
            val cleanTags = normalizeTags(metadata.tags)

            // Store tags with leading/trailing commas for exact-match filtering
            // e.g. ["work", "urgent"] → ",work,urgent,"
            val normalizedTags = if (cleanTags.isNotEmpty()) ",${cleanTags.joinToString(",")}," else ""

            val values = ContentValues().apply {
                put("id", id)
                put("content", content)
                put("tags", normalizedTags)
                put("source", metadata.source)
                put("created_at", now)
            }

            database.insertOrThrow("memories", null, values)
            if (hasFts) {
                database.execSQL(
                    "INSERT INTO memories_fts(rowid, content) SELECT rowid, content FROM memories WHERE id = ?",
                    arrayOf(id)
                )
            }

            id
        }

    override suspend fun search(query: String, limit: Int): List<MemoryResult> =
        withContext(Dispatchers.IO) {
            if (query.isBlank()) return@withContext emptyList()
            val safeLimit = limit.coerceAtLeast(1)

            val cursor = if (hasFts) {
                try {
                    database.rawQuery(
                        """
                        SELECT m.id, m.content, m.tags, m.source, m.created_at
                        FROM memories m
                        JOIN memories_fts fts ON m.rowid = fts.rowid
                        WHERE memories_fts MATCH ?
                        ORDER BY m.created_at DESC
                        LIMIT ?
                        """.trimIndent(),
                        arrayOf(query, safeLimit.toString())
                    )
                } catch (e: android.database.sqlite.SQLiteException) {
                    // FTS5 syntax error (unmatched quotes, bare operators, etc.)
                    // Fall back to LIKE search for this query
                    Log.w("SqliteMemoryProvider", "FTS5 query failed, falling back to LIKE", e)
                    database.rawQuery(
                        """
                        SELECT id, content, tags, source, created_at
                        FROM memories
                        WHERE content LIKE ?
                        ORDER BY created_at DESC
                        LIMIT ?
                        """.trimIndent(),
                        arrayOf("%$query%", safeLimit.toString())
                    )
                }
            } else {
                database.rawQuery(
                    """
                    SELECT id, content, tags, source, created_at
                    FROM memories
                    WHERE content LIKE ?
                    ORDER BY created_at DESC
                    LIMIT ?
                    """.trimIndent(),
                    arrayOf("%$query%", safeLimit.toString())
                )
            }

            cursor.use { toResults(it) }
        }

    override suspend fun delete(id: String): Unit = withContext(Dispatchers.IO) {
        if (hasFts) {
            database.execSQL(
                "DELETE FROM memories_fts WHERE rowid IN (SELECT rowid FROM memories WHERE id = ?)",
                arrayOf(id)
            )
        }
        database.delete("memories", "id = ?", arrayOf(id))
    }

    override suspend fun list(filter: MemoryFilter?): List<MemoryResult> =
        withContext(Dispatchers.IO) {
            val whereClauses = mutableListOf<String>()
            val args = mutableListOf<String>()

            filter?.since?.let {
                whereClauses += "created_at >= ?"
                args += it.toString()
            }

            // Move tag filtering into SQL when tags are specified
            val requiredTags = filter?.tags?.let { normalizeTags(it) }
            requiredTags?.forEach { tag ->
                // Exact match within normalized comma-delimited tags (e.g. ",work,urgent,")
                whereClauses += "tags LIKE ?"
                args += "%,$tag,%"
            }

            val where = if (whereClauses.isEmpty()) "" else "WHERE ${whereClauses.joinToString(" AND ")}"
            val limitClause = filter?.limit?.let { " LIMIT ?" }  ?: ""
            val sql = buildString {
                append("SELECT id, content, tags, source, created_at FROM memories ")
                append(where)
                append(" ORDER BY created_at DESC")
                append(limitClause)
            }

            if (filter?.limit != null) {
                args += filter.limit!!.coerceAtLeast(1).toString()
            }

            database.rawQuery(sql, args.toTypedArray()).use { toResults(it) }
        }

    /** Normalize tags: trim whitespace, lowercase, drop blanks. */
    private fun normalizeTags(tags: List<String>): List<String> =
        tags.map { it.trim().lowercase() }.filter { it.isNotEmpty() }

    private fun toResults(cursor: android.database.Cursor): List<MemoryResult> {
        val results = mutableListOf<MemoryResult>()
        while (cursor.moveToNext()) {
            val tagsRaw = cursor.getString(2).orEmpty()
            val tags = tagsRaw
                .split(',')
                .map { it.trim() }
                .filter { it.isNotEmpty() }

            results += MemoryResult(
                id = cursor.getString(0),
                content = cursor.getString(1),
                tags = tags,
                source = cursor.getString(3),
                createdAt = cursor.getLong(4)
            )
        }
        return results
    }
}
