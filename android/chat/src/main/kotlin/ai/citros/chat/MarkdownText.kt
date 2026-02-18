package ai.citros.chat

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
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
 */
@Composable
internal fun MarkdownText(
    text: String,
    color: Color,
    style: TextStyle,
    modifier: Modifier = Modifier
) {
    val annotated = remember(text) { parseMarkdown(text, color) }
    Text(
        text = annotated,
        color = color,
        style = style,
        modifier = modifier
    )
}

private val BOLD_ITALIC_REGEX = Regex("""\*\*\*(.+?)\*\*\*|___(.+?)___""")
private val BOLD_REGEX = Regex("""\*\*(.+?)\*\*|__(.+?)__""")
private val ITALIC_REGEX = Regex("""(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)|(?<!_)_(?!_)(.+?)(?<!_)_(?!_)""")
private val CODE_REGEX = Regex("""`([^`]+)`""")
private val HEADING_REGEX = Regex("""^(#{1,3})\s+(.+)$""")
private val BULLET_REGEX = Regex("""^\s*[-*]\s+(.+)$""")
private val CODE_BLOCK_REGEX = Regex("""```(?:\w*\n)?([\s\S]*?)```""", RegexOption.MULTILINE)

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
            withStyle(SpanStyle(
                fontFamily = FontFamily.Monospace,
                background = baseColor.copy(alpha = 0.08f)
            )) {
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
 * Apply inline markdown formatting: bold, italic, and inline code.
 * Processes in order: code (to prevent inner formatting), bold, italic.
 */
private fun AnnotatedString.Builder.appendFormattedInline(text: String, baseColor: Color) {
    // Tokenize into segments: code, bold, italic, plain
    data class Segment(val text: String, val style: SpanStyle?)

    val segments = mutableListOf<Segment>()
    var remaining = text

    // Extract inline code first (prevents bold/italic inside code)
    while (remaining.isNotEmpty()) {
        val codeMatch = CODE_REGEX.find(remaining)
        if (codeMatch == null) {
            segments.add(Segment(remaining, null))
            break
        }
        if (codeMatch.range.first > 0) {
            segments.add(Segment(remaining.substring(0, codeMatch.range.first), null))
        }
        segments.add(Segment(
            codeMatch.groupValues[1],
            SpanStyle(fontFamily = FontFamily.Monospace, background = baseColor.copy(alpha = 0.08f))
        ))
        remaining = remaining.substring(codeMatch.range.last + 1)
    }

    // Now process bold/italic in non-code segments
    for (segment in segments) {
        if (segment.style != null) {
            // Code segment — render as-is
            withStyle(segment.style) { append(segment.text) }
        } else {
            // Process bold and italic
            appendBoldItalic(segment.text)
        }
    }
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
            boldItalicMatch -> withStyle(SpanStyle(
                fontWeight = FontWeight.Bold,
                fontStyle = FontStyle.Italic
            )) { append(innerText) }
            boldMatch -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(innerText) }
            else -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append(innerText) }
        }

        remaining = remaining.substring(nextMatch.range.last + 1)
    }
}
