package ai.citros.chat

import android.widget.Toast
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.LinkInteractionListener
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.sp

/**
 * Renders markdown-formatted text as styled Compose [Text].
 *
 * Supports:
 * - **bold** (`**text**` or `__text__`)
 * - *italic* (`*text*` or `_text_`)
 * - `inline code` (backtick-wrapped)
 * - # Headings (H1–H3, line-level)
 * - Bullet lists (`- item` or `* item` at start of line)
 * - Code blocks (triple-backtick fenced, rendered monospace)
 * - Markdown links (`[label](https://example.com)`)
 */
@Composable
internal fun MarkdownText(
    text: String,
    color: Color,
    style: TextStyle,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val uriHandler = LocalUriHandler.current
    val openUrl: (String) -> Unit = remember(context, uriHandler) {
        { url ->
            try {
                uriHandler.openUri(url)
            } catch (_: Exception) {
                Toast.makeText(context, LINK_OPEN_FAILED_MESSAGE, Toast.LENGTH_SHORT).show()
            }
        }
    }

    val annotated = remember(text, color, openUrl) {
        withLinkAnnotations(
            markdown = parseMarkdown(text, color),
            linkColor = LINK_COLOR,
            onOpenUrl = openUrl
        )
    }

    Text(
        text = annotated,
        style = style.copy(color = color),
        modifier = modifier
    )
}

private val BOLD_ITALIC_REGEX = Regex("""\*\*\*(.+?)\*\*\*|___(.+?)___""")
private val BOLD_REGEX = Regex("""\*\*(.+?)\*\*|__(.+?)__""")
private val ITALIC_REGEX = Regex("""(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)|(?<![\w_])_(?!_)(.+?)(?<!_)_(?![\w_])""")
private val CODE_REGEX = Regex("""`([^`]+)`""")
private val HEADING_REGEX = Regex("""^(#{1,3})\s+(.+)$""")
private val BULLET_REGEX = Regex("""^\s*[-*]\s+(.+)$""")
private val CODE_BLOCK_REGEX = Regex("""```(?:\w*\n)?([\s\S]*?)```""", RegexOption.MULTILINE)
private val URL_CANDIDATE_REGEX = Regex("""https?://[^\s<\[\]{}"']+""", RegexOption.IGNORE_CASE)
private const val URL_ANNOTATION_TAG = "url"
private val URL_TRAILING_TRIM_CHARS = charArrayOf('.', ',', ';', ':', '!', '?', ']', '}')
private val LINK_COLOR = Color(0xFF5A8DFF)
private const val LINK_OPEN_FAILED_MESSAGE = "Couldn't open link"

private data class TextRange(val start: Int, val end: Int)
private data class UrlMatch(val url: String, val start: Int, val end: Int)
private data class MarkdownInlineLink(
    val fullMatchStart: Int,
    val fullMatchEnd: Int,
    val label: String,
    val url: String
)

internal fun withLinkAnnotations(
    markdown: AnnotatedString,
    linkColor: Color,
    onOpenUrl: ((String) -> Unit)? = null
): AnnotatedString {
    if (markdown.text.isEmpty()) return markdown

    val builder = AnnotatedString.Builder(markdown)
    val monospaceSpans = markdown.spanStyles.filter { it.item.fontFamily == FontFamily.Monospace }
    val linkedRanges = mutableListOf<TextRange>()

    markdown
        .getStringAnnotations(tag = URL_ANNOTATION_TAG, start = 0, end = markdown.text.length)
        .forEach { annotation ->
            val range = TextRange(annotation.start, annotation.end)
            if (range.start >= range.end) return@forEach
            if (isInsideMonospace(range, monospaceSpans)) return@forEach

            addLink(
                builder = builder,
                range = range,
                url = annotation.item,
                linkColor = linkColor,
                onOpenUrl = onOpenUrl
            )
            linkedRanges += range
        }

    findPlainUrls(markdown.text).forEach { match ->
        val range = TextRange(start = match.start, end = match.end)
        if (range.start >= range.end) return@forEach
        if (isInsideMonospace(range, monospaceSpans)) return@forEach
        if (linkedRanges.any { rangesOverlap(it, range) }) return@forEach

        addLink(
            builder = builder,
            range = range,
            url = match.url,
            linkColor = linkColor,
            onOpenUrl = onOpenUrl
        )
        linkedRanges += range
    }

    return builder.toAnnotatedString()
}

