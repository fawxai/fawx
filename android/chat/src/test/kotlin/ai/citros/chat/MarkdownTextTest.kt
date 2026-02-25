package ai.citros.chat

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class MarkdownTextTest {

    private val baseColor = Color.White

    private data class UrlRange(val url: String, val start: Int, val end: Int)

    private fun AnnotatedString.urlRanges(): List<UrlRange> {
        return getLinkAnnotations(0, text.length).mapNotNull { range ->
            val url = (range.item as? LinkAnnotation.Url)?.url ?: return@mapNotNull null
            UrlRange(url = url, start = range.start, end = range.end)
        }
    }

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
    fun `underscore italic text is rendered with italic span`() {
        val result = parseMarkdown("Hello _italic_ world", baseColor)
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

    @Test
    fun `withLinkAnnotations adds clickable https annotation`() {
        val markdown = parseMarkdown("Book here: https://example.com/flights", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("https://example.com/flights", urls.single().url)
    }

    @Test
    fun `withLinkAnnotations supports http urls`() {
        val markdown = parseMarkdown("Legacy endpoint: http://example.com/v1/status", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("http://example.com/v1/status", urls.single().url)
    }

    @Test
    fun `withLinkAnnotations trims trailing punctuation from url`() {
        val markdown = parseMarkdown("Try https://example.com/flights, it works.", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("https://example.com/flights", urls.single().url)
    }

    @Test
    fun `withLinkAnnotations handles balanced parentheses in url`() {
        val markdown = parseMarkdown(
            "Read https://en.wikipedia.org/wiki/Kotlin_(programming_language) for details",
            baseColor
        )

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("https://en.wikipedia.org/wiki/Kotlin_(programming_language)", urls.single().url)
    }

    @Test
    fun `withLinkAnnotations trims unmatched closing parenthesis`() {
        val markdown = parseMarkdown("(see https://example.com/path(foo))", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("https://example.com/path(foo)", urls.single().url)
    }

    @Test
    fun `withLinkAnnotations applies distinct link style`() {
        val source = "Visit https://example.com"
        val markdown = parseMarkdown(source, baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val url = linked.urlRanges().single()

        val styleRange = linked.spanStyles.find {
            it.start == url.start &&
                it.end == url.end &&
                it.item.color == Color.Cyan &&
                it.item.textDecoration == TextDecoration.Underline
        }

        assertTrue(styleRange != null, "Expected underline + color style on link range")
    }

    @Test
    fun `withLinkAnnotations preserves markdown spans`() {
        val markdown = parseMarkdown("**Deal** at https://example.com", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val boldSpan = linked.spanStyles.find { it.item.fontWeight == FontWeight.Bold }

        assertTrue(boldSpan != null, "Expected bold span to remain after link annotation")
        assertTrue(linked.urlRanges().isNotEmpty(), "Expected url link annotation")
    }

    @Test
    fun `withLinkAnnotations supports multiple urls with correct offsets`() {
        val source = "One https://one.example/path and two https://two.example/path"
        val markdown = parseMarkdown(source, baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges().sortedBy { it.start }

        assertEquals(2, urls.size)

        val firstExpected = "https://one.example/path"
        val secondExpected = "https://two.example/path"

        assertEquals(firstExpected, urls[0].url)
        assertEquals(source.indexOf(firstExpected), urls[0].start)
        assertEquals(urls[0].start + firstExpected.length, urls[0].end)

        assertEquals(secondExpected, urls[1].url)
        assertEquals(source.indexOf(secondExpected), urls[1].start)
        assertEquals(urls[1].start + secondExpected.length, urls[1].end)
    }

    @Test
    fun `withLinkAnnotations supports urls at start and end positions`() {
        val source = "https://start.example middle text https://end.example"
        val markdown = parseMarkdown(source, baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges().sortedBy { it.start }

        assertEquals(2, urls.size)
        assertEquals(0, urls[0].start)
        assertEquals(source.length, urls[1].end)
    }

    @Test
    fun `withLinkAnnotations does not linkify inline code urls`() {
        val markdown = parseMarkdown("Endpoint: `https://api.example.com/v2/users`", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)

        assertTrue(linked.urlRanges().isEmpty(), "URL inside inline code should not be linkified")
    }

    @Test
    fun `withLinkAnnotations does not linkify fenced code block urls`() {
        val ticks = "\u0060\u0060\u0060"
        val markdown = parseMarkdown("Before\n$ticks\ncurl https://api.example.com/v2/users\n$ticks\nAfter", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)

        assertTrue(linked.urlRanges().isEmpty(), "URL inside code block should not be linkified")
    }

    @Test
    fun `parseMarkdown supports markdown link syntax`() {
        val markdown = parseMarkdown("Visit [Jetpack Compose](https://developer.android.com/jetpack/compose) now", baseColor)

        assertEquals("Visit Jetpack Compose now", markdown.text)

        val linked = withLinkAnnotations(markdown, Color.Cyan)
        val urls = linked.urlRanges()

        assertEquals(1, urls.size)
        assertEquals("https://developer.android.com/jetpack/compose", urls.single().url)

        val linkedText = linked.text.substring(urls.single().start, urls.single().end)
        assertEquals("Jetpack Compose", linkedText)
    }

    @Test
    fun `parseMarkdown does not turn markdown links inside inline code into links`() {
        val markdown = parseMarkdown("Keep this as code: `[label](https://example.com)`", baseColor)

        val linked = withLinkAnnotations(markdown, Color.Cyan)

        assertTrue(linked.urlRanges().isEmpty())
    }
}
