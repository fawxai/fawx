package ai.citros.chat

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MarkdownTextTest {

    private val baseColor = Color.White

    @Test
    fun `plain text passes through unchanged`() {
        val result = parseMarkdown("Hello world", baseColor)
        assertEquals("Hello world", result.text)
    }

    @Test
    fun `bold text is rendered with bold span`() {
        val result = parseMarkdown("Hello **bold** world", baseColor)
        assertEquals("Hello bold world", result.text)
        val boldSpan = result.spanStyles.find { it.item.fontWeight == FontWeight.Bold }
        assertTrue(boldSpan != null, "Expected a bold span")
        assertEquals(6, boldSpan.start)
        assertEquals(10, boldSpan.end)
    }

    @Test
    fun `italic text is rendered with italic span`() {
        val result = parseMarkdown("Hello *italic* world", baseColor)
        assertEquals("Hello italic world", result.text)
        val italicSpan = result.spanStyles.find { it.item.fontStyle == FontStyle.Italic }
        assertTrue(italicSpan != null, "Expected an italic span")
    }

    @Test
    fun `inline code is rendered with monospace font`() {
        val result = parseMarkdown("Use `println()` here", baseColor)
        assertEquals("Use println() here", result.text)
        val codeSpan = result.spanStyles.find { it.item.fontFamily == FontFamily.Monospace }
        assertTrue(codeSpan != null, "Expected a monospace span")
    }

    @Test
    fun `heading 1 is bold and large`() {
        val result = parseMarkdown("# My Heading", baseColor)
        assertEquals("My Heading", result.text)
        val boldSpan = result.spanStyles.find { it.item.fontWeight == FontWeight.Bold }
        assertTrue(boldSpan != null, "Expected a bold span for heading")
    }

    @Test
    fun `heading 2 is bold`() {
        val result = parseMarkdown("## Sub Heading", baseColor)
        assertEquals("Sub Heading", result.text)
        val boldSpan = result.spanStyles.find { it.item.fontWeight == FontWeight.Bold }
        assertTrue(boldSpan != null, "Expected a bold span for heading 2")
    }

    @Test
    fun `bullet list items get bullet prefix`() {
        val result = parseMarkdown("- Item one\n- Item two", baseColor)
        assertTrue(result.text.contains("\u2022 Item one"), "Expected bullet for item one")
        assertTrue(result.text.contains("\u2022 Item two"), "Expected bullet for item two")
    }

    @Test
    fun `code blocks render monospace`() {
        val ticks = "\u0060\u0060\u0060"
        val md = "Before\n" + ticks + "\nval x = 1\n" + ticks + "\nAfter"
        val result = parseMarkdown(md, baseColor)
        assertTrue(result.text.contains("val x = 1"), "Expected code block content")
        val codeSpan = result.spanStyles.find { it.item.fontFamily == FontFamily.Monospace }
        assertTrue(codeSpan != null, "Expected a monospace span for code block")
    }

    @Test
    fun `mixed formatting in single line`() {
        val result = parseMarkdown("This is **bold** and *italic* text", baseColor)
        assertEquals("This is bold and italic text", result.text)
        val boldSpan = result.spanStyles.find { it.item.fontWeight == FontWeight.Bold }
        val italicSpan = result.spanStyles.find { it.item.fontStyle == FontStyle.Italic }
        assertTrue(boldSpan != null, "Expected bold")
        assertTrue(italicSpan != null, "Expected italic")
    }

    @Test
    fun `multiline with headings bullets and inline formatting`() {
        val md = "# Title\n\nSome **bold** text.\n\n- First item\n- Second *item*"
        val result = parseMarkdown(md, baseColor)
        assertTrue(result.text.contains("Title"), "Should have title")
        assertTrue(result.text.contains("bold"), "Should have bold text")
        assertTrue(result.text.contains("\u2022 First item"), "Should have bullet")
    }

    @Test
    fun `empty string produces empty result`() {
        val result = parseMarkdown("", baseColor)
        assertEquals("", result.text)
        assertTrue(result.spanStyles.isEmpty(), "No spans expected for empty string")
    }

    @Test
    fun `bold italic combo is rendered with both styles`() {
        val result = parseMarkdown("This is ***bold italic*** text", baseColor)
        assertEquals("This is bold italic text", result.text)
        val combo = result.spanStyles.find {
            it.item.fontWeight == FontWeight.Bold && it.item.fontStyle == FontStyle.Italic
        }
        assertTrue(combo != null, "Expected a bold+italic span")
    }
}