private fun addLink(
    builder: AnnotatedString.Builder,
    range: TextRange,
    url: String,
    linkColor: Color,
    onOpenUrl: ((String) -> Unit)?
) {
    val linkStyle = SpanStyle(
        color = linkColor,
        textDecoration = TextDecoration.Underline
    )
    val interactionListener = onOpenUrl?.let { openUrl ->
        LinkInteractionListener { openUrl(url) }
    }
    builder.addLink(
        LinkAnnotation.Url(
            url = url,
            styles = TextLinkStyles(style = linkStyle),
            linkInteractionListener = interactionListener
        ),
        range.start,
        range.end
    )
    builder.addStyle(linkStyle, range.start, range.end)
}

private fun isInsideMonospace(
    range: TextRange,
    monospaceSpans: List<AnnotatedString.Range<SpanStyle>>
): Boolean {
    return monospaceSpans.any { span ->
        range.start >= span.start && range.end <= span.end
    }
}

private fun rangesOverlap(first: TextRange, second: TextRange): Boolean {
    return first.start < second.end && second.start < first.end
}

private fun findPlainUrls(text: String): List<UrlMatch> {
    if (text.isEmpty()) return emptyList()

    return URL_CANDIDATE_REGEX.findAll(text).mapNotNull { match ->
        val normalizedUrl = normalizeDetectedUrl(match.value)
        if (!isSupportedHttpUrl(normalizedUrl)) return@mapNotNull null

        val start = match.range.first
        val end = start + normalizedUrl.length
        if (end <= start) return@mapNotNull null

        UrlMatch(url = normalizedUrl, start = start, end = end)
    }.toList()
}

private fun normalizeDetectedUrl(candidate: String): String {
    var end = candidate.length

    while (end > 0) {
        val trailing = candidate[end - 1]
        when {
            trailing in URL_TRAILING_TRIM_CHARS -> end--
            trailing == ')' && hasExtraClosingParen(candidate, end) -> end--
            else -> break
        }
    }

    return candidate.substring(0, end)
}

private fun hasExtraClosingParen(candidate: String, endExclusive: Int): Boolean {
    var opening = 0
    var closing = 0

    for (index in 0 until endExclusive) {
        when (candidate[index]) {
            '(' -> opening++
            ')' -> closing++
        }
    }

    return closing > opening
}

private fun isSupportedHttpUrl(url: String): Boolean {
    if (url.isBlank() || url.any(Char::isWhitespace)) return false
    return url.startsWith("http://", ignoreCase = true) || url.startsWith("https://", ignoreCase = true)
}

internal fun parseMarkdown(text: String, baseColor: Color = Color.Unspecified): AnnotatedString {
    return buildAnnotatedString {
        // First, extract code blocks and process them separately
        val blockMatches = CODE_BLOCK_REGEX.findAll(text).toList()

        var cursor = 0
        for (match in blockMatches) {
            // Process text before code block
            if (match.range.first > cursor) {
                appendInlineMarkdown(text.substring(cursor, match.range.first), baseColor)
            }
            // Render code block as monospace
            val codeContent = match.groupValues[1].trimEnd()
            withStyle(
                SpanStyle(
                    fontFamily = FontFamily.Monospace,
                    background = baseColor.copy(alpha = 0.08f)
                )
            ) {
                append(codeContent)
            }
            cursor = match.range.last + 1
        }

        // Process remaining text after last code block
        if (cursor < text.length) {
            appendInlineMarkdown(text.substring(cursor), baseColor)
        }
    }
}

