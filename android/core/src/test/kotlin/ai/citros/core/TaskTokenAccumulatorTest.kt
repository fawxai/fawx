package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotSame

class TaskTokenAccumulatorTest {

    @Test
    fun `empty accumulator has zero totals`() {
        val accumulator = TaskTokenAccumulator()

        assertEquals(0L, accumulator.totalTokens)
        assertEquals(0L, accumulator.totalInputTokens)
        assertEquals(0L, accumulator.totalOutputTokens)
        assertEquals(0, accumulator.callCount)
        assertEquals(emptyList(), accumulator.snapshot())
    }

    @Test
    fun `single record updates totals`() {
        val accumulator = TaskTokenAccumulator()

        accumulator.record(TokenUsage(inputTokens = 120, outputTokens = 30))

        assertEquals(150L, accumulator.totalTokens)
        assertEquals(120L, accumulator.totalInputTokens)
        assertEquals(30L, accumulator.totalOutputTokens)
        assertEquals(1, accumulator.callCount)
    }

    @Test
    fun `multiple records accumulate totals`() {
        val accumulator = TaskTokenAccumulator()

        accumulator.record(TokenUsage(inputTokens = 100, outputTokens = 20))
        accumulator.record(TokenUsage(inputTokens = 50, outputTokens = 10))
        accumulator.record(TokenUsage(inputTokens = 25, outputTokens = 5))

        assertEquals(210L, accumulator.totalTokens)
        assertEquals(175L, accumulator.totalInputTokens)
        assertEquals(35L, accumulator.totalOutputTokens)
        assertEquals(3, accumulator.callCount)
    }

    @Test
    fun `reset clears all accumulated values`() {
        val accumulator = TaskTokenAccumulator()
        accumulator.record(TokenUsage(inputTokens = 10, outputTokens = 5))

        accumulator.reset()

        assertEquals(0L, accumulator.totalTokens)
        assertEquals(0L, accumulator.totalInputTokens)
        assertEquals(0L, accumulator.totalOutputTokens)
        assertEquals(0, accumulator.callCount)
        assertEquals(emptyList(), accumulator.snapshot())
    }

    @Test
    fun `snapshot is immutable copy`() {
        val accumulator = TaskTokenAccumulator()
        accumulator.record(TokenUsage(inputTokens = 10, outputTokens = 5))

        val snapshot = accumulator.snapshot().toMutableList()
        val originalSnapshot = accumulator.snapshot()
        snapshot += TokenUsage(inputTokens = 99, outputTokens = 1)

        assertEquals(1, originalSnapshot.size)
        assertEquals(10, originalSnapshot[0].inputTokens)
        assertEquals(5, originalSnapshot[0].outputTokens)
        assertNotSame(snapshot, originalSnapshot)
        assertEquals(15L, accumulator.totalTokens)
        assertEquals(1, accumulator.callCount)
    }

    @Test
    fun `concurrent record calls preserve all usage entries`() = runBlocking {
        val accumulator = TaskTokenAccumulator()
        val writers = 200

        (1..writers).map {
            async(Dispatchers.Default) {
                accumulator.record(TokenUsage(inputTokens = 3, outputTokens = 2))
            }
        }.awaitAll()

        assertEquals(writers, accumulator.callCount)
        assertEquals(600L, accumulator.totalInputTokens)
        assertEquals(400L, accumulator.totalOutputTokens)
        assertEquals(1_000L, accumulator.totalTokens)
    }
}
