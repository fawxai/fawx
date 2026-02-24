package ai.citros.core

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase

/** Baseline schema + entities + DAO for action playbooks. */
data class PlaybookEntity(
    val id: Long = 0,
    val appPackage: String,
    val taskType: String,
    val description: String? = null,
    val parameterSchema: String? = null,
    val successCount: Int = 1,
    val failCount: Int = 0,
    val confidence: Float = 0.5f,
    val appVersionCode: Int? = null,
    val createdAt: Long,
    val lastUsedAt: Long,
    val lastSucceededAt: Long? = null,
    val shared: Boolean = false,
    val source: String = "local"
)

data class PlaybookStepEntity(
    val id: Long = 0,
    val playbookId: Long,
    val stepOrder: Int,
    val screenFingerprint: String,
    val screenPackage: String? = null,
    val screenActivity: String? = null,
    val toolName: String,
    val toolInputTemplate: String,
    val selectorStrategy: String,
    val selectorValue: String,
    val expectedNextFingerprint: String? = null,
    val settleTimeMs: Int = 1000,
    val alternatives: String? = null
)

object PlaybookSchema {
    fun create(database: SQLiteDatabase) {
        // Ensure ON DELETE CASCADE and future FK constraints are actually enforced by SQLite.
        database.execSQL("PRAGMA foreign_keys = ON")

        database.execSQL(
            """
            CREATE TABLE IF NOT EXISTS playbooks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                app_package TEXT NOT NULL,
                task_type TEXT NOT NULL,
                description TEXT,
                parameter_schema TEXT,
                success_count INTEGER NOT NULL DEFAULT 1,
                fail_count INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0.5,
                app_version_code INTEGER,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL,
                last_succeeded_at INTEGER,
                shared INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'local'
            )
            """.trimIndent()
        )

        database.execSQL(
            """
            CREATE TABLE IF NOT EXISTS playbook_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                playbook_id INTEGER NOT NULL,
                step_order INTEGER NOT NULL,
                screen_fingerprint TEXT NOT NULL,
                screen_package TEXT,
                screen_activity TEXT,
                tool_name TEXT NOT NULL,
                tool_input_template TEXT NOT NULL,
                selector_strategy TEXT NOT NULL,
                selector_value TEXT NOT NULL,
                expected_next_fingerprint TEXT,
                settle_time_ms INTEGER NOT NULL DEFAULT 1000,
                alternatives TEXT,
                UNIQUE(playbook_id, step_order),
                FOREIGN KEY(playbook_id) REFERENCES playbooks(id) ON DELETE CASCADE
            )
            """.trimIndent()
        )

        database.execSQL("CREATE INDEX IF NOT EXISTS idx_playbook_lookup ON playbooks(app_package, task_type)")
        database.execSQL("CREATE INDEX IF NOT EXISTS idx_playbook_confidence ON playbooks(confidence DESC)")
        database.execSQL("CREATE INDEX IF NOT EXISTS idx_step_fingerprint ON playbook_steps(screen_fingerprint)")
        database.execSQL("CREATE INDEX IF NOT EXISTS idx_step_playbook ON playbook_steps(playbook_id, step_order)")
    }
}

interface PlaybookDao {
    fun insertPlaybook(entity: PlaybookEntity): Long
    fun insertStep(entity: PlaybookStepEntity): Long
    fun findByAppAndType(appPackage: String, taskType: String): List<PlaybookEntity>
    fun getPlaybook(playbookId: Long): PlaybookEntity?
    fun getSteps(playbookId: Long): List<PlaybookStepEntity>
    fun incrementSuccess(playbookId: Long)
    fun incrementFail(playbookId: Long)
    fun updateConfidence(playbookId: Long, confidence: Float)
    fun recordExecution(playbookId: Long, success: Boolean)
}