private fun AnnotatedString.Builder.appendInlineMarkdown(text: String, baseColor: Color) {
    val lines = text.split("\n")
    for ((lineIndex, line) in lines.withIndex()) {
        if (lineIndex > 0) append("\n")

        // Check for heading
        val headingMatch = HEADING_REGEX.matchEntire(line)
        if (headingMatch != null) {
            val level = headingMatch.groupValues[1].length
            val headingText = headingMatch.groupValues[2]
            val headingStyle = when (level) {
                1 -> SpanStyle(fontWeight = FontWeight.Bold, fontSize = 20.sp)
                2 -> SpanStyle(fontWeight = FontWeight.Bold, fontSize = 17.sp)
                else -> SpanStyle(fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            }
            withStyle(headingStyle) {
                appendFormattedInline(headingText, baseColor)
            }
            continue
        }

        // Check for bullet
        val bulletMatch = BULLET_REGEX.matchEntire(line)
        if (bulletMatch != null) {
            append("  • ")
            appendFormattedInline(bulletMatch.groupValues[1], baseColor)
            continue
        }

        // Regular line — apply inline formatting
        appendFormattedInline(line, baseColor)
    }
}

/**
 * Apply inline markdown formatting: links, bold, italic, and inline code.
 * Processing order: code first (to preserve code semantics), then links, then bold/italic.
 */
private fun AnnotatedString.Builder.appendFormattedInline(text: String, baseColor: Color) {
    // Tokenize into segments: code or plain text
    data class Segment(val text: String, val style: SpanStyle?)

    val segments = mutableListOf<Segment>()
    var remaining = text

    // Extract inline code first (prevents links/bold/italic inside code)
    while (remaining.isNotEmpty()) {
        val codeMatch = CODE_REGEX.find(remaining)
        if (codeMatch == null) {
            segments.add(Segment(remaining, null))
            break
        }
        if (codeMatch.range.first > 0) {
            segments.add(Segment(remaining.substring(0, codeMatch.range.first), null))
        }
        segments.add(
            Segment(
                codeMatch.groupValues[1],
                SpanStyle(
                    fontFamily = FontFamily.Monospace,
                    background = baseColor.copy(alpha = 0.08f)
                )
            )
        )
        remaining = remaining.substring(codeMatch.range.last + 1)
    }

    // Process markdown links / bold / italic only in non-code segments
    for (segment in segments) {
        if (segment.style != null) {
            // Code segment — render as-is
            withStyle(segment.style) { append(segment.text) }
        } else {
            appendTextWithMarkdownLinks(segment.text)
        }
    }
}

private fun AnnotatedString.Builder.appendTextWithMarkdownLinks(text: String) {
    val markdownLinks = findMarkdownInlineLinks(text)
    if (markdownLinks.isEmpty()) {
        appendBoldItalic(text)
        return
    }

    var cursor = 0
    for (link in markdownLinks) {
        if (link.fullMatchStart > cursor) {
            appendBoldItalic(text.substring(cursor, link.fullMatchStart))
        }

        val linkStart = length
        appendBoldItalic(link.label)
        val linkEnd = length

        if (linkEnd > linkStart) {
            addStringAnnotation(
                tag = URL_ANNOTATION_TAG,
                annotation = link.url,
                start = linkStart,
                end = linkEnd
            )
        }

        cursor = link.fullMatchEnd
    }

    if (cursor < text.length) {
        appendBoldItalic(text.substring(cursor))
    }
}

private fun findMarkdownInlineLinks(text: String): List<MarkdownInlineLink> {
    val links = mutableListOf<MarkdownInlineLink>()
    var index = 0

    while (index < text.length) {
        val labelStart = text.indexOf('[', startIndex = index)
        if (labelStart == -1) break

        val labelEnd = text.indexOf(']', startIndex = labelStart + 1)
        if (labelEnd == -1 || labelEnd + 1 >= text.length || text[labelEnd + 1] != '(') {
            index = labelStart + 1
            continue
        }

        val label = text.substring(labelStart + 1, labelEnd)
        if (label.isEmpty()) {
            index = labelEnd + 1
            continue
        }

        val urlStart = labelEnd + 2
        var cursor = urlStart
        var nestedParens = 0
        var urlEnd = -1

        while (cursor < text.length) {
            when (text[cursor]) {
                '(' -> nestedParens++
                ')' -> {
                    if (nestedParens == 0) {
                        urlEnd = cursor
                        break
                    }
                    nestedParens--
                }
            }
            cursor++
        }

        if (urlEnd == -1) {
            index = labelStart + 1
            continue
        }

        val rawUrl = text.substring(urlStart, urlEnd).trim()
        val normalizedUrl = normalizeDetectedUrl(rawUrl)
        if (isSupportedHttpUrl(normalizedUrl)) {
            links += MarkdownInlineLink(
                fullMatchStart = labelStart,
                fullMatchEnd = urlEnd + 1,
                label = label,
                url = normalizedUrl
            )
        }

        index = urlEnd + 1
    }

    return links
}

private fun AnnotatedString.Builder.appendBoldItalic(text: String) {
    var remaining = text
    while (remaining.isNotEmpty()) {
        // Find the earliest bold+italic, bold, or italic match
        val boldItalicMatch = BOLD_ITALIC_REGEX.find(remaining)
        val boldMatch = BOLD_REGEX.find(remaining)
        val italicMatch = ITALIC_REGEX.find(remaining)

        val nextMatch = listOfNotNull(boldItalicMatch, boldMatch, italicMatch)
            .minByOrNull { it.range.first }

        if (nextMatch == null) {
            append(remaining)
            break
        }

        // Append text before match
        if (nextMatch.range.first > 0) {
            append(remaining.substring(0, nextMatch.range.first))
        }

        val innerText = nextMatch.groupValues.drop(1).firstOrNull { it.isNotEmpty() } ?: ""

        when (nextMatch) {
            boldItalicMatch -> withStyle(
                SpanStyle(
                    fontWeight = FontWeight.Bold,
                    fontStyle = FontStyle.Italic
                )
            ) { append(innerText) }
            boldMatch -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(innerText) }
            else -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append(innerText) }
        }

        remaining = remaining.substring(nextMatch.range.last + 1)
    }
}
