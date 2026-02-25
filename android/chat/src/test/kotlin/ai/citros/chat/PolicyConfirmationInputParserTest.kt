package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class PolicyConfirmationInputParserTest {

    @Test
    fun `parse accepts natural-language approval phrases`() {
        assertEquals(true, PolicyConfirmationInputParser.parse("you have permission"))
        assertEquals(true, PolicyConfirmationInputParser.parse("go ahead and do it"))
        assertEquals(true, PolicyConfirmationInputParser.parse("I grant permission"))
    }

    @Test
    fun `parse accepts denial phrases`() {
        assertEquals(false, PolicyConfirmationInputParser.parse("no"))
        assertEquals(false, PolicyConfirmationInputParser.parse("don't do that"))
        assertEquals(false, PolicyConfirmationInputParser.parse("not now, cancel"))
    }

    @Test
    fun `parse rejects negated approval tokens`() {
        assertEquals(false, PolicyConfirmationInputParser.parse("not sure"))
        assertEquals(false, PolicyConfirmationInputParser.parse("not ok"))
        assertEquals(false, PolicyConfirmationInputParser.parse("not okay"))
        assertEquals(false, PolicyConfirmationInputParser.parse("never approve"))
        assertEquals(false, PolicyConfirmationInputParser.parse("not really"))
    }

    @Test
    fun `parse returns null for unrelated text`() {
        assertNull(PolicyConfirmationInputParser.parse("what is blocking you from using accessibility"))
        assertNull(PolicyConfirmationInputParser.parse(""))
    }

    @Test
    fun `parse avoids contextual false positives on long question-style follow ups`() {
        assertNull(PolicyConfirmationInputParser.parse("Can you resume from where you left off"))
        assertNull(PolicyConfirmationInputParser.parse("Could you continue explaining that part"))
    }

    @Test
    fun `parse ignores very long contextual messages even when they contain approval tokens`() {
        assertNull(
            PolicyConfirmationInputParser.parse(
                "I know you asked for permission but can you continue with the explanation first"
            )
        )
    }

    @Test
    fun `parse handles punctuation and emoji edge cases`() {
        assertEquals(true, PolicyConfirmationInputParser.parse("Yes!!!"))
        assertEquals(false, PolicyConfirmationInputParser.parse("No 🚫"))
        assertNull(PolicyConfirmationInputParser.parse("🤔🤔🤔"))
    }
}
