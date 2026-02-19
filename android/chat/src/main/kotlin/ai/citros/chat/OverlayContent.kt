package ai.citros.chat

/**
 * LIVE OVERLAY composables rendered inside [OverlayService]'s ComposeView.
 *
 * These composables are what the user actually sees floating over other apps.
 * [OverlayServiceContent] in OverlayService.kt switches between them based on
 * [OverlaySurfaceMode]:
 *   - [OverlayMiniChatContent] — bottom-anchored floating panel (~40% height)
 *   - [OverlayBubbleContent] — circular floating indicator (~56dp)
 *
 * ⚠️  NOT the same as OverlayPortedScreen.kt, which contains FULL-SCREEN copies
 * of these composables embedded inside ChatActivity (for preview and in-app use).
 * Changes to overlay behavior/rendering must be made HERE to affect the actual
 * floating overlay. Update OverlayPortedScreen.kt separately if parity is needed.
 */

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayStep
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

private val SuccessColor = Color(0xFF22C55E)
private val ErrorColor = Color(0xFFEF4444)

/**
 * Mini-chat overlay content composable.
 *
 * Renders the bottom-anchored floating panel showing:
 * - Header with status, expand/bubble buttons
 * - Scrollable transcript of tool execution lines
 * - Queued message input
 * - Stop button during execution
 *
 * Used by both [OverlayService] and [OverlayPreviewScreen].
 */
