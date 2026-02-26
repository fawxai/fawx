package ai.citros.core

import android.os.Bundle
import android.text.InputType
import android.util.Log
import android.view.accessibility.AccessibilityNodeInfo
import java.util.concurrent.TimeUnit

enum class InputTier {
    SET_TEXT,
    CLIPBOARD_PASTE,
    KEY_EVENTS,
    ADB_INPUT
}

sealed class InputResult {
    data class Success(val tier: InputTier) : InputResult()
    data class Fallback(val tier: InputTier) : InputResult()
    data class Failed(val tier: InputTier? = null) : InputResult()
}

/**
 * Robust text input chain:
 * 1) ACTION_SET_TEXT
 * 2) Clipboard paste (skipped for password fields)
 * 3) Char-by-char key events (30ms, then 100ms retry)
 * 4) ADB `input text` fallback
 */
class RobustTextInput(
    private val findFocusedInputNode: () -> AccessibilityNodeInfo? = { findFocusedInputNodeFromScreenReader() },
    private val clipboardWrite: (String) -> Boolean = { ClipboardHelper.write(it) },
    private val clipboardClear: () -> Unit = { ClipboardHelper.write("") },
    private val dispatchCharacter: (Char) -> Boolean = { dispatchCharacterWithAdb(it) },
    private val adbInputText: (String) -> Boolean = { adbInputTextDefault(it) },
    private val sleepMs: (Long) -> Unit = { Thread.sleep(it) }
) {

    fun inputText(text: String, verify: Boolean = true): InputResult {
        val node = findFocusedInputNode() ?: return InputResult.Failed()
        return try {
            inputText(node, text, verify)
        } finally {
            node.recycle()
        }
    }

    internal fun inputText(node: AccessibilityNodeInfo, text: String, verify: Boolean = true): InputResult {
        Log.d(TAG, "Tier 1 attempt: ACTION_SET_TEXT")
        if (performSetText(node, text) && (!verify || verifyText(node, text))) {
            Log.d(TAG, "Tier 1 success: ACTION_SET_TEXT")
            return InputResult.Success(InputTier.SET_TEXT)
        }
        Log.d(TAG, "Tier 1 failed: ACTION_SET_TEXT")

        if (!isPasswordField(node)) {
            Log.d(TAG, "Tier 2 attempt: CLIPBOARD_PASTE")
            val pasteDispatched = try {
                if (clipboardWrite(text)) node.performAction(AccessibilityNodeInfo.ACTION_PASTE) else false
            } finally {
                runCatching { clipboardClear() }
                    .onFailure { Log.d(TAG, "Tier 2 cleanup failed: clipboardClear", it) }
            }
            if (pasteDispatched && (!verify || verifyText(node, text))) {
                Log.d(TAG, "Tier 2 success: CLIPBOARD_PASTE")
                return InputResult.Fallback(InputTier.CLIPBOARD_PASTE)
            }
            Log.d(TAG, "Tier 2 failed: CLIPBOARD_PASTE")
        } else {
            Log.d(TAG, "Tier 2 skipped: password field")
        }

        Log.d(TAG, "Tier 3 attempt: KEY_EVENTS delay=30")
        if (typeByCharacters(node, text, 30L) && (!verify || verifyText(node, text))) {
            Log.d(TAG, "Tier 3 success: KEY_EVENTS delay=30")
            return InputResult.Fallback(InputTier.KEY_EVENTS)
        }
        Log.d(TAG, "Tier 3 failed: KEY_EVENTS delay=30")

        Log.d(TAG, "Tier 3 attempt: KEY_EVENTS delay=100")
        if (typeByCharacters(node, text, 100L) && (!verify || verifyText(node, text))) {
            Log.d(TAG, "Tier 3 success: KEY_EVENTS delay=100")
            return InputResult.Fallback(InputTier.KEY_EVENTS)
        }
        Log.d(TAG, "Tier 3 failed: KEY_EVENTS delay=100")

        Log.d(TAG, "Tier 4 attempt: ADB_INPUT")
        clearField(node)
        val adbOk = adbInputText(text)
        return if (adbOk && (!verify || verifyText(node, text))) {
            Log.d(TAG, "Tier 4 success: ADB_INPUT")
            InputResult.Fallback(InputTier.ADB_INPUT)
        } else {
            Log.d(TAG, "Tier 4 failed: ADB_INPUT")
            InputResult.Failed(InputTier.ADB_INPUT)
        }
    }

    /**
     * Verifies typed text by comparing trimmed values.
     *
     * Leading and trailing whitespace are intentionally ignored because many
     * target input flows normalize edges while preserving meaningful internal spacing.
     */
    internal fun verifyText(node: AccessibilityNodeInfo, expected: String): Boolean {
        val actual = node.text?.toString() ?: return false
        return actual.trim() == expected.trim()
    }

    internal fun clearField(node: AccessibilityNodeInfo): Boolean {
        val current = node.text?.toString().orEmpty()
        val selected = if (current.isNotEmpty()) {
            val args = Bundle().apply {
                putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, 0)
                putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, current.length)
            }
            node.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, args)
        } else {
            true
        }

        val deleteOk = if (selected) {
            node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "")
            })
        } else {
            false
        }
        return selected && deleteOk
    }

    private fun performSetText(node: AccessibilityNodeInfo, text: String): Boolean {
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        }
        return node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    private fun typeByCharacters(node: AccessibilityNodeInfo, text: String, delayMs: Long): Boolean {
        clearField(node)
        for (ch in text) {
            if (!dispatchCharacter(ch)) return false
            sleepMs(delayMs)
        }
        return true
    }

    private fun isPasswordField(node: AccessibilityNodeInfo): Boolean {
        val inputType = node.inputType
        return node.isPassword ||
            (inputType and InputType.TYPE_TEXT_VARIATION_PASSWORD) != 0 ||
            (inputType and InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD) != 0 ||
            (inputType and InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD) != 0 ||
            (inputType and InputType.TYPE_NUMBER_VARIATION_PASSWORD) != 0
    }

    companion object {
        private const val TAG = "RobustTextInput"

        internal fun adbInputTextDefault(text: String): Boolean = runInputText(text)

        internal fun dispatchCharacterWithAdb(ch: Char): Boolean = runInputText(ch.toString())

        private fun runInputText(text: String): Boolean {
            return try {
                val process = ProcessBuilder("input", "text", text)
                    .redirectErrorStream(true)
                    .start()
                if (!process.waitFor(5, TimeUnit.SECONDS)) {
                    process.destroyForcibly()
                    false
                } else {
                    process.exitValue() == 0
                }
            } catch (_: Exception) {
                false
            }
        }

        private fun findFocusedInputNodeFromScreenReader(): AccessibilityNodeInfo? {
            val service = ScreenReader.getService() ?: return null
            val root = service.rootInActiveWindow ?: return null
            try {
                return findFocusedEditableNode(root)
            } finally {
                root.recycle()
            }
        }

        private fun findFocusedEditableNode(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
            if (node.isEditable && node.isFocused) return AccessibilityNodeInfo.obtain(node)
            for (i in 0 until node.childCount) {
                val child = node.getChild(i) ?: continue
                val result = try {
                    findFocusedEditableNode(child)
                } finally {
                    child.recycle()
                }
                if (result != null) return result
            }
            return null
        }
    }
}
