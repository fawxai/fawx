package ai.citros.core

import android.os.Bundle
import android.text.InputType
import android.view.accessibility.AccessibilityNodeInfo
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.times
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class RobustTextInputTest {

    @Test
    fun `tier 1 success path setText works and verify passes`() {
        val node = mockNode(text = "Hello", setTextResult = true)
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { false },
            dispatchCharacter = { false },
            adbInputText = { false },
            sleepMs = {}
        )

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Success && result.tier == InputTier.SET_TEXT)
        verify(node).performAction(eq(AccessibilityNodeInfo.ACTION_SET_TEXT), any())
        verify(node, never()).performAction(eq(AccessibilityNodeInfo.ACTION_PASTE))
    }

    @Test
    fun `returns failed when no focused input node found`() {
        val input = RobustTextInput(findFocusedInputNode = { null }, sleepMs = {})

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Failed)
    }

    @Test
    fun `tier 1 failure falls back to tier 2 clipboard paste`() {
        val node = mockNode(text = "Hello", setTextResult = false, pasteResult = true)
        var clipboardWrites = 0
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { clipboardWrites++; true },
            dispatchCharacter = { false },
            adbInputText = { false },
            sleepMs = {}
        )

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Fallback && result.tier == InputTier.CLIPBOARD_PASTE)
        assertTrue(clipboardWrites >= 1)
        verify(node).performAction(eq(AccessibilityNodeInfo.ACTION_PASTE))
    }

    @Test
    fun `tier 2 skipped for password fields including isPassword flag`() {
        val node = mockNode(
            text = "",
            setTextResult = false,
            pasteResult = true,
            isPassword = true
        )

        var clipboardCalled = false
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { clipboardCalled = true; true },
            dispatchCharacter = { false },
            adbInputText = { false },
            sleepMs = {}
        )

        input.inputText("secret")

        assertFalse(clipboardCalled)
        verify(node, never()).performAction(eq(AccessibilityNodeInfo.ACTION_PASTE))
    }

    @Test
    fun `isPasswordField behavior covers visible web and number password`() {
        val cases = listOf(
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
            InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
        )

        for (inputType in cases) {
            val node = mockNode(
                text = "",
                setTextResult = false,
                pasteResult = true,
                inputType = inputType
            )
            var clipboardCalled = false
            val input = RobustTextInput(
                findFocusedInputNode = { node },
                clipboardWrite = { clipboardCalled = true; true },
                dispatchCharacter = { false },
                adbInputText = { false },
                sleepMs = {}
            )

            input.inputText("secret")
            assertFalse("clipboard should be skipped for inputType=$inputType", clipboardCalled)
        }
    }

    @Test
    fun `tier 2 clears clipboard immediately after paste even on clear failure`() {
        val node = mockNode(text = "Hello", setTextResult = false, pasteResult = true)
        var cleared = false
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { true },
            clipboardClear = { cleared = true; throw IllegalStateException("clear failed") },
            dispatchCharacter = { false },
            adbInputText = { false },
            sleepMs = {}
        )

        val result = input.inputText("Hello")

        assertTrue(cleared)
        assertTrue(result is InputResult.Fallback && result.tier == InputTier.CLIPBOARD_PASTE)
    }

    @Test
    fun `tier 3 adaptive delay retries at 100ms after 30ms verification failure`() {
        val node = mockNode(text = "", setTextResult = false, pasteResult = false)
        var clearCalls = 0
        whenever(node.performAction(eq(AccessibilityNodeInfo.ACTION_SET_TEXT), any())).thenAnswer { invocation ->
            val args = invocation.getArgument<Bundle>(1)
            val value = args.getCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE)?.toString().orEmpty()
            if (value.isEmpty()) clearCalls++
            false
        }
        whenever(node.text).thenAnswer { if (clearCalls >= 2) "Hello" else "bad" }

        val delays = mutableListOf<Long>()
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { false },
            dispatchCharacter = { true },
            adbInputText = { false },
            sleepMs = { delays += it }
        )

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Fallback && result.tier == InputTier.KEY_EVENTS)
        assertTrue(delays.contains(30L))
        assertTrue(delays.contains(100L))
    }

    @Test
    fun `tier 4 adb fallback success path returns fallback adb tier`() {
        val node = mockNode(text = "Hello", setTextResult = false, pasteResult = false)
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { false },
            dispatchCharacter = { false },
            adbInputText = { true },
            sleepMs = {}
        )

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Fallback && result.tier == InputTier.ADB_INPUT)
    }

    @Test
    fun `full chain failure returns Failed`() {
        val node = mockNode(text = "", setTextResult = false, pasteResult = false)
        val input = RobustTextInput(
            findFocusedInputNode = { node },
            clipboardWrite = { false },
            dispatchCharacter = { false },
            adbInputText = { false },
            sleepMs = {}
        )

        val result = input.inputText("Hello")

        assertTrue(result is InputResult.Failed)
    }

    @Test
    fun `verifyText matches trimmed text and rejects mismatch`() {
        val node = mockNode(text = "  Hello  ")
        val input = RobustTextInput(findFocusedInputNode = { node }, sleepMs = {})

        assertTrue(input.verifyText(node, "Hello"))
        assertFalse(input.verifyText(node, "World"))
    }

    @Test
    fun `clearField selects all and clears text`() {
        val node = mockNode(text = "Hello")
        val input = RobustTextInput(findFocusedInputNode = { node }, sleepMs = {})

        val result = input.clearField(node)

        assertTrue(result)
        verify(node).performAction(eq(AccessibilityNodeInfo.ACTION_SET_SELECTION), any())
        verify(node, times(1)).performAction(eq(AccessibilityNodeInfo.ACTION_SET_TEXT), any())
    }

    @Test
    fun `empty string input succeeds`() {
        val node = mockNode(text = "", setTextResult = true)
        val input = RobustTextInput(findFocusedInputNode = { node }, sleepMs = {})

        val result = input.inputText("", verify = true)

        assertTrue(result is InputResult.Success && result.tier == InputTier.SET_TEXT)
    }

    @Test
    fun `special characters input succeeds`() {
        val text = "héllo 🌍"
        val node = mockNode(text = text, setTextResult = true)
        val input = RobustTextInput(findFocusedInputNode = { node }, sleepMs = {})

        val result = input.inputText(text, verify = true)

        assertTrue(result is InputResult.Success && result.tier == InputTier.SET_TEXT)
    }

    private fun mockNode(
        text: CharSequence = "",
        setTextResult: Boolean = true,
        pasteResult: Boolean = false,
        inputType: Int = InputType.TYPE_CLASS_TEXT,
        isPassword: Boolean = false
    ): AccessibilityNodeInfo {
        val node = mock<AccessibilityNodeInfo>()
        whenever(node.isEditable).thenReturn(true)
        whenever(node.isFocused).thenReturn(true)
        whenever(node.inputType).thenReturn(inputType)
        whenever(node.isPassword).thenReturn(isPassword)
        whenever(node.text).thenReturn(text)
        whenever(node.performAction(eq(AccessibilityNodeInfo.ACTION_SET_TEXT), any())).thenReturn(setTextResult)
        whenever(node.performAction(eq(AccessibilityNodeInfo.ACTION_SET_SELECTION), any())).thenReturn(true)
        whenever(node.performAction(eq(AccessibilityNodeInfo.ACTION_PASTE))).thenReturn(pasteResult)
        whenever(node.recycle()).then {}
        return node
    }
}