@Composable
internal fun OverlayMiniChatContent(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    currentStep: OverlayStep,
    lines: List<OverlayLine>,
    queuedMessageDraft: String,
    onQueuedDraftChange: (String) -> Unit,
    onSubmitQueuedMessage: () -> Unit,
    onStopAction: () -> Unit,
    onResumeOrRetry: () -> Unit,
    onOpenFull: () -> Unit,
    onOpenBubble: () -> Unit,
    modifier: Modifier = Modifier
) {
    var isUndoStopVisible by rememberSaveable { mutableStateOf(false) }
    val scrollState = rememberScrollState()

    // Auto-scroll to bottom when lines change or last line content updates.
    val lastLineText = lines.lastOrNull()?.text
    LaunchedEffect(lines.size, lastLineText) {
        kotlinx.coroutines.yield()
        kotlinx.coroutines.delay(100)
        scrollState.animateScrollTo(scrollState.maxValue)
        // Second pass: content may still be measuring (e.g. markdown rendering).
        kotlinx.coroutines.delay(300)
        if (scrollState.maxValue > scrollState.value) {
            scrollState.animateScrollTo(scrollState.maxValue)
        }
    }

    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(OverlayUiConstants.MiniChatCornerRadius),
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.96f),
        border = BorderStroke(1.dp, flavor.primary.copy(alpha = 0.45f)),
        tonalElevation = 6.dp
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = OverlayUiConstants.MiniChatMaxHeight),
            verticalArrangement = Arrangement.spacedBy(0.dp)
        ) {
            // Header
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 9.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                CitrosFloatingAppIconGraphic(
                    flavor = flavor,
                    size = 22.dp,
                    showBackground = false,
                    orbOnly = true
                )
                Text(
                    text = when (runState) {
                        OverlayRunState.IDLE -> "Ready"
                        OverlayRunState.EXECUTING -> currentStep.label
                        OverlayRunState.COMPLETED -> "Completed"
                        OverlayRunState.FAILED -> "Action failed"
                        OverlayRunState.STOPPED -> "Stopped"
                    },
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                    color = when (runState) {
                        OverlayRunState.EXECUTING, OverlayRunState.IDLE -> MaterialTheme.colorScheme.onSurface
                        else -> runStateColor(runState)
                    }
                )
                TextButton(
                    onClick = onOpenFull,
                    contentPadding = OverlayUiConstants.CompactChipPadding,
                    colors = ButtonDefaults.textButtonColors(contentColor = flavor.primary),
                    modifier = Modifier.semantics { contentDescription = "Open full app mode" }
                ) {
                    Text("Full", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
                }
                TextButton(
                    onClick = onOpenBubble,
                    contentPadding = OverlayUiConstants.CompactChipPadding,
                    colors = ButtonDefaults.textButtonColors(contentColor = flavor.primary),
                    modifier = Modifier.semantics { contentDescription = "Open bubble mode" }
                ) {
                    Text("Bubble", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
                }
            }

            // Transcript lines
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(horizontal = 10.dp)
                    .verticalScroll(scrollState),
                verticalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                lines.forEach { line ->
                    Row(
                        verticalAlignment = Alignment.Top,
                        horizontalArrangement = Arrangement.spacedBy(6.dp)
                    ) {
                        when (line.type) {
                            OverlayLineType.USER -> Text(
                                ">",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            OverlayLineType.SYSTEM -> Text(
                                "-",
                                style = MaterialTheme.typography.labelSmall,
                                color = flavor.primary
                            )
                            OverlayLineType.QUEUED -> {
                                Surface(
                                    shape = RoundedCornerShape(OverlayUiConstants.PillCornerRadius),
                                    color = flavor.primary.copy(alpha = 0.2f)
                                ) {
                                    Text(
                                        "Queued",
                                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = flavor.primary
                                    )
                                }
                            }
                        }
                        MarkdownText(
                            text = line.text,
                            style = MaterialTheme.typography.bodySmall,
                            color = if (line.type == OverlayLineType.QUEUED) {
                                MaterialTheme.colorScheme.onSurface.copy(alpha = 0.7f)
                            } else {
                                MaterialTheme.colorScheme.onSurface
                            }
                        )
                    }
                }

                if (runState == OverlayRunState.EXECUTING) {
                    AssistChip(
                        onClick = {},
                        label = { Text("Step ${currentStep.step} of ${currentStep.total}") }
                    )
                }

                if (runState == OverlayRunState.FAILED) {
                    Surface(
                        shape = RoundedCornerShape(OverlayUiConstants.ErrorCardCornerRadius),
                        color = ErrorColor.copy(alpha = 0.12f),
                        border = BorderStroke(1.dp, ErrorColor.copy(alpha = 0.28f))
                    ) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 10.dp, vertical = 9.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                "Action failed",
                                modifier = Modifier.weight(1f),
                                style = MaterialTheme.typography.bodySmall,
                                color = ErrorColor
                            )
                        }
                    }
                }
            }

            // Footer: undo-stop bar or input bar
            if (isUndoStopVisible) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "Action stopped",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f),
                        modifier = Modifier.weight(1f)
                    )
                    OutlinedButton(
                        onClick = {
                            onResumeOrRetry()
                            isUndoStopVisible = false
                        },
                        contentPadding = OverlayUiConstants.CompactActionPadding
                    ) {
                        Text("Undo", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
                    }
                }
            } else {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    val queueKeyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        imeAction = androidx.compose.ui.text.input.ImeAction.Send
                    )
                    val queueKeyboardActions = androidx.compose.foundation.text.KeyboardActions(
                        onSend = { onSubmitQueuedMessage() }
                    )
                    TextField(
                        value = queuedMessageDraft,
                        onValueChange = onQueuedDraftChange,
                        modifier = Modifier
                            .weight(1f),
                        singleLine = true,
                        placeholder = { Text("Queue a follow-up...") },
                        keyboardOptions = queueKeyboardOptions,
                        keyboardActions = queueKeyboardActions,
                        colors = TextFieldDefaults.colors(
                            focusedContainerColor = MaterialTheme.colorScheme.surface,
                            unfocusedContainerColor = MaterialTheme.colorScheme.surface,
                            focusedIndicatorColor = flavor.primary,
                            unfocusedIndicatorColor = MaterialTheme.colorScheme.outline.copy(alpha = 0.35f),
                            cursorColor = flavor.primary
                        )
                    )
                    OutlinedButton(
                        onClick = onSubmitQueuedMessage,
                        contentPadding = OverlayUiConstants.CompactActionPadding,
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = flavor.primary)
                    ) {
                        Text("Queue", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
                    }

                    if (runState == OverlayRunState.EXECUTING) {
                        Button(
                            onClick = {
                                onStopAction()
                                isUndoStopVisible = true
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = ErrorColor),
                            contentPadding = OverlayUiConstants.PrimaryActionPadding
                        ) {
                            Text("Stop", style = MaterialTheme.typography.labelSmall, color = contrastOn(ErrorColor))
                        }
                    }
                }
            }
        }
    }
}

