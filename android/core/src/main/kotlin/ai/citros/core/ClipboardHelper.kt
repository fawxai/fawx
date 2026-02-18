package ai.citros.core

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.accessibility.AccessibilityNodeInfo

/**
 * Clipboard helper that wraps Android's ClipboardManager.
 * Uses the AccessibilityService context for clipboard access.
 *
 * ## Android 13+ Restrictions
 * Starting with Android 13 (API 33), apps can only read clipboard content if they
 * are in the foreground or are the current IME. Since we operate through an
 * AccessibilityService, clipboard reading may be restricted on some devices.
 * The [read] method handles this gracefully by returning null when access is denied.
 */
object ClipboardHelper {

    private const val TAG = "ClipboardHelper"

    private var context: Context? = null

    /**
     * Attach the clipboard helper to an application context.
     * Uses [Context.getApplicationContext] to avoid activity/service leaks —
     * the clipboard manager outlives any single component.
     */
    fun attach(ctx: Context) {
        context = ctx.applicationContext
    }

    @Volatile
    private var clipListener: ClipboardManager.OnPrimaryClipChangedListener? = null

    @Synchronized
    fun detach() {
        stopListening()
        context = null
    }

    fun isAttached(): Boolean = context != null

    private fun getClipboardManager(): ClipboardManager? {
        val ctx = context ?: return null
        // CLIPBOARD_SERVICE is guaranteed to return ClipboardManager on all Android versions
        return ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }

    /**
     * Start listening for clipboard changes. The [callback] receives the new text
     * content (or null if non-text) whenever the primary clip changes.
     *
     * Only one listener is active at a time — calling this again replaces the previous one.
     *
     * Important lifecycle notes:
     * - If your callback captures an Activity/Fragment, call [stopListening] when it is destroyed.
     * - [detach] automatically calls [stopListening].
     * - If called while detached (no context), this is a no-op and listener remains inactive.
     *
     * Note: On Android 12L+ (API 32), background clipboard access may be restricted.
     * The listener fires, but [read] may return null if the app isn't in the foreground.
     */
    @Synchronized
    fun startListening(callback: (String?) -> Unit) {
        stopListening()
        val cm = getClipboardManager() ?: return
        val listener = ClipboardManager.OnPrimaryClipChangedListener {
            try {
                callback(read())
            } catch (e: RuntimeException) {
                Log.w(TAG, "Clipboard listener callback failed", e)
                // Keep listener chain resilient to callback failures.
            }
        }
        clipListener = listener
        cm.addPrimaryClipChangedListener(listener)
    }

    /**
     * Stop listening for clipboard changes and release the listener.
     */
    @Synchronized
    fun stopListening() {
        val cm = getClipboardManager()
        clipListener?.let { cm?.removePrimaryClipChangedListener(it) }
        clipListener = null
    }

    /** Whether a clipboard change listener is currently active. */
    @Synchronized
    fun isListening(): Boolean = clipListener != null

    /**
     * Read current clipboard text content.
     *
     * @return Clipboard text, or null if empty, non-text, or access denied (API 33+)
     */
    fun read(): String? {
        val cm = getClipboardManager() ?: return null
        return try {
            if (!cm.hasPrimaryClip()) return null
            val clip = cm.primaryClip ?: return null
            if (clip.itemCount == 0) return null

            // Check if it's text content
            val desc = clip.description
            if (desc != null && !desc.hasMimeType(ClipDescription.MIMETYPE_TEXT_PLAIN) &&
                !desc.hasMimeType(ClipDescription.MIMETYPE_TEXT_HTML)
            ) {
                return null
            }

            clip.getItemAt(0)?.coerceToText(context)?.toString()
        } catch (e: SecurityException) {
            // Android 13+ may throw SecurityException for non-foreground apps
            null
        }
    }

    /**
     * Write text to the clipboard.
     *
     * @param text Text to place on the clipboard
     * @param label Optional label for the clip data
     * @return true if successful
     */
    fun write(text: String, label: String = "Citros"): Boolean {
        val cm = getClipboardManager() ?: return false
        return try {
            val clip = ClipData.newPlainText(label, text)
            cm.setPrimaryClip(clip)
            true
        } catch (e: SecurityException) {
            // Android 13+ may restrict clipboard writes for non-foreground apps
            false
        }
    }

    /**
     * Write text to clipboard and paste it into the currently focused field.
     * Uses AccessibilityService to perform the paste action.
     *
     * @param text Text to paste
     * @return true if both write and paste succeeded
     */
    fun writeAndPaste(text: String): Boolean {
        if (!write(text)) return false
        return performPaste()
    }

    /**
     * Perform paste action on the currently focused input field
     * via AccessibilityService.
     *
     * @return true if paste was dispatched successfully
     */
    private fun performPaste(): Boolean {
        val svc = ScreenReader.getService() ?: return false
        val root = svc.rootInActiveWindow ?: return false
        return try {
            val focused = findFocusedNode(root)
            if (focused != null) {
                val result = focused.performAction(AccessibilityNodeInfo.ACTION_PASTE)
                focused.recycle()
                result
            } else {
                false
            }
        } catch (e: Exception) {
            false
        } finally {
            root.recycle()
        }
    }

    /**
     * Find the currently focused editable node in the accessibility tree.
     */
    private fun findFocusedNode(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        if (node.isFocused && node.isEditable) {
            return AccessibilityNodeInfo.obtain(node)
        }
        for (i in 0 until node.childCount) {
            val child = node.getChild(i) ?: continue
            val result = findFocusedNode(child)
            child.recycle()
            if (result != null) return result
        }
        return null
    }
}
