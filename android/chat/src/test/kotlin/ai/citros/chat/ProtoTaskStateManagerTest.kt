package ai.citros.chat

import ai.citros.core.Message
import ai.citros.core.SerializedToolCall
import ai.citros.core.TaskState
import ai.citros.core.TaskStatus
import ai.citros.core.TaskStateManager
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.test.runTest
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.util.UUID
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

@RunWith(RobolectricTestRunner::class)
class ProtoTaskStateManagerTest {

    @Test
    fun `checkpoint then loadPending round-trips task state`() = runTest {
        val now = 1_700_000_000_000L
        val manager = ProtoTaskStateManager(
            context = ApplicationProvider.getApplicationContext(),
            fileName = "task-state-${UUID.randomUUID()}.pb",
            clock = { now }
        )

        val state = TaskState(
            taskId = "task-1",
            userMessage = "open gmail",
            conversationHistory = listOf(
                Message(role = "user", content = "open gmail"),
                Message(role = "assistant", content = "Opening Gmail")
            ),
            currentStep = 2,
            maxSteps = 25,
            startedAtMs = now - 10_000,
            lastCheckpointMs = now,
            pendingToolCalls = listOf(
                SerializedToolCall(id = "tool-3", name = "tap", inputJson = "{\"element_id\":3}")
            ),
            status = TaskStatus.ACTIVE
        )

        manager.checkpoint(state)
        val loaded = manager.loadPending()

        assertNotNull(loaded)
        assertEquals("task-1", loaded.taskId)
        assertEquals("open gmail", loaded.userMessage)
        assertEquals(2, loaded.currentStep)
        assertEquals(25, loaded.maxSteps)
        assertEquals(TaskStatus.INTERRUPTED, loaded.status)
        assertEquals(2, loaded.conversationHistory.size)
        assertEquals("tap", loaded.pendingToolCalls.single().name)
    }

    @Test
    fun `loadPending discards stale checkpoints`() = runTest {
        val createdAt = 1_700_000_000_000L
        var now = createdAt

        val manager = ProtoTaskStateManager(
            context = ApplicationProvider.getApplicationContext(),
            fileName = "task-state-${UUID.randomUUID()}.pb",
            clock = { now }
        )

        manager.checkpoint(
            TaskState(
                taskId = "task-stale",
                userMessage = "do thing",
                conversationHistory = emptyList(),
                currentStep = 1,
                maxSteps = 25,
                startedAtMs = createdAt,
                lastCheckpointMs = createdAt,
                pendingToolCalls = emptyList(),
                status = TaskStatus.ACTIVE
            )
        )

        now = createdAt + TaskStateManager.STALE_THRESHOLD_MS + 1

        val stale = manager.loadPending()
        assertNull(stale)

        val afterClear = manager.loadPending()
        assertNull(afterClear)
    }
}