/**
 * Bubble overlay content composable.
 *
 * Renders the small circular floating indicator with:
 * - Hero badge with progress spinner during execution
 * - Unread count / status badge
 * - Long-press quick actions (Stop, Expand, Dismiss)
 *
 * Used by both [OverlayService] and [OverlayPreviewScreen].
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
internal fun OverlayBubbleContent(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    unreadCount: Int,
    onExpand: () -> Unit,
    onStopAction: () -> Unit,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier
) {
    var isQuickActionsOpen by rememberSaveable { mutableStateOf(false) }

    Box(modifier = modifier) {
        // Quick actions popup
        if (isQuickActionsOpen) {
            Surface(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(bottom = 74.dp)
                    .width(OverlayUiConstants.BubbleQuickActionsWidth),
                shape = RoundedCornerShape(OverlayUiConstants.StandardCardCornerRadius),
                color = MaterialTheme.colorScheme.surface.copy(alpha = 0.98f),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.35f))
            ) {
                Column(modifier = Modifier.padding(vertical = 6.dp)) {
                    TextButton(
                        onClick = {
                            onStopAction()
                            isQuickActionsOpen = false
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Stop action" }
                    ) { Text("Stop Action") }
                    TextButton(
                        onClick = {
                            onExpand()
                            isQuickActionsOpen = false
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Expand overlay" }
                    ) { Text("Expand") }
                    TextButton(
                        onClick = {
                            isQuickActionsOpen = false
                            onDismiss()
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Dismiss overlay" }
                    ) { Text("Dismiss Overlay") }
                }
            }
        }

        // Bubble
        Box(modifier = Modifier.align(Alignment.BottomEnd)) {
            Surface(
                modifier = Modifier
                    .size(OverlayUiConstants.BubbleSize)
                    .semantics { contentDescription = "Overlay bubble" },
                shape = CircleShape,
                color = Color.Transparent,
                border = null,
                tonalElevation = 6.dp
            ) {
                Box(modifier = Modifier.fillMaxSize()) {
                    Box(contentAlignment = Alignment.Center, modifier = Modifier.fillMaxSize()) {
                        CitrosFloatingAppIconGraphic(
                            flavor = flavor,
                            size = OverlayUiConstants.BubbleSize * 2.1f,
                            showBackground = false,
                            orbOnly = true
                        )
                        if (runState == OverlayRunState.EXECUTING) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(OverlayUiConstants.BubbleProgressSize),
                                strokeWidth = 2.dp,
                                color = flavor.primary,
                                trackColor = Color.Transparent
                            )
                        }
                    }
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .combinedClickable(
                                onClick = {
                                    if (isQuickActionsOpen) {
                                        isQuickActionsOpen = false
                                    } else {
                                        onExpand()
                                    }
                                },
                                onLongClick = { isQuickActionsOpen = !isQuickActionsOpen }
                            )
                    )
                }
            }

            // Badge
            if (runState == OverlayRunState.COMPLETED || runState == OverlayRunState.FAILED || unreadCount > 0) {
                val badgeColor = when {
                    runState == OverlayRunState.COMPLETED -> SuccessColor
                    runState == OverlayRunState.FAILED -> ErrorColor
                    else -> flavor.primary
                }
                Surface(
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .size(OverlayUiConstants.BubbleBadgeSize),
                    shape = CircleShape,
                    color = badgeColor
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            text = when {
                                runState == OverlayRunState.COMPLETED -> "✓"
                                runState == OverlayRunState.FAILED -> "!"
                                else -> unreadCount.toString()
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = contrastOn(badgeColor)
                        )
                    }
                }
            }
        }
    }
}

/**
 * Map [OverlayRunState] to its display color.
 */
internal fun runStateColor(runState: OverlayRunState): Color = when (runState) {
    OverlayRunState.IDLE -> Color.Unspecified
    OverlayRunState.EXECUTING, OverlayRunState.COMPLETED -> SuccessColor
    OverlayRunState.FAILED, OverlayRunState.STOPPED -> ErrorColor
}
