package ai.citros.chat

import ai.citros.core.MemoryFilter
import ai.citros.core.MemoryMetadata
import android.database.sqlite.SQLiteDatabase
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class SqliteMemoryProviderTest {

    private lateinit var db: SQLiteDatabase
    private lateinit var provider: SqliteMemoryProvider

    @Before
    fun setUp() {
        db = SQLiteDatabase.create(null)
        provider = SqliteMemoryProvider(db)
    }

    @After
    fun tearDown() {
        db.close()
    }

    @Test
    fun `store and list returns memory`() = runTest {
        val id = provider.store(
            content = "buy oat milk",
            metadata = MemoryMetadata(tags = listOf("shopping", "home"), source = "user")
        )

        val all = provider.list()

        assertEquals(1, all.size)
        assertEquals(id, all[0].id)
        assertEquals("buy oat milk", all[0].content)
        assertEquals(listOf("shopping", "home"), all[0].tags)
        assertEquals("user", all[0].source)
    }

    @Test
    fun `search matches keyword`() = runTest {
        provider.store("project alpha deadline friday", MemoryMetadata(tags = listOf("work")))
        provider.store("call mom on saturday", MemoryMetadata(tags = listOf("personal")))

        val results = provider.search("alpha", limit = 10)

        assertEquals(1, results.size)
        assertTrue(results[0].content.contains("alpha"))
    }

    @Test
    fun `delete removes memory`() = runTest {
        val id = provider.store("temporary note", MemoryMetadata())
        provider.delete(id)

        val all = provider.list()
        assertTrue(all.none { it.id == id })
    }

    @Test
    fun `list supports filter and ordering`() = runTest {
        provider.store("first", MemoryMetadata(tags = listOf("work")))
        // Small delay to guarantee distinct created_at timestamps
        kotlinx.coroutines.delay(5)
        val secondId = provider.store("second", MemoryMetadata(tags = listOf("work", "urgent")))
        kotlinx.coroutines.delay(5)
        provider.store("third", MemoryMetadata(tags = listOf("personal")))

        val all = provider.list()
        assertEquals(listOf("third", "second", "first"), all.map { it.content })

        val workOnly = provider.list(MemoryFilter(tags = listOf("work")))
        assertEquals(2, workOnly.size)
        assertTrue(workOnly.all { "work" in it.tags })

        val urgentOnly = provider.list(MemoryFilter(tags = listOf("work", "urgent")))
        assertEquals(1, urgentOnly.size)
        assertEquals(secondId, urgentOnly[0].id)

        val since = provider.list(MemoryFilter(since = all[1].createdAt, limit = 1))
        assertEquals(1, since.size)
        assertEquals("third", since[0].content)
    }

    // ========== Tag Filtering False Positive Tests (#328) ==========

    @Test
    fun `tag filter does not match substring tags`() = runTest {
        // #328: filtering by "work" should NOT match "working" or "homework"
        provider.store("meeting notes", MemoryMetadata(tags = listOf("work")))
        provider.store("gym routine", MemoryMetadata(tags = listOf("working-out")))
        provider.store("school assignment", MemoryMetadata(tags = listOf("homework")))
        provider.store("career stuff", MemoryMetadata(tags = listOf("work-related")))

        val results = provider.list(MemoryFilter(tags = listOf("work")))

        assertEquals(1, results.size)
        assertEquals("meeting notes", results[0].content)
    }

    @Test
    fun `tag filter exact match with multiple tags`() = runTest {
        provider.store("urgent task", MemoryMetadata(tags = listOf("work", "urgent")))
        provider.store("workout plan", MemoryMetadata(tags = listOf("working", "urgent")))

        val results = provider.list(MemoryFilter(tags = listOf("work")))

        assertEquals(1, results.size)
        assertEquals("urgent task", results[0].content)
    }

    @Test
    fun `tag filter handles single-character tags without false positives`() = runTest {
        provider.store("tagged a", MemoryMetadata(tags = listOf("a")))
        provider.store("tagged ab", MemoryMetadata(tags = listOf("ab")))
        provider.store("tagged ba", MemoryMetadata(tags = listOf("ba")))

        val results = provider.list(MemoryFilter(tags = listOf("a")))

        assertEquals(1, results.size)
        assertEquals("tagged a", results[0].content)
    }

    @Test
    fun `stored tags roundtrip correctly with normalization`() = runTest {
        val id = provider.store(
            "test content",
            MemoryMetadata(tags = listOf("alpha", "beta", "gamma"))
        )

        val results = provider.list()
        assertEquals(1, results.size)
        assertEquals(listOf("alpha", "beta", "gamma"), results[0].tags)
    }

    @Test
    fun `empty tags stored and retrieved correctly`() = runTest {
        provider.store("no tags", MemoryMetadata(tags = emptyList()))

        val results = provider.list()
        assertEquals(1, results.size)
        assertEquals(emptyList(), results[0].tags)
    }

    @Test
    fun `migration normalizes existing non-normalized tags`() = runTest {
        // Simulate pre-migration data by inserting directly
        val values = android.content.ContentValues().apply {
            put("id", "legacy-1")
            put("content", "old format")
            put("tags", "work,urgent")  // old format: no leading/trailing commas
            put("source", "test")
            put("created_at", System.currentTimeMillis())
        }
        db.insert("memories", null, values)

        // Re-create provider to trigger migration
        provider = SqliteMemoryProvider(db)

        val results = provider.list(MemoryFilter(tags = listOf("work")))
        assertEquals(1, results.size)
        assertEquals("old format", results[0].content)
        assertEquals(listOf("work", "urgent"), results[0].tags)
    }

    @Test
    fun `tag filter is case insensitive`() = runTest {
        provider.store("meeting notes", MemoryMetadata(tags = listOf("Work")))
        provider.store("gym routine", MemoryMetadata(tags = listOf("personal")))

        val results = provider.list(MemoryFilter(tags = listOf("work")))
        assertEquals(1, results.size)
        assertEquals("meeting notes", results[0].content)
    }

    @Test
    fun `tags with mixed case are stored lowercase`() = runTest {
        provider.store("item", MemoryMetadata(tags = listOf("URGENT", "Work", "personal")))

        val results = provider.list()
        assertEquals(1, results.size)
        assertEquals(listOf("urgent", "work", "personal"), results[0].tags)
    }

    @Test
    fun `special characters in tags match exactly`() = runTest {
        provider.store("cpp project", MemoryMetadata(tags = listOf("c++")))
        provider.store("node project", MemoryMetadata(tags = listOf("node.js")))

        val cppResults = provider.list(MemoryFilter(tags = listOf("c++")))
        assertEquals(1, cppResults.size)
        assertEquals("cpp project", cppResults[0].content)

        val nodeResults = provider.list(MemoryFilter(tags = listOf("node.js")))
        assertEquals(1, nodeResults.size)
        assertEquals("node project", nodeResults[0].content)
    }
}
