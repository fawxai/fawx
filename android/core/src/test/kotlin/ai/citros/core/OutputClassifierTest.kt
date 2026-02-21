package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class OutputClassifierTest {

    // ========== Tool categories ==========

    @Test
    fun `categoryOf returns correct category for mapped tools`() {
        assertEquals(ToolCategory.MECHANICAL, OutputClassifier.categoryOf("tap"))
        assertEquals(ToolCategory.PROMINENT, OutputClassifier.categoryOf("open_app"))
        assertEquals(ToolCategory.RESEARCH, OutputClassifier.categoryOf("web_search"))
        assertEquals(ToolCategory.RESEARCH, OutputClassifier.categoryOf("recall"))
        assertEquals(ToolCategory.REASONING, OutputClassifier.categoryOf("think"))
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("learn"))
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("remember"))
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("list_files"))
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("read_file"))
    }

    @Test
    fun `categoryOf returns OTHER for unmapped tools`() {
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("some_future_tool"))
        assertEquals(ToolCategory.OTHER, OutputClassifier.categoryOf("read_file"))
    }

    @Test
    fun `TOOL_CATEGORIES contains all expected tools`() {
        val expectedMechanical = listOf("tap", "tap_text", "long_press", "swipe", "scroll",
            "press_back", "press_home", "type_text", "wait", "read_screen")
        val expectedProminent = listOf("open_app", "open_notifications", "read_notifications",
            "screenshot", "subtask")
        val expectedResearch = listOf("web_search", "web_fetch", "recall")
        val expectedReasoning = listOf("think")
        val expectedOther = listOf(
            "paste", "clipboard", "learn", "remember", "list_files", "read_file",
            "write_file", "copy", "set_clipboard", "list_memories",
            "tap_notification", "dismiss_notification", "reply_notification"
        )

        for (tool in expectedMechanical) {
            assertEquals(ToolCategory.MECHANICAL, OutputClassifier.TOOL_CATEGORIES[tool], "$tool")
        }
        for (tool in expectedProminent) {
            assertEquals(ToolCategory.PROMINENT, OutputClassifier.TOOL_CATEGORIES[tool], "$tool")
        }
        for (tool in expectedResearch) {
            assertEquals(ToolCategory.RESEARCH, OutputClassifier.TOOL_CATEGORIES[tool], "$tool")
        }
        for (tool in expectedReasoning) {
            assertEquals(ToolCategory.REASONING, OutputClassifier.TOOL_CATEGORIES[tool], "$tool")
        }
        for (tool in expectedOther) {
            assertEquals(ToolCategory.OTHER, OutputClassifier.TOOL_CATEGORIES[tool], "$tool")
        }
    }

    // ========== classify() ==========

    @Test
    fun `mechanical actions are hidden`() {
        val mechanical = listOf("tap", "tap_text", "long_press", "swipe", "scroll",
            "press_back", "press_home", "type_text", "wait", "read_screen")
        for (tool in mechanical) {
            assertEquals(
                OutputVisibility.HIDE,
                OutputClassifier.classify(tool, "Success"),
                "$tool should be HIDE"
            )
        }
    }

    @Test
    fun `think is show dimmed`() {
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("think", "I should tap the compose button")
        )
    }

    @Test
    fun `open_app is show`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("open_app", "Opened Gmail")
        )
    }

    @Test
    fun `open_notifications is show`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("open_notifications", "Opened notifications")
        )
    }

    @Test
    fun `screenshot is show (prominent)`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("screenshot", "Screenshot description: inbox with 3 emails")
        )
    }

    @Test
    fun `read_notifications is show (prominent)`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("read_notifications", "4 notifications found")
        )
    }

    @Test
    fun `subtask is show`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("subtask", "Found the email from Sarah")
        )
    }

    @Test
    fun `web_search is show`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("web_search", "Results for weather Denver")
        )
    }

    @Test
    fun `web_fetch is show`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("web_fetch", "URL: https://example.com\n\nPage content...")
        )
    }

    @Test
    fun `file operations are show dimmed`() {
        val fileTools = listOf("read_file", "write_file", "list_files")
        for (tool in fileTools) {
            assertEquals(
                OutputVisibility.SHOW_DIMMED,
                OutputClassifier.classify(tool, "Success"),
                "$tool should be SHOW_DIMMED"
            )
        }
    }

    @Test
    fun `memory tools are show dimmed`() {
        val memoryTools = listOf("remember", "list_memories")
        for (tool in memoryTools) {
            assertEquals(
                OutputVisibility.SHOW_DIMMED,
                OutputClassifier.classify(tool, "Stored memory"),
                "$tool should be SHOW_DIMMED"
            )
        }
    }

    @Test
    fun `clipboard tools are show dimmed`() {
        val clipTools = listOf("copy", "set_clipboard", "paste")
        for (tool in clipTools) {
            assertEquals(
                OutputVisibility.SHOW_DIMMED,
                OutputClassifier.classify(tool, "Clipboard content: hello"),
                "$tool should be SHOW_DIMMED"
            )
        }
    }

    @Test
    fun `wait is hidden (mechanical)`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("wait", "Waited 2s")
        )
    }

    @Test
    fun `read_screen is hidden (mechanical)`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("read_screen", "Screen refreshed")
        )
    }

    @Test
    fun `notification tools are show dimmed`() {
        val notifTools = listOf("tap_notification",
            "dismiss_notification", "reply_notification")
        for (tool in notifTools) {
            assertEquals(
                OutputVisibility.SHOW_DIMMED,
                OutputClassifier.classify(tool, "Success"),
                "$tool should be SHOW_DIMMED"
            )
        }
    }

    @Test
    fun `unknown tool defaults to show dimmed`() {
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("some_future_tool", "Did something")
        )
    }

    @Test
    fun `empty result string is classified based on tool category`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("tap", "")
        )
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("open_app", "")
        )
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("think", "")
        )
    }

    // ========== applyVerbosity() ==========

    @Test
    fun `verbose mode shows everything`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(OutputVisibility.HIDE, OutputVerbosity.VERBOSE)
        )
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW_DIMMED, OutputVerbosity.VERBOSE)
        )
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW, OutputVerbosity.VERBOSE)
        )
    }

    @Test
    fun `minimal mode hides dimmed items`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW_DIMMED, OutputVerbosity.MINIMAL)
        )
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.applyVerbosity(OutputVisibility.HIDE, OutputVerbosity.MINIMAL)
        )
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW, OutputVerbosity.MINIMAL)
        )
    }

    @Test
    fun `normal mode preserves default classification`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.applyVerbosity(OutputVisibility.HIDE, OutputVerbosity.NORMAL)
        )
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW_DIMMED, OutputVerbosity.NORMAL)
        )
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(OutputVisibility.SHOW, OutputVerbosity.NORMAL)
        )
    }

    // ========== formatForDisplay() ==========

    @Test
    fun `hidden items return null`() {
        assertNull(OutputClassifier.formatForDisplay("tap", "Tapped element 5", OutputVisibility.HIDE))
    }

    @Test
    fun `shown items get robot emoji`() {
        assertEquals("\uD83E\uDD16 Opened Gmail",
            OutputClassifier.formatForDisplay("open_app", "Opened Gmail", OutputVisibility.SHOW))
    }

    @Test
    fun `think gets thought bubble emoji`() {
        assertEquals("\uD83D\uDCAD I should find the compose button",
            OutputClassifier.formatForDisplay("think", "I should find the compose button", OutputVisibility.SHOW_DIMMED))
    }

    @Test
    fun `dimmed non-think tools get gear emoji`() {
        assertEquals("\u2699\uFE0F Screen refreshed",
            OutputClassifier.formatForDisplay("read_screen", "Screen refreshed", OutputVisibility.SHOW_DIMMED))
    }

    // ========== isError flag ==========

    @Test
    fun `isError true classifies errors by severity`() {
        // Mechanical tool with unknown error → EXPLORATORY → HIDE
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("tap", "some error", isError = true)
        )
        // File tool with access denied → INFORMATIONAL → SHOW_DIMMED
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("read_file", """{"ok":false,"error":"Access denied"}""", isError = true)
        )
        // Memory tool unknown error → EXPLORATORY (OTHER category) → HIDE
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("remember", "Tool not configured", isError = true)
        )
        // Research tool timeout → TRANSIENT → HIDE
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("web_search", "Search failed: timeout", isError = true)
        )
    }

    @Test
    fun `isError false falls through to normal classification`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("tap", "Tapped element 5", isError = false)
        )
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("read_file", """{"ok":true}""", isError = false)
        )
    }

    // ========== End-to-end scenarios ==========

    // ========== classifyError() ==========

    @Test
    fun `classifyError returns PERSISTENT for accessibility lost`() {
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("tap", "Accessibility service lost connection")
        )
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("tap", "Accessibility disconnected")
        )
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("tap", "Accessibility unavailable")
        )
    }

    @Test
    fun `classifyError returns PERSISTENT for auth failure keywords`() {
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("web_fetch", "HTTP 401 Unauthorized")
        )
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("web_fetch", "403 Forbidden")
        )
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("web_fetch", "Invalid API key provided")
        )
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("web_search", "Unauthorized access")
        )
    }

    @Test
    fun `classifyError returns TRANSIENT for server error first occurrence`() {
        assertEquals(
            ErrorSeverity.TRANSIENT,
            OutputClassifier.classifyError("web_fetch", "HTTP 500 Internal Server Error")
        )
        assertEquals(
            ErrorSeverity.TRANSIENT,
            OutputClassifier.classifyError("web_fetch", "502 Bad Gateway")
        )
        assertEquals(
            ErrorSeverity.TRANSIENT,
            OutputClassifier.classifyError("web_fetch", "503 Service Unavailable")
        )
    }

    @Test
    fun `classifyError returns EXPLORATORY for element not found`() {
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Element not found: button[5]")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Could not tap element 3")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Could not find the compose button")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Failed to tap target")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Failed to click element")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "No matching element")
        )
    }

    @Test
    fun `classifyError returns INFORMATIONAL for app not installed`() {
        assertEquals(
            ErrorSeverity.INFORMATIONAL,
            OutputClassifier.classifyError("open_app", "App not installed: com.example")
        )
    }

    @Test
    fun `classifyError returns INFORMATIONAL for permission denied`() {
        assertEquals(
            ErrorSeverity.INFORMATIONAL,
            OutputClassifier.classifyError("read_file", "Permission denied: /data/secret")
        )
        assertEquals(
            ErrorSeverity.INFORMATIONAL,
            OutputClassifier.classifyError("read_file", "Access denied")
        )
    }

    @Test
    fun `classifyError returns EXPLORATORY for unknown error on mechanical tool`() {
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("tap", "Something went wrong")
        )
        assertEquals(
            ErrorSeverity.EXPLORATORY,
            OutputClassifier.classifyError("swipe", "Unknown failure")
        )
    }

    @Test
    fun `classifyError returns INFORMATIONAL for unknown error on prominent tool`() {
        assertEquals(
            ErrorSeverity.INFORMATIONAL,
            OutputClassifier.classifyError("open_app", "Something went wrong")
        )
        assertEquals(
            ErrorSeverity.INFORMATIONAL,
            OutputClassifier.classifyError("web_search", "Unknown failure")
        )
    }

    @Test
    fun `classifyError escalates EXPLORATORY to TRANSIENT after threshold`() {
        val ctx = RetryContext(consecutiveFailures = 2)
        assertEquals(
            ErrorSeverity.TRANSIENT,
            OutputClassifier.classifyError("tap", "Element not found", ctx)
        )
    }

    @Test
    fun `classifyError escalates TRANSIENT to PERSISTENT after threshold`() {
        val ctx = RetryContext(consecutiveFailures = 3)
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("web_fetch", "HTTP 500 Server Error", ctx)
        )
    }

    @Test
    fun `classifyError does not escalate PERSISTENT further`() {
        val ctx = RetryContext(consecutiveFailures = 10)
        assertEquals(
            ErrorSeverity.PERSISTENT,
            OutputClassifier.classifyError("tap", "Accessibility lost", ctx)
        )
    }

    // ========== classify() with errors ==========

    @Test
    fun `classify hides exploratory errors`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("tap", "Element not found", isError = true)
        )
    }

    @Test
    fun `classify hides transient errors`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify("web_fetch", "500 Internal Server Error", isError = true)
        )
    }

    @Test
    fun `classify shows persistent errors`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify("tap", "Accessibility lost connection", isError = true)
        )
    }

    @Test
    fun `classify dims informational errors`() {
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.classify("open_app", "App not installed", isError = true)
        )
    }

    @Test
    fun `classify uses ToolResult severity when provided`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.classify(
                "tap", "Element not found", isError = true,
                severity = ErrorSeverity.PERSISTENT
            )
        )
    }

    @Test
    fun `classify falls back to classifyError when severity is null`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.classify(
                "tap", "Element not found", isError = true,
                severity = null
            )
        )
    }

    // ========== applyVerbosity + errors ==========

    @Test
    fun `applyVerbosity PERSISTENT always shows regardless of verbosity mode`() {
        for (verbosity in OutputVerbosity.entries) {
            assertEquals(
                OutputVisibility.SHOW,
                OutputClassifier.applyVerbosity(
                    OutputVisibility.SHOW, verbosity, severity = ErrorSeverity.PERSISTENT
                ),
                "PERSISTENT should SHOW in $verbosity mode"
            )
        }
    }

    @Test
    fun `applyVerbosity VERBOSE shows exploratory errors as SHOW_DIMMED`() {
        assertEquals(
            OutputVisibility.SHOW_DIMMED,
            OutputClassifier.applyVerbosity(
                OutputVisibility.HIDE, OutputVerbosity.VERBOSE,
                severity = ErrorSeverity.EXPLORATORY
            )
        )
    }

    @Test
    fun `applyVerbosity VERBOSE shows informational errors as SHOW`() {
        assertEquals(
            OutputVisibility.SHOW,
            OutputClassifier.applyVerbosity(
                OutputVisibility.SHOW_DIMMED, OutputVerbosity.VERBOSE,
                severity = ErrorSeverity.INFORMATIONAL
            )
        )
    }

    @Test
    fun `end-to-end pipeline PERSISTENT error shows in MINIMAL mode`() {
        // Simulate the full classify → applyVerbosity pipeline
        val visibility = OutputClassifier.classify(
            "tap", "Accessibility lost connection", isError = true
        )
        val severity = OutputClassifier.classifyError("tap", "Accessibility lost connection")
        val effective = OutputClassifier.applyVerbosity(visibility, OutputVerbosity.MINIMAL, severity)
        assertEquals(ErrorSeverity.PERSISTENT, severity)
        assertEquals(OutputVisibility.SHOW, effective)
    }

    @Test
    fun `end-to-end pipeline EXPLORATORY error hidden in MINIMAL mode`() {
        val visibility = OutputClassifier.classify(
            "tap", "Element not found", isError = true
        )
        val severity = OutputClassifier.classifyError("tap", "Element not found")
        val effective = OutputClassifier.applyVerbosity(visibility, OutputVerbosity.MINIMAL, severity)
        assertEquals(ErrorSeverity.EXPLORATORY, severity)
        assertEquals(OutputVisibility.HIDE, effective)
    }

    @Test
    fun `RetryContext validates threshold ordering`() {
        try {
            RetryContext(escalateToTransientAt = 5, escalateToPersistentAt = 3)
            throw AssertionError("Expected IllegalArgumentException")
        } catch (e: IllegalArgumentException) {
            // expected
        }
    }

    @Test
    fun `applyVerbosity MINIMAL hides informational errors`() {
        assertEquals(
            OutputVisibility.HIDE,
            OutputClassifier.applyVerbosity(
                OutputVisibility.SHOW_DIMMED, OutputVerbosity.MINIMAL,
                severity = ErrorSeverity.INFORMATIONAL
            )
        )
    }

    // ========== End-to-end scenarios ==========

    @Test
    fun `typical action loop classifies correctly`() {
        assertEquals(OutputVisibility.SHOW, OutputClassifier.classify("open_app", "Opened Gmail"))
        assertEquals(OutputVisibility.SHOW_DIMMED, OutputClassifier.classify("think", "I see the inbox"))
        assertEquals(OutputVisibility.HIDE, OutputClassifier.classify("tap", "Tapped element 3"))
        assertEquals(OutputVisibility.HIDE, OutputClassifier.classify("scroll", "Scrolled down"))
        assertEquals(OutputVisibility.HIDE, OutputClassifier.classify("tap", "tap failed", isError = true))
    }

    @Test
    fun `web research loop classifies correctly`() {
        assertEquals(OutputVisibility.SHOW, OutputClassifier.classify("web_search", "Results for weather"))
        assertEquals(OutputVisibility.SHOW_DIMMED, OutputClassifier.classify("think", "Let me check the first result"))
        assertEquals(OutputVisibility.SHOW, OutputClassifier.classify("web_fetch", "URL: https://weather.com\n\nDenver: 45F"))
    }

    // ========== summarize() tests ==========

    @Test
    fun `summarize strips SCREEN block from result`() {
        val result = "Opened Gmail\n\nSCREEN:\nInbox (42)\nCompose\nSearch\nSettings"
        assertEquals("Opened Gmail", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize strips verification suffixes`() {
        val result = "Tapped 'Send' button\n[Verified: Button was pressed successfully]"
        assertEquals("Tapped 'Send' button", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize strips verification failed suffixes`() {
        val result = "Tapped 'Send' button\n[Verification FAILED: Element not found]"
        assertEquals("Tapped 'Send' button", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize strips verification skipped suffixes`() {
        val result = "Tapped 'Send' button\n[Verification skipped: No screenshot]"
        assertEquals("Tapped 'Send' button", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize takes first line of multiline result`() {
        val result = "Search results for: weather\n\n1. Weather.com\n   https://weather.com\n   Denver forecast: 45F"
        assertEquals("Search results for: weather", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize truncates long single line at word boundary`() {
        val longLine = "A ".repeat(150).trim() // 299 chars "A A A A..."
        val result = OutputClassifier.summarize(longLine)
        assertTrue(result.length <= OutputClassifier.DISPLAY_MAX_CHARS + 1) // truncated at word boundary + ellipsis char
        assertTrue(result.endsWith("…"))
    }

    @Test
    fun `summarize passes through short results unchanged`() {
        assertEquals("Opened Chrome", OutputClassifier.summarize("Opened Chrome"))
    }

    @Test
    fun `summarize handles empty result`() {
        assertEquals("", OutputClassifier.summarize(""))
    }

    @Test
    fun `formatStatus strips screen dumps and keeps concise action`() {
        val status = "Waited 2s. Screen:\nApp: com.android.settings\n[0] Search"
        assertEquals("Waited 2s", OutputClassifier.formatStatus(status))
    }

    @Test
    fun `formatStatus collapses json payloads into generic working label`() {
        val status = "{\"ok\":true,\"tool\":\"web_fetch\",\"result\":\"big payload\"}"
        assertEquals("Working...", OutputClassifier.formatStatus(status))
    }

    @Test
    fun `formatStatus returns generic label for blank status`() {
        assertEquals("Working...", OutputClassifier.formatStatus("   "))
    }

    @Test
    fun `formatStatus collapses array payloads into generic working label`() {
        assertEquals("Working...", OutputClassifier.formatStatus("[1,2,3]"))
    }

    @Test
    fun `formatStatus collapses post summarize json payloads`() {
        val status = "Screenshot description:\n{\"raw\":\"payload\"}"
        assertEquals("Working...", OutputClassifier.formatStatus(status))
    }

    @Test
    fun `formatStatus truncates long statuses for overlay readability`() {
        val longStatus = "Searching for " + "very ".repeat(40) + "specific information"
        val formatted = OutputClassifier.formatStatus(longStatus)
        assertTrue(formatted.length <= OutputClassifier.STATUS_MAX_CHARS + 1)
        assertTrue(formatted.endsWith("…"))
    }

    @Test
    fun `summarize strips screen block with single newline prefix`() {
        val result = "Done\nSCREEN:\nSome content"
        assertEquals("Done", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize with screen block and verification`() {
        val result = "Opened Settings\n[Verified: Settings screen visible]\n\nSCREEN:\nWi-Fi\nBluetooth\nDisplay"
        assertEquals("Opened Settings", OutputClassifier.summarize(result))
    }

    // ========== formatForDisplay with summarization ==========

    @Test
    fun `formatForDisplay summarizes verbose tool result`() {
        val verboseResult = "Opened Gmail\n\nSCREEN:\nInbox (42)\nCompose\nSearch\nArchive\nTrash"
        val display = OutputClassifier.formatForDisplay("open_app", verboseResult, OutputVisibility.SHOW)
        assertEquals("🤖 Opened Gmail", display)
    }

    @Test
    fun `formatForDisplay still returns null for HIDE`() {
        assertNull(OutputClassifier.formatForDisplay("tap", "Tapped element", OutputVisibility.HIDE))
    }

    @Test
    fun `formatForDisplay dimmed reasoning preserves short text`() {
        val display = OutputClassifier.formatForDisplay("think", "Let me check the inbox", OutputVisibility.SHOW_DIMMED)
        assertEquals("💭 Let me check the inbox", display)
    }

    // ========== Edge case and SHOW_DIMMED tests ==========

    @Test
    fun `summarize strips SCREEN block at start of result`() {
        // Issue #4: result starts directly with SCREEN: (no prefix)
        val result = "SCREEN:\nInbox (42)\nCompose\nSearch"
        assertEquals("", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize handles result that is only SCREEN block`() {
        val result = "SCREEN:\nSettings\nWi-Fi\nBluetooth"
        assertEquals("", OutputClassifier.summarize(result))
    }

    @Test
    fun `formatForDisplay dimmed gear tool shows gear emoji`() {
        // Suggestion: test SHOW_DIMMED for a non-reasoning (gear) tool
        val display = OutputClassifier.formatForDisplay(
            "clipboard_write", "Copied to clipboard", OutputVisibility.SHOW_DIMMED
        )
        assertEquals("⚙️ Copied to clipboard", display)
    }

    @Test
    fun `formatForDisplay dimmed memory tool shows gear emoji`() {
        val display = OutputClassifier.formatForDisplay(
            "memory_search", "Found 3 results", OutputVisibility.SHOW_DIMMED
        )
        assertEquals("⚙️ Found 3 results", display)
    }

    @Test
    fun `summarize null fallback truncation appends ellipsis`() {
        // Suggestion #3: verify null fallback (all blank lines) truncates with ellipsis
        val longWhitespace = "   ".repeat(100)  // only whitespace lines
        val result = OutputClassifier.summarize(longWhitespace)
        assertTrue(result.length <= OutputClassifier.DISPLAY_MAX_CHARS + 1)
    }


    // --- Screenshot and wait summarize tests (#625) ---

    @Test
    fun `summarize extracts screenshot description`() {
        val result = "Screenshot description:\nThe screen shows a weather app with 72F"
        assertEquals(
            "The screen shows a weather app with 72F",
            OutputClassifier.summarize(result)
        )
    }

    @Test
    fun `summarize handles empty screenshot description`() {
        assertEquals(
            "Analyzed screen",
            OutputClassifier.summarize("Screenshot description:")
        )
    }

    @Test
    fun `summarize strips wait screen dump`() {
        val result = "Waited 2s. Screen:\nApp: com.android.settings\n[0] Search Settings"
        assertEquals(
            "Waited 2s",
            OutputClassifier.summarize(result)
        )
    }

    @Test
    fun `summarize keeps plain wait`() {
        assertEquals(
            "Waited 2s",
            OutputClassifier.summarize("Waited 2s")
        )
    }

    @Test
    fun `summarize strips wait screen dump with decimal duration`() {
        val result = "Waited 2.5s. Screen:\nApp: com.android.settings\n[0] Search"
        assertEquals(
            "Waited 2.5s",
            OutputClassifier.summarize(result)
        )
    }

    @Test
    fun `summarize strips wait screen dump with ms duration`() {
        val result = "Waited 200ms. Screen:\nApp: com.android.clock\n[0] Alarm"
        assertEquals(
            "Waited 200ms",
            OutputClassifier.summarize(result)
        )
    }


    @Test
    fun `summarize remember json as saved content`() {
        val result = """{"ok":true,"tool":"remember","content":"Favorite restaurant: Sushi Den on Pearl Street"}"""
        assertEquals("Saved: Favorite restaurant: Sushi Den on Pearl Street", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize learn json as saved content`() {
        val result = """{"ok":true,"tool":"learn","content":"Use metric units"}"""
        assertEquals("Saved: Use metric units", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize recall json with results`() {
        val result = """{"ok":true,"tool":"recall","results":[{"content":"Favorite restaurant: Sushi Den on Pearl Street"}]}"""
        assertEquals("Recalled: Favorite restaurant: Sushi Den on Pearl Street", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize recall json with empty results`() {
        val result = """{"ok":true,"tool":"recall","results":[]}"""
        assertEquals("No results found", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize list_files json`() {
        val result = """{"ok":true,"tool":"list_files","files":["file1.txt","file2.txt","file3.txt","file4.txt"]}"""
        assertEquals("Files: file1.txt, file2.txt, file3.txt, ...", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize list_files json empty array as none`() {
        val result = """{"ok":true,"tool":"list_files","files":[]}"""
        assertEquals("Files: none", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize list_files json with three files has no ellipsis`() {
        val result = """{"ok":true,"tool":"list_files","files":["a.txt","b.txt","c.txt"]}"""
        assertEquals("Files: a.txt, b.txt, c.txt", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize unknown json tool falls back to generic string extraction`() {
        val result = """{"ok":true,"tool":"future_tool","note":"Did thing"}"""
        assertEquals("future_tool", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize read_file json`() {
        val result = """{"ok":true,"tool":"read_file","path":"/tmp/note.txt","content":"top secret"}"""
        assertEquals("Read: /tmp/note.txt", OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize malformed json falls through existing logic`() {
        val result = "{" + "\"ok\":true,"
        assertEquals(result, OutputClassifier.summarize(result))
    }

    @Test
    fun `summarize non json behavior unchanged`() {
        val result = "Opened Gmail\n\nSCREEN:\nInbox"
        assertEquals("Opened Gmail", OutputClassifier.summarize(result))
    }

    // --- PR #630: edge cases ---

    @Test
    fun `summarize screenshot description with very long first line truncates at DISPLAY_MAX_CHARS`() {
        val longDescription = "A".repeat(500)
        val result = "Screenshot description:\n$longDescription"
        val summarized = OutputClassifier.summarize(result)
        assertTrue(
            summarized.length <= OutputClassifier.DISPLAY_MAX_CHARS + 1,
            "Expected truncated to at most ${OutputClassifier.DISPLAY_MAX_CHARS + 1} chars but got ${summarized.length}"
        )
        assertTrue(summarized.endsWith("…"), "Expected ellipsis at end")
    }

    @Test
    fun `TOOL_CATEGORIES covers all known tool names from PhoneTools`() {
        // Every tool in PhoneTools.ALL should have an EXPLICIT entry in TOOL_CATEGORIES.
        // categoryOf() falls back to OTHER for unmapped tools, so assertNotNull is useless —
        // we must check the map directly to catch tools that were added without categorization.
        val allToolNames = PhoneTools.ALL.map { it.name }
        val categorized = OutputClassifier.TOOL_CATEGORIES.keys

        for (toolName in allToolNames) {
            assertTrue(
                categorized.contains(toolName),
                "Tool '$toolName' from PhoneTools.ALL is not explicitly mapped in TOOL_CATEGORIES — " +
                    "it will silently fall back to OTHER. Add an entry."
            )
        }
    }
}