class SqlitePlaybookDao(
    private val database: SQLiteDatabase
) : PlaybookDao {
    override fun insertPlaybook(entity: PlaybookEntity): Long {
        val values = ContentValues().apply {
            put("app_package", entity.appPackage)
            put("task_type", entity.taskType)
            put("description", entity.description)
            put("parameter_schema", entity.parameterSchema)
            put("success_count", entity.successCount)
            put("fail_count", entity.failCount)
            put("confidence", entity.confidence)
            put("app_version_code", entity.appVersionCode)
            put("created_at", entity.createdAt)
            put("last_used_at", entity.lastUsedAt)
            put("last_succeeded_at", entity.lastSucceededAt)
            put("shared", if (entity.shared) 1 else 0)
            put("source", entity.source)
        }
        return database.insertOrThrow("playbooks", null, values)
    }

    override fun insertStep(entity: PlaybookStepEntity): Long {
        val values = ContentValues().apply {
            put("playbook_id", entity.playbookId)
            put("step_order", entity.stepOrder)
            put("screen_fingerprint", entity.screenFingerprint)
            put("screen_package", entity.screenPackage)
            put("screen_activity", entity.screenActivity)
            put("tool_name", entity.toolName)
            put("tool_input_template", entity.toolInputTemplate)
            put("selector_strategy", entity.selectorStrategy)
            put("selector_value", entity.selectorValue)
            put("expected_next_fingerprint", entity.expectedNextFingerprint)
            put("settle_time_ms", entity.settleTimeMs)
            put("alternatives", entity.alternatives)
        }
        return database.insertOrThrow("playbook_steps", null, values)
    }

    override fun getPlaybook(playbookId: Long): PlaybookEntity? {
        val cursor = database.rawQuery(
            """
            SELECT id, app_package, task_type, description, parameter_schema, success_count, fail_count,
                   confidence, app_version_code, created_at, last_used_at, last_succeeded_at, shared, source
            FROM playbooks
            WHERE id = ?
            LIMIT 1
            """.trimIndent(),
            arrayOf(playbookId.toString())
        )

        cursor.use {
            if (!it.moveToFirst()) return null
            return PlaybookEntity(
                id = it.getLong(it.getColumnIndexOrThrow("id")),
                appPackage = it.getString(it.getColumnIndexOrThrow("app_package")),
                taskType = it.getString(it.getColumnIndexOrThrow("task_type")),
                description = it.getString(it.getColumnIndexOrThrow("description")),
                parameterSchema = it.getString(it.getColumnIndexOrThrow("parameter_schema")),
                successCount = it.getInt(it.getColumnIndexOrThrow("success_count")),
                failCount = it.getInt(it.getColumnIndexOrThrow("fail_count")),
                confidence = it.getFloat(it.getColumnIndexOrThrow("confidence")),
                appVersionCode = it.getColumnIndexOrThrow("app_version_code").let { idx -> if (it.isNull(idx)) null else it.getInt(idx) },
                createdAt = it.getLong(it.getColumnIndexOrThrow("created_at")),
                lastUsedAt = it.getLong(it.getColumnIndexOrThrow("last_used_at")),
                lastSucceededAt = it.getColumnIndexOrThrow("last_succeeded_at").let { idx -> if (it.isNull(idx)) null else it.getLong(idx) },
                shared = it.getInt(it.getColumnIndexOrThrow("shared")) == 1,
                source = it.getString(it.getColumnIndexOrThrow("source"))
            )
        }
    }

    override fun findByAppAndType(appPackage: String, taskType: String): List<PlaybookEntity> {
        val cursor = database.rawQuery(
            """
            SELECT id, app_package, task_type, description, parameter_schema, success_count, fail_count,
                   confidence, app_version_code, created_at, last_used_at, last_succeeded_at, shared, source
            FROM playbooks
            WHERE app_package = ? AND task_type = ?
            ORDER BY confidence DESC
            """.trimIndent(),
            arrayOf(appPackage, taskType)
        )

        cursor.use {
            val idIndex = it.getColumnIndexOrThrow("id")
            val appPackageIndex = it.getColumnIndexOrThrow("app_package")
            val taskTypeIndex = it.getColumnIndexOrThrow("task_type")
            val descriptionIndex = it.getColumnIndexOrThrow("description")
            val parameterSchemaIndex = it.getColumnIndexOrThrow("parameter_schema")
            val successCountIndex = it.getColumnIndexOrThrow("success_count")
            val failCountIndex = it.getColumnIndexOrThrow("fail_count")
            val confidenceIndex = it.getColumnIndexOrThrow("confidence")
            val appVersionCodeIndex = it.getColumnIndexOrThrow("app_version_code")
            val createdAtIndex = it.getColumnIndexOrThrow("created_at")
            val lastUsedAtIndex = it.getColumnIndexOrThrow("last_used_at")
            val lastSucceededAtIndex = it.getColumnIndexOrThrow("last_succeeded_at")
            val sharedIndex = it.getColumnIndexOrThrow("shared")
            val sourceIndex = it.getColumnIndexOrThrow("source")

            val rows = mutableListOf<PlaybookEntity>()
            while (it.moveToNext()) {
                rows += PlaybookEntity(
                    id = it.getLong(idIndex),
                    appPackage = it.getString(appPackageIndex),
                    taskType = it.getString(taskTypeIndex),
                    description = it.getString(descriptionIndex),
                    parameterSchema = it.getString(parameterSchemaIndex),
                    successCount = it.getInt(successCountIndex),
                    failCount = it.getInt(failCountIndex),
                    confidence = it.getFloat(confidenceIndex),
                    appVersionCode = if (it.isNull(appVersionCodeIndex)) null else it.getInt(appVersionCodeIndex),
                    createdAt = it.getLong(createdAtIndex),
                    lastUsedAt = it.getLong(lastUsedAtIndex),
                    lastSucceededAt = if (it.isNull(lastSucceededAtIndex)) null else it.getLong(lastSucceededAtIndex),
                    shared = it.getInt(sharedIndex) == 1,
                    source = it.getString(sourceIndex)
                )
            }
            return rows
        }
    }

    override fun getSteps(playbookId: Long): List<PlaybookStepEntity> {
        val cursor = database.rawQuery(
            """
            SELECT id, playbook_id, step_order, screen_fingerprint, screen_package, screen_activity,
                   tool_name, tool_input_template, selector_strategy, selector_value,
                   expected_next_fingerprint, settle_time_ms, alternatives
            FROM playbook_steps
            WHERE playbook_id = ?
            ORDER BY step_order ASC
            """.trimIndent(),
            arrayOf(playbookId.toString())
        )

        cursor.use {
            val idIndex = it.getColumnIndexOrThrow("id")
            val playbookIdIndex = it.getColumnIndexOrThrow("playbook_id")
            val stepOrderIndex = it.getColumnIndexOrThrow("step_order")
            val screenFingerprintIndex = it.getColumnIndexOrThrow("screen_fingerprint")
            val screenPackageIndex = it.getColumnIndexOrThrow("screen_package")
            val screenActivityIndex = it.getColumnIndexOrThrow("screen_activity")
            val toolNameIndex = it.getColumnIndexOrThrow("tool_name")
            val toolInputTemplateIndex = it.getColumnIndexOrThrow("tool_input_template")
            val selectorStrategyIndex = it.getColumnIndexOrThrow("selector_strategy")
            val selectorValueIndex = it.getColumnIndexOrThrow("selector_value")
            val expectedNextFingerprintIndex = it.getColumnIndexOrThrow("expected_next_fingerprint")
            val settleTimeMsIndex = it.getColumnIndexOrThrow("settle_time_ms")
            val alternativesIndex = it.getColumnIndexOrThrow("alternatives")

            val rows = mutableListOf<PlaybookStepEntity>()
            while (it.moveToNext()) {
                rows += PlaybookStepEntity(
                    id = it.getLong(idIndex),
                    playbookId = it.getLong(playbookIdIndex),
                    stepOrder = it.getInt(stepOrderIndex),
                    screenFingerprint = it.getString(screenFingerprintIndex),
                    screenPackage = it.getString(screenPackageIndex),
                    screenActivity = it.getString(screenActivityIndex),
                    toolName = it.getString(toolNameIndex),
                    toolInputTemplate = it.getString(toolInputTemplateIndex),
                    selectorStrategy = it.getString(selectorStrategyIndex),
                    selectorValue = it.getString(selectorValueIndex),
                    expectedNextFingerprint = it.getString(expectedNextFingerprintIndex),
                    settleTimeMs = it.getInt(settleTimeMsIndex),
                    alternatives = it.getString(alternativesIndex)
                )
            }
            return rows
        }
    }

    override fun incrementSuccess(playbookId: Long) {
        val now = System.currentTimeMillis()
        database.execSQL(
            "UPDATE playbooks SET success_count = success_count + 1, last_used_at = ?, last_succeeded_at = ? WHERE id = ?",
            arrayOf(now, now, playbookId)
        )
    }

    override fun incrementFail(playbookId: Long) {
        database.execSQL(
            "UPDATE playbooks SET fail_count = fail_count + 1, last_used_at = ? WHERE id = ?",
            arrayOf(System.currentTimeMillis(), playbookId)
        )
    }

    override fun updateConfidence(playbookId: Long, confidence: Float) {
        database.execSQL(
            "UPDATE playbooks SET confidence = ? WHERE id = ?",
            arrayOf(confidence, playbookId)
        )
    }

    override fun recordExecution(playbookId: Long, success: Boolean) {
        val now = System.currentTimeMillis()
        if (success) {
            database.execSQL(
                """
                UPDATE playbooks
                SET success_count = success_count + 1,
                    last_used_at = ?,
                    last_succeeded_at = ?,
                    confidence = CAST(success_count + 1 AS REAL) / (success_count + fail_count + 1)
                WHERE id = ?
                """.trimIndent(),
                arrayOf(now, now, playbookId)
            )
        } else {
            database.execSQL(
                """
                UPDATE playbooks
                SET fail_count = fail_count + 1,
                    last_used_at = ?,
                    confidence = CAST(success_count AS REAL) / (success_count + fail_count + 1)
                WHERE id = ?
                """.trimIndent(),
                arrayOf(now, playbookId)
            )
        }
    }
}
