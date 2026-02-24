package ai.citros.core

import android.content.Context
import android.database.sqlite.SQLiteConstraintException
import android.database.sqlite.SQLiteDatabase
import androidx.test.core.app.ApplicationProvider
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mockito.kotlin.argumentCaptor
import org.mockito.kotlin.mock
import org.mockito.kotlin.verify

class PlaybookStoreTest {
    @Test
    fun schemaCreate_emitsPlaybookTablesAndIndices() {
        val database = mock<SQLiteDatabase>()

        PlaybookSchema.create(database)

        val sqlCaptor = argumentCaptor<String>()
        verify(database, org.mockito.kotlin.atLeast(1)).execSQL(sqlCaptor.capture())

        val sqlStatements = sqlCaptor.allValues.joinToString("\n")
        assertTrue(sqlStatements.contains("PRAGMA foreign_keys = ON"))
        assertTrue(sqlStatements.contains("CREATE TABLE IF NOT EXISTS playbooks"))
        assertTrue(sqlStatements.contains("CREATE TABLE IF NOT EXISTS playbook_steps"))
        assertTrue(sqlStatements.contains("ON DELETE CASCADE"))
        assertTrue(sqlStatements.contains("idx_playbook_lookup"))
        assertTrue(sqlStatements.contains("idx_step_playbook"))
    }

    @Test
    fun dao_insertAndFindByAppAndType_roundTripsPlaybookFields() {
        withInMemoryDatabase { db ->
            PlaybookSchema.create(db)
            val dao = SqlitePlaybookDao(db)

            val insertedId = dao.insertPlaybook(
                PlaybookEntity(
                    appPackage = "com.messages",
                    taskType = "reply",
                    description = "reply to mom",
                    parameterSchema = "{\"text\":\"string\"}",
                    successCount = 4,
                    failCount = 1,
                    confidence = 0.9f,
                    appVersionCode = 123,
                    createdAt = 1000L,
                    lastUsedAt = 2000L,
                    lastSucceededAt = 1900L,
                    shared = true,
                    source = "learned"
                )
            )

            val rows = dao.findByAppAndType("com.messages", "reply")
            assertEquals(1, rows.size)
            assertEquals(
                PlaybookEntity(
                    id = insertedId,
                    appPackage = "com.messages",
                    taskType = "reply",
                    description = "reply to mom",
                    parameterSchema = "{\"text\":\"string\"}",
                    successCount = 4,
                    failCount = 1,
                    confidence = 0.9f,
                    appVersionCode = 123,
                    createdAt = 1000L,
                    lastUsedAt = 2000L,
                    lastSucceededAt = 1900L,
                    shared = true,
                    source = "learned"
                ),
                rows.single()
            )
        }
    }

    @Test
    fun dao_getSteps_returnsOrderedStepsWithAllFields() {
        withInMemoryDatabase { db ->
            PlaybookSchema.create(db)
            val dao = SqlitePlaybookDao(db)

            val playbookId = dao.insertPlaybook(
                PlaybookEntity(
                    appPackage = "com.messages",
                    taskType = "reply",
                    createdAt = 100L,
                    lastUsedAt = 100L
                )
            )

            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = playbookId,
                    stepOrder = 2,
                    screenFingerprint = "fp2",
                    screenPackage = "com.messages",
                    screenActivity = "ComposeActivity",
                    toolName = "tap_text",
                    toolInputTemplate = "{\"text\":\"Send\"}",
                    selectorStrategy = "text",
                    selectorValue = "Send",
                    expectedNextFingerprint = "fp3",
                    settleTimeMs = 1200,
                    alternatives = "[]"
                )
            )
            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = playbookId,
                    stepOrder = 1,
                    screenFingerprint = "fp1",
                    toolName = "type_text",
                    toolInputTemplate = "{\"text\":\"hello\"}",
                    selectorStrategy = "id",
                    selectorValue = "compose",
                    settleTimeMs = 900
                )
            )

            val steps = dao.getSteps(playbookId)
            assertEquals(listOf(1, 2), steps.map { it.stepOrder })
            assertEquals("fp1", steps[0].screenFingerprint)
            assertEquals("type_text", steps[0].toolName)
            assertEquals("fp2", steps[1].screenFingerprint)
            assertEquals("ComposeActivity", steps[1].screenActivity)
            assertEquals("fp3", steps[1].expectedNextFingerprint)
        }
    }

    @Test(expected = SQLiteConstraintException::class)
    fun dao_insertStep_rejectsDuplicateStepOrderWithinPlaybook() {
        withInMemoryDatabase { db ->
            PlaybookSchema.create(db)
            val dao = SqlitePlaybookDao(db)

            val playbookId = dao.insertPlaybook(
                PlaybookEntity(
                    appPackage = "com.messages",
                    taskType = "reply",
                    createdAt = 100L,
                    lastUsedAt = 100L
                )
            )

            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = playbookId,
                    stepOrder = 1,
                    screenFingerprint = "fp1",
                    toolName = "tap",
                    toolInputTemplate = "{}",
                    selectorStrategy = "text",
                    selectorValue = "Send"
                )
            )

            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = playbookId,
                    stepOrder = 1,
                    screenFingerprint = "fp2",
                    toolName = "tap",
                    toolInputTemplate = "{}",
                    selectorStrategy = "text",
                    selectorValue = "Send"
                )
            )
        }
    }

    private fun withInMemoryDatabase(block: (SQLiteDatabase) -> Unit) {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val name = "playbook-store-test-${UUID.randomUUID()}"
        val database = context.openOrCreateDatabase(name, Context.MODE_PRIVATE, null)
        try {
            block(database)
        } finally {
            database.close()
            context.deleteDatabase(name)
        }
    }
}
