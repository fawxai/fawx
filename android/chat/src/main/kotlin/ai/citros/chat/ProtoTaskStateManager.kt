package ai.citros.chat

import ai.citros.chat.proto.TaskStateProto
import ai.citros.core.Message
import ai.citros.core.SerializedToolCall
import ai.citros.core.TaskState
import ai.citros.core.TaskStateManager
import ai.citros.core.TaskStatus
import androidx.datastore.core.CorruptionException
import androidx.datastore.core.DataStore
import androidx.datastore.core.DataStoreFactory
import androidx.datastore.core.Serializer
import androidx.datastore.dataStoreFile
import android.content.Context
import android.util.Log
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import java.io.InputStream
import java.io.OutputStream

class ProtoTaskStateManager(
    context: Context,
    private val fileName: String = DEFAULT_FILE_NAME,
    private val clock: () -> Long = System::currentTimeMillis,
    private val dataStore: DataStore<TaskStateProto> = DataStoreFactory.create(
        serializer = TaskStateSerializer,
        produceFile = { context.dataStoreFile(fileName) }
    )
) : TaskStateManager {

    override suspend fun checkpoint(state: TaskState) {
        dataStore.updateData {
            state.toProto()
        }
    }

    override suspend fun loadPending(staleThresholdMs: Long): TaskState? {
        val snapshot = dataStore.updateData { current ->
            if (current.taskId.isBlank()) return@updateData current
            if (current.status == TaskStateProto.TaskStatus.ACTIVE) {
                current.toBuilder()
                    .setStatus(TaskStateProto.TaskStatus.INTERRUPTED)
                    .build()
            } else {
                current
            }
        }

        if (snapshot.taskId.isBlank()) return null

        val ageMs = clock() - snapshot.lastCheckpointMs
        if (ageMs > staleThresholdMs) {
            clear()
            return null
        }

        return snapshot.toDomain()
    }

    override suspend fun clear() {
        dataStore.updateData { TaskStateProto.getDefaultInstance() }
    }

    private fun TaskState.toProto(): TaskStateProto {
        val builder = TaskStateProto.newBuilder()
            .setTaskId(taskId)
            .setUserMessage(userMessage)
            .setCurrentStep(currentStep)
            .setMaxSteps(maxSteps)
            .setStartedAtMs(startedAtMs)
            .setLastCheckpointMs(lastCheckpointMs)
            .setStatus(status.toProto())
            .setSubtaskInProgress(subtaskInProgress)
            .setSubtaskDepth(subtaskDepth)

        if (!subtaskGoal.isNullOrBlank()) {
            builder.subtaskGoal = subtaskGoal
        }

        conversationHistory.forEach { msg ->
            builder.addConversationMessagesJson(json.encodeToString(msg))
        }

        pendingToolCalls.forEach { tool ->
            builder.addPendingToolCalls(
                TaskStateProto.PendingToolCall.newBuilder()
                    .setId(tool.id)
                    .setName(tool.name)
                    .setInputJson(tool.inputJson)
                    .build()
            )
        }

        return builder.build()
    }

    private fun TaskStateProto.toDomain(): TaskState {
        val conversation = conversationMessagesJsonList.mapIndexedNotNull { index, encoded ->
            runCatching { json.decodeFromString<Message>(encoded) }
                .onFailure {
                    Log.w(TAG, "Dropping malformed conversation message at index=$index", it)
                }
                .getOrNull()
        }

        val pending = pendingToolCallsList.map {
            SerializedToolCall(
                id = it.id,
                name = it.name,
                inputJson = it.inputJson
            )
        }

        return TaskState(
            taskId = taskId,
            userMessage = userMessage,
            conversationHistory = conversation,
            currentStep = currentStep,
            maxSteps = maxSteps,
            startedAtMs = startedAtMs,
            lastCheckpointMs = lastCheckpointMs,
            pendingToolCalls = pending,
            status = status.toDomain(),
            subtaskInProgress = subtaskInProgress,
            subtaskGoal = subtaskGoal.takeIf { it.isNotBlank() },
            subtaskDepth = subtaskDepth
        )
    }

    private fun TaskStatus.toProto(): TaskStateProto.TaskStatus = when (this) {
        TaskStatus.ACTIVE -> TaskStateProto.TaskStatus.ACTIVE
        TaskStatus.INTERRUPTED -> TaskStateProto.TaskStatus.INTERRUPTED
        TaskStatus.COMPLETED -> TaskStateProto.TaskStatus.COMPLETED
        TaskStatus.FAILED -> TaskStateProto.TaskStatus.FAILED
    }

    private fun TaskStateProto.TaskStatus.toDomain(): TaskStatus = when (this) {
        TaskStateProto.TaskStatus.ACTIVE -> TaskStatus.ACTIVE
        TaskStateProto.TaskStatus.INTERRUPTED -> TaskStatus.INTERRUPTED
        TaskStateProto.TaskStatus.COMPLETED -> TaskStatus.COMPLETED
        TaskStateProto.TaskStatus.FAILED,
        TaskStateProto.TaskStatus.TASK_STATUS_UNSPECIFIED,
        TaskStateProto.TaskStatus.UNRECOGNIZED -> TaskStatus.FAILED
    }

    companion object {
        private const val TAG = "ProtoTaskStateManager"
        private const val DEFAULT_FILE_NAME = "task_state.pb"
        private val json = Json { ignoreUnknownKeys = true }
    }
}

object TaskStateSerializer : Serializer<TaskStateProto> {
    override val defaultValue: TaskStateProto = TaskStateProto.getDefaultInstance()

    override suspend fun readFrom(input: InputStream): TaskStateProto {
        return try {
            TaskStateProto.parseFrom(input)
        } catch (e: Exception) {
            throw CorruptionException("Cannot read task_state.pb", e)
        }
    }

    override suspend fun writeTo(t: TaskStateProto, output: OutputStream) {
        t.writeTo(output)
    }
}
