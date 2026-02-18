package ai.citros.core

/**
 * Phone control tool definitions for structured function calling.
 * 
 * These tools replace regex-based action parsing with native tool use APIs.
 * Each tool is defined with JSON Schema for input validation.
 * 
 * ## Task Completion Convention
 * 
 * There is no explicit "task_complete" tool. When the agent returns a text-only
 * response (stopReason: "end_turn"), the task is considered complete. This is
 * the natural signal that the agent has finished its work and is ready to
 * communicate results to the user.
 * 
 * ## Scroll vs Swipe
 * 
 * - **scroll**: Scrolls content within a scrollable container (up/down only).
 *   Maps to Android's `AccessibilityNodeInfo.ACTION_SCROLL_FORWARD/BACKWARD`.
 *   Most Android ScrollView widgets only support vertical scrolling.
 * 
 * - **swipe**: Performs a gesture swipe in any direction (up/down/left/right).
 *   Can be used for navigation gestures, dismissing items, or switching pages.
 */
object PhoneTools {
    
    /**
     * Tap a UI element by its numeric ID.
     * Example: tap(element_id=5) to click element [5] from screen content.
     */
    val TAP = Tool(
        name = "tap",
        description = "Tap a UI element by its numeric ID from the screen content",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "element_id" to mapOf(
                    "type" to "integer",
                    "description" to "The numeric ID of the element to tap (e.g., 5 for element [5])"
                )
            ),
            "required" to listOf("element_id")
        )
    )
    
    /**
     * Tap a UI element containing specific text.
     * Example: tap_text(text="Search") to click a button labeled "Search".
     */
    val TAP_TEXT = Tool(
        name = "tap_text",
        description = "Tap a UI element that contains the specified text",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "text" to mapOf(
                    "type" to "string",
                    "description" to "The text to search for in UI elements"
                )
            ),
            "required" to listOf("text")
        )
    )
    
    /**
     * Type text into the currently focused input field.
     * IMPORTANT: This only types text - it does NOT submit or send.
     * After typing, you must separately tap the send/submit button.
     */
    val TYPE_TEXT = Tool(
        name = "type_text",
        description = "Type text into the currently focused input field. Does NOT submit - you must tap the send/submit button separately after typing.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "text" to mapOf(
                    "type" to "string",
                    "description" to "The text to type into the focused field"
                )
            ),
            "required" to listOf("text")
        )
    )
    
    /**
     * Perform a swipe gesture in the specified direction.
     */
    val SWIPE = Tool(
        name = "swipe",
        description = "Perform a swipe gesture in the specified direction",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "direction" to mapOf(
                    "type" to "string",
                    "enum" to listOf("up", "down", "left", "right"),
                    "description" to "Direction to swipe"
                )
            ),
            "required" to listOf("direction")
        )
    )
    
    /**
     * Press the back button.
     */
    val PRESS_BACK = Tool(
        name = "press_back",
        description = "Press the back button",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf<String, Any>(),
            "required" to listOf<String>()
        )
    )
    
    /**
     * Press the home button.
     */
    val PRESS_HOME = Tool(
        name = "press_home",
        description = "Press the home button to go to the home screen",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf<String, Any>(),
            "required" to listOf<String>()
        )
    )
    
    /**
     * Launch an app by name.
     * Example: open_app(app_name="YouTube")
     */
    val OPEN_APP = Tool(
        name = "open_app",
        description = "Launch an app by its name",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "app_name" to mapOf(
                    "type" to "string",
                    "description" to "The name of the app to launch (e.g., 'YouTube', 'Gmail')"
                )
            ),
            "required" to listOf("app_name")
        )
    )
    
    /**
     * Open the notification shade/drawer.
     */
    val OPEN_NOTIFICATIONS = Tool(
        name = "open_notifications",
        description = "Open the notification shade/drawer",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf<String, Any>(),
            "required" to listOf<String>()
        )
    )
    
    /**
     * Re-read the current screen content.
     * Use this to refresh the screen state after an action if the updated
     * screen content is not provided automatically.
     */
    val READ_SCREEN = Tool(
        name = "read_screen",
        description = "Re-read the current screen content to get updated UI state",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf<String, Any>(),
            "required" to listOf<String>()
        )
    )
    
    /**
     * Scroll in the specified direction.
     */
    val SCROLL = Tool(
        name = "scroll",
        description = "Scroll in the specified direction",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "direction" to mapOf(
                    "type" to "string",
                    "enum" to listOf("up", "down"),
                    "description" to "Direction to scroll"
                )
            ),
            "required" to listOf("direction")
        )
    )
    
    val READ_FILE = Tool(
        name = "read_file",
        description = "Read a UTF-8 text file from the agent directory (e.g. SOUL.md, memory/2026-02-12.md)",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "path" to mapOf(
                    "type" to "string",
                    "description" to "Relative path inside agent/ (path traversal outside agent directory is blocked)"
                )
            ),
            "required" to listOf("path")
        )
    )

    val WRITE_FILE = Tool(
        name = "write_file",
        description = "Write UTF-8 text to a file in the agent directory. SECURITY.md is read-only and cannot be changed.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "path" to mapOf(
                    "type" to "string",
                    "description" to "Relative path inside agent/"
                ),
                "content" to mapOf(
                    "type" to "string",
                    "description" to "Full file content to write"
                )
            ),
            "required" to listOf("path", "content")
        )
    )

    val LIST_FILES = Tool(
        name = "list_files",
        description = "List files and directories within the agent directory. Optional path parameter is relative to agent/",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "path" to mapOf(
                    "type" to "string",
                    "description" to "Optional relative directory path inside agent/. Defaults to root when omitted."
                )
            ),
            "required" to listOf<String>()
        )
    )

    /**
     * Think/plan without taking an action. Use for complex tasks.
     * Output is returned as tool result but not shown prominently to the user.
     */
    val THINK = Tool(
        name = "think",
        description = "Think about the current situation and plan next steps. Use for complex multi-step tasks to reason before acting. Not shown to user.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "thought" to mapOf(
                    "type" to "string",
                    "description" to "Your reasoning about the current state and what to do next"
                )
            ),
            "required" to listOf("thought")
        )
    )

    /**
     * Wait for the screen to update before reading again.
     * Useful after launching apps or triggering loading states.
     */
    val WAIT = Tool(
        name = "wait",
        description = "Wait for the screen to update (e.g., after launching an app or loading content), then read the screen. Use when you expect the UI to change after an action.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "seconds" to mapOf(
                    "type" to "integer",
                    "description" to "Seconds to wait (1-5, clamped to range)"
                )
            ),
            "required" to listOf("seconds")
        )
    )

    /**
     * Long-press a UI element (for context menus, copy/paste, drag, etc.).
     */
    val LONG_PRESS = Tool(
        name = "long_press",
        description = "Long-press a UI element by its numeric ID (for context menus, copy/paste, etc.)",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "element_id" to mapOf(
                    "type" to "integer",
                    "description" to "The numeric ID of the element to long-press"
                )
            ),
            "required" to listOf("element_id")
        )
    )

    /**
     * Take a screenshot and describe what's on screen using vision.
     */
    val SCREENSHOT = Tool(
        name = "screenshot",
        description = "Take a screenshot and describe the screen using vision AI. Returns a detailed text description. More accurate than read_screen for visual content.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "prompt" to mapOf(
                    "type" to "string",
                    "description" to "Optional prompt to guide the vision model (e.g., 'What color is the button?' or 'Read the text in the dialog'). Defaults to a general screen description."
                )
            ),
            "required" to listOf<String>()
        )
    )

    /**
     * Read the current clipboard text content.
     * Note: On Android 13+, clipboard reading may be restricted.
     */
    val COPY = Tool(
        name = "copy",
        description = "Read text FROM the clipboard (does not copy anything). Returns the current clipboard content, or an error if empty or restricted on Android 13+.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf<String, Any>(),
            "required" to listOf<String>()
        )
    )

    /**
     * Write text to the clipboard without pasting.
     */
    val SET_CLIPBOARD = Tool(
        name = "set_clipboard",
        description = "Write text to the clipboard. Does NOT paste — use the 'paste' tool to also paste into the focused field.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "text" to mapOf(
                    "type" to "string",
                    "description" to "Text to place on the clipboard"
                )
            ),
            "required" to listOf("text")
        )
    )

    /**
     * Write text to clipboard and paste it into the currently focused input field.
     */
    val PASTE = Tool(
        name = "paste",
        description = "Write text to the clipboard and paste it into the currently focused input field. Combines set_clipboard + paste action.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "text" to mapOf(
                    "type" to "string",
                    "description" to "Text to paste into the focused field"
                )
            ),
            "required" to listOf("text")
        )
    )

    /**
     * Read active notifications on the device.
     */
    val READ_NOTIFICATIONS = Tool(
        name = "read_notifications",
        description = "Read active (non-dismissed) notifications. Returns key, app name, title, text, and available actions for each notification. Use the key with tap_notification, dismiss_notification, or reply_notification.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "include_ongoing" to mapOf(
                    "type" to "boolean",
                    "description" to "Include ongoing notifications like music players and foreground services (default: false)"
                )
            ),
            "required" to listOf<String>()
        )
    )

    /**
     * Tap (open) a notification by its stable key.
     */
    val TAP_NOTIFICATION = Tool(
        name = "tap_notification",
        description = "Open a notification by its key (from read_notifications). This sends the notification's content intent, typically opening the originating app.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "notification_key" to mapOf(
                    "type" to "string",
                    "description" to "The notification key from read_notifications output"
                )
            ),
            "required" to listOf("notification_key")
        )
    )

    /**
     * Dismiss a notification by its stable key.
     */
    val DISMISS_NOTIFICATION = Tool(
        name = "dismiss_notification",
        description = "Dismiss (remove) a notification by its key from read_notifications. Cannot dismiss ongoing notifications.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "notification_key" to mapOf(
                    "type" to "string",
                    "description" to "The notification key from read_notifications output"
                )
            ),
            "required" to listOf("notification_key")
        )
    )

    /**
     * Reply to a notification that supports inline reply.
     */
    val REPLY_NOTIFICATION = Tool(
        name = "reply_notification",
        description = "Reply to a notification using its inline reply action (e.g., reply to a message notification). Only works on notifications that have a [reply] action.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "notification_key" to mapOf(
                    "type" to "string",
                    "description" to "The notification key from read_notifications output"
                ),
                "text" to mapOf(
                    "type" to "string",
                    "description" to "Reply text to send"
                )
            ),
            "required" to listOf("notification_key", "text")
        )
    )
    val REMEMBER = Tool(
        name = "remember",
        description = "Store a memory for later recall",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "content" to mapOf(
                    "type" to "string",
                    "description" to "Memory content to store"
                ),
                "tags" to mapOf(
                    "type" to "string",
                    "description" to "Optional comma-separated tags (e.g., 'work,idea')"
                )
            ),
            "required" to listOf("content")
        )
    )

    val RECALL = Tool(
        name = "recall",
        description = "Search stored memories by keyword",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "query" to mapOf(
                    "type" to "string",
                    "description" to "Search query"
                ),
                "limit" to mapOf(
                    "type" to "integer",
                    "description" to "Max results to return (default 5)"
                )
            ),
            "required" to listOf("query")
        )
    )

    val LIST_MEMORIES = Tool(
        name = "list_memories",
        description = "List recent stored memories",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "limit" to mapOf(
                    "type" to "integer",
                    "description" to "Max results to return (default 10)"
                )
            ),
            "required" to listOf<String>()
        )
    )

    /**
     * All available phone control tools.
     * Use this list when calling chatWithTools().
     */
    /**
     * Search the web using a search engine.
     * Returns titles, URLs, and descriptions for the query.
     */
    val WEB_SEARCH = Tool(
        name = "web_search",
        description = "Search the web. Returns titles, URLs, and snippets. Use for current events, facts, or research the user asks about.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "query" to mapOf(
                    "type" to "string",
                    "description" to "Search query string"
                ),
                "count" to mapOf(
                    "type" to "integer",
                    "description" to "Number of results (1-5, default 3)"
                )
            ),
            "required" to listOf("query")
        )
    )

    /**
     * Fetch and read a web page.
     * Downloads the page and extracts readable text content.
     */
    val WEB_FETCH = Tool(
        name = "web_fetch",
        description = "Fetch and read a web page URL. Returns extracted text content. Use after web_search to read a specific result, or to read any URL.",
        inputSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "url" to mapOf(
                    "type" to "string",
                    "description" to "URL to fetch (http or https)"
                ),
                "max_chars" to mapOf(
                    "type" to "integer",
                    "description" to "Maximum characters to return (default 5000)"
                )
            ),
            "required" to listOf("url")
        )
    )

    /**
     * API tools that require network access.
     * Only available to models at STANDARD tier or above.
     * Conditionally included via PhoneAgentApi.getToolsForModel().
     */
    val API_TOOLS: List<Tool> = listOf(
        WEB_SEARCH,
        WEB_FETCH
    )

    val ALL: List<Tool> = listOf(
        TAP,
        TAP_TEXT,
        TYPE_TEXT,
        SWIPE,
        PRESS_BACK,
        PRESS_HOME,
        OPEN_APP,
        OPEN_NOTIFICATIONS,
        READ_SCREEN,
        SCROLL,
        SCREENSHOT,
        COPY,
        SET_CLIPBOARD,
        PASTE,
        READ_NOTIFICATIONS,
        TAP_NOTIFICATION,
        DISMISS_NOTIFICATION,
        REPLY_NOTIFICATION,
        READ_FILE,
        WRITE_FILE,
        LIST_FILES,
        REMEMBER,
        RECALL,
        LIST_MEMORIES,
        THINK,
        WAIT,
        LONG_PRESS
    )
}
