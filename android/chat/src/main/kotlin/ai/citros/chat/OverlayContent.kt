package ai.citros.chat

/**
 * LIVE OVERLAY composables rendered inside [OverlayService]'s ComposeView.
 *
 * These composables are what the user actually sees floating over other apps.
 * [OverlayServiceContent] in OverlayService.kt switches between them based on
 * [OverlaySurfaceMode]:
 *   - [OverlayMiniChatContent] — bottom-anchored floating panel (~40% height)
 *   - [OverlaySearchBarContent] — docked bottom search bar (~52dp)
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
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val SuccessColor = Color(0xFF22C55E)
private val ErrorColor = Color(0xFFEF4444)

/**
 * Mini-chat overlay content composable.
 *
 * Renders the bottom-anchored floating panel showing:
 * - Header with status, full/island buttons
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
    onOpenIsland: () -> Unit,
    onMinimize: () -> Unit = {},
    modifier: Modifier = Modifier
) {
    var isUndoStopVisible by rememberSaveable { mutableStateOf(false) }
    val scrollState = rememberScrollState()
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val panelColor = if (isDarkTheme) {
        Color(0xEB1C1C1E)
    } else {
        Color(0xEBF2F2F7)
    }
    val panelBackdropColor = if (isDarkTheme) {
        Color(0xB0000000)
    } else {
        Color.White.copy(alpha = 0.72f)
    }
    val statusColor = when (runState) {
        OverlayRunState.EXECUTING, OverlayRunState.COMPLETED -> SuccessColor
        OverlayRunState.FAILED, OverlayRunState.STOPPED -> ErrorColor
        OverlayRunState.IDLE -> flavor.primary.copy(alpha = 0.78f)
    }
    val statusText = when (runState) {
        OverlayRunState.IDLE -> "Ready"
        OverlayRunState.EXECUTING -> currentStep.label
        OverlayRunState.COMPLETED -> "Completed"
        OverlayRunState.FAILED -> "Action failed"
        OverlayRunState.STOPPED -> "Stopped"
    }

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
        color = panelColor,
        border = BorderStroke(1.dp, surfaces.separator),
        tonalElevation = 8.dp
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = OverlayUiConstants.MiniChatMaxHeight)
        ) {
            Box(
                modifier = Modifier
                    .matchParentSize()
                    .background(panelBackdropColor)
                    .blur(24.dp)
            )

            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 8.dp),
                contentAlignment = Alignment.Center
            ) {
                Box(
                    modifier = Modifier
                        .size(width = 36.dp, height = 4.dp)
                        .background(surfaces.labelTertiary.copy(alpha = 0.45f), RoundedCornerShape(999.dp))
                )
            }

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                CitrosDirectiveOrb(
                    flavor = flavor,
                    size = 24.dp
                )
                Text(
                    text = "Citros",
                    style = CitrosTypography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                    color = surfaces.labelPrimary
                )
                Surface(
                    shape = RoundedCornerShape(999.dp),
                    color = surfaces.surface2,
                    border = BorderStroke(1.dp, surfaces.separatorLight),
                    modifier = Modifier
                        .clickable(onClick = onOpenFull)
                        .semantics { contentDescription = "Open full app mode" }
                ) {
                    Text(
                        text = "Full",
                        style = CitrosTypography.labelSmall,
                        color = surfaces.labelSecondary,
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                    )
                }
                Surface(
                    shape = RoundedCornerShape(999.dp),
                    color = surfaces.surface2,
                    border = BorderStroke(1.dp, surfaces.separatorLight),
                    modifier = Modifier
                        .clickable(onClick = onOpenIsland)
                        .semantics { contentDescription = "Open dynamic island mode" }
                ) {
                    Text(
                        text = "Island",
                        style = CitrosTypography.labelSmall,
                        color = surfaces.labelSecondary,
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                    )
                }
                Surface(
                    shape = RoundedCornerShape(999.dp),
                    color = surfaces.surface2,
                    border = BorderStroke(1.dp, surfaces.separatorLight),
                    modifier = Modifier
                        .clickable(onClick = onMinimize)
                        .semantics { contentDescription = "Minimize to search bar" }
                ) {
                    Text(
                        text = "↓",
                        style = CitrosTypography.labelSmall,
                        color = surfaces.labelSecondary,
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                    )
                }
            }

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(statusColor, CircleShape)
                )
                Text(
                    text = statusText,
                    style = CitrosTypography.bodySmall,
                    color = surfaces.labelSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f)
                )
            }

            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(horizontal = 10.dp)
                    .verticalScroll(scrollState),
                verticalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                lines.forEach { line ->
                    val bubbleColor = when (line.type) {
                        OverlayLineType.SYSTEM -> surfaces.surface2
                        OverlayLineType.USER -> surfaces.surface1
                        OverlayLineType.QUEUED -> flavor.primary.copy(alpha = 0.14f)
                    }
                    val bubbleBorder = when (line.type) {
                        OverlayLineType.QUEUED -> flavor.primary.copy(alpha = 0.34f)
                        else -> surfaces.separatorLight
                    }

                    Surface(
                        modifier = if (line.type == OverlayLineType.QUEUED) Modifier.testTag(TEST_TAG_OVERLAY_QUEUED_LINE) else Modifier,
                        shape = RoundedCornerShape(
                            topStart = 14.dp,
                            topEnd = 14.dp,
                            bottomStart = if (line.type == OverlayLineType.SYSTEM) 6.dp else 14.dp,
                            bottomEnd = 14.dp
                        ),
                        color = bubbleColor,
                        border = BorderStroke(1.dp, bubbleBorder)
                    ) {
                        MarkdownText(
                            text = line.text,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
                            style = CitrosTypography.bodySmall,
                            color = if (line.type == OverlayLineType.QUEUED) {
                                surfaces.labelSecondary
                            } else {
                                surfaces.labelPrimary
                            }
                        )
                    }
                }

                if (runState == OverlayRunState.EXECUTING) {
                    AssistChip(
                        onClick = {},
                        label = { Text("Step ${currentStep.step} of ${currentStep.total}") },
                        colors = AssistChipDefaults.assistChipColors(
                            containerColor = surfaces.surface2,
                            labelColor = surfaces.labelSecondary
                        )
                    )
                }
            }

            if (isUndoStopVisible) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 2.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "Stopped",
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelSecondary,
                        modifier = Modifier.weight(1f)
                    )
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = surfaces.surface2,
                        border = BorderStroke(1.dp, surfaces.separatorLight),
                        modifier = Modifier.clickable {
                            onResumeOrRetry()
                            isUndoStopVisible = false
                        }
                    ) {
                        Text(
                            "Resume",
                            style = CitrosTypography.labelSmall,
                            color = flavor.primary,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                        )
                    }
                }
            } else if (runState == OverlayRunState.FAILED) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 2.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "Failed",
                        style = CitrosTypography.bodySmall,
                        color = ErrorColor.copy(alpha = 0.82f),
                        modifier = Modifier.weight(1f)
                    )
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = surfaces.surface2,
                        border = BorderStroke(1.dp, surfaces.separatorLight),
                        modifier = Modifier.clickable { onResumeOrRetry() }
                    ) {
                        Text(
                            "Retry",
                            style = CitrosTypography.labelSmall,
                            color = ErrorColor,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                        )
                    }
                }
            }

            val isExecuting = runState == OverlayRunState.EXECUTING
            val hasInputText = queuedMessageDraft.isNotBlank()
            val showStopButton = isExecuting && !hasInputText
            val sendEnabled = hasInputText || showStopButton
            val activeSendButtonColor = if (flavor == CitrosFlavor.NONE) {
                if (isDarkTheme) Color.White else Color.Black
            } else {
                flavor.primary
            }
            val activeSendIconTint = contrastOn(activeSendButtonColor)
            val inactiveSendButtonColor = if (isDarkTheme) surfaces.surface3 else surfaces.surface2
            val inactiveSendIconTint = surfaces.labelQuaternary

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Surface(
                    modifier = Modifier.weight(1f),
                    shape = RoundedCornerShape(22.dp),
                    color = surfaces.surface2,
                    border = BorderStroke(1.dp, surfaces.separatorLight)
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = 2.dp, end = 6.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        OutlinedTextField(
                            value = queuedMessageDraft,
                            onValueChange = onQueuedDraftChange,
                            modifier = Modifier
                                .weight(1f)
                                .heightIn(max = 132.dp),
                            placeholder = {
                                Text(
                                    text = if (isExecuting) "Steer or queue..." else "Message",
                                    style = CitrosTypography.bodyLarge
                                )
                            },
                            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                                imeAction = androidx.compose.ui.text.input.ImeAction.Send
                            ),
                            keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                                onSend = { onSubmitQueuedMessage() }
                            ),
                            singleLine = false,
                            maxLines = 6,
                            centerSingleLineContentWhenMultiline = true,
                            textStyle = CitrosTypography.bodyLarge,
                            shape = RoundedCornerShape(18.dp),
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = Color.Transparent,
                                unfocusedBorderColor = Color.Transparent,
                                focusedContainerColor = Color.Transparent,
                                unfocusedContainerColor = Color.Transparent,
                                cursorColor = flavor.primary,
                                focusedTextColor = surfaces.labelPrimary,
                                unfocusedTextColor = surfaces.labelPrimary,
                                focusedPlaceholderColor = surfaces.labelTertiary,
                                unfocusedPlaceholderColor = surfaces.labelTertiary
                            )
                        )
                        Box(
                            modifier = Modifier
                                .size(32.dp)
                                .clip(CircleShape)
                                .background(if (hasInputText) surfaces.surface3 else Color.Transparent)
                                .clickable(
                                    enabled = hasInputText,
                                    onClick = { onQueuedDraftChange("") }
                                ),
                            contentAlignment = Alignment.Center
                        ) {
                            if (hasInputText) {
                                MessageInputClearGlyph(
                                    tint = surfaces.labelSecondary.copy(alpha = 0.92f),
                                    modifier = Modifier.size(13.dp)
                                )
                            } else {
                                MessageInputMicGlyph(
                                    tint = surfaces.labelSecondary.copy(alpha = 0.72f),
                                    modifier = Modifier.size(16.dp)
                                )
                            }
                        }
                    }
                }

                MessageInputGlassIconButton(
                    onClick = {
                        when {
                            showStopButton -> {
                                onStopAction()
                                isUndoStopVisible = true
                            }
                            hasInputText -> onSubmitQueuedMessage()
                        }
                    },
                    enabled = sendEnabled,
                    backgroundColor = if (hasInputText) activeSendButtonColor else inactiveSendButtonColor,
                    iconTint = when {
                        showStopButton -> surfaces.labelPrimary
                        hasInputText -> activeSendIconTint
                        else -> inactiveSendIconTint
                    },
                    contentDescription = when {
                        showStopButton -> "Stop"
                        else -> "Send"
                    }
                ) { resolvedIconTint ->
                    if (showStopButton) {
                        Box(
                            modifier = Modifier
                                .size(12.dp)
                                .clip(RoundedCornerShape(3.dp))
                                .background(resolvedIconTint)
                        )
                    } else {
                        MessageInputArrowGlyph(
                            tint = resolvedIconTint,
                            modifier = Modifier.size(16.dp)
                        )
                    }
                }
            }
            }
        }
    }
}

private enum class SearchBarVisualState {
    IDLE,
    EXECUTING,
    COMPLETED,
    FAILED,
    UNREAD
}

private fun resolveSearchBarVisualState(
    runState: OverlayRunState,
    unreadCount: Int
): SearchBarVisualState = when {
    runState == OverlayRunState.EXECUTING -> SearchBarVisualState.EXECUTING
    runState == OverlayRunState.COMPLETED -> SearchBarVisualState.COMPLETED
    runState == OverlayRunState.FAILED || runState == OverlayRunState.STOPPED -> SearchBarVisualState.FAILED
    unreadCount > 0 -> SearchBarVisualState.UNREAD
    else -> SearchBarVisualState.IDLE
}

@Composable
internal fun OverlaySearchBarContent(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    statusLabel: String,
    unreadCount: Int,
    onExpand: () -> Unit,
    onStopAction: () -> Unit,
    modifier: Modifier = Modifier
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) { citrosDirectiveFlavorTokens(flavor, surfaces) }
    val visualState = remember(runState, unreadCount) {
        resolveSearchBarVisualState(runState = runState, unreadCount = unreadCount)
    }
    val barShape = RoundedCornerShape(cg(7))
    val barColor = if (isDarkTheme) {
        Color(0xE01C1C1E)
    } else {
        Color(0xE0F2F2F7)
    }
    val barShadowColor = if (isDarkTheme) Color(0x4D000000) else Color(0x0F000000)
    val placeholderText = "Ask Citros anything..."
    val maxUnread = unreadCount.coerceAtMost(99)
    val pulseTransition = rememberInfiniteTransition(label = "search_bar_pulse")
    val pulseAlpha by pulseTransition.animateFloat(
        initialValue = 0.45f,
        targetValue = 0.70f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1200, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "search_bar_pulse_alpha"
    )
    val isFailed = visualState == SearchBarVisualState.FAILED
    val targetOrbColor = if (isFailed) {
        surfaces.red
    } else {
        flavorTokens.orbColor
    }
    val targetOrbInner = if (isFailed) {
        Color.White.copy(alpha = 0.20f)
    } else {
        flavorTokens.orbInner
    }
    val targetOrbGlow = if (isFailed) {
        Color(0x26FF453A)
    } else {
        flavorTokens.orbGlow
    }
    val orbColor by animateColorAsState(
        targetValue = targetOrbColor,
        animationSpec = tween(durationMillis = 250, easing = FastOutSlowInEasing),
        label = "search_bar_orb_color"
    )
    val orbInner by animateColorAsState(
        targetValue = targetOrbInner,
        animationSpec = tween(durationMillis = 250, easing = FastOutSlowInEasing),
        label = "search_bar_orb_inner"
    )
    val orbGlow by animateColorAsState(
        targetValue = targetOrbGlow,
        animationSpec = tween(durationMillis = 250, easing = FastOutSlowInEasing),
        label = "search_bar_orb_glow"
    )
    val resolvedStatusText = statusLabel.takeIf { it.isNotBlank() && it != "Waiting..." } ?: when (visualState) {
        SearchBarVisualState.IDLE,
        SearchBarVisualState.UNREAD -> placeholderText
        SearchBarVisualState.EXECUTING -> "Working..."
        SearchBarVisualState.COMPLETED -> "Completed"
        SearchBarVisualState.FAILED -> "Action failed"
    }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = cg(13), max = cg(13))
            .shadow(
                elevation = 16.dp,
                shape = barShape,
                clip = false,
                ambientColor = barShadowColor,
                spotColor = barShadowColor
            )
            .clip(barShape)
            .background(barColor, barShape)
            .then(
                if (isDarkTheme) Modifier else Modifier.border(
                    width = 1.dp,
                    color = surfaces.separator,
                    shape = barShape
                )
            )
            .clickable(onClick = onExpand)
            .semantics { contentDescription = "Overlay search bar" }
    ) {
        Box(
            modifier = Modifier
                .matchParentSize()
                .background(barColor)
                .blur(cg(10))
        )

        Row(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = cg(2)),
            horizontalArrangement = Arrangement.spacedBy(cg(2)),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier.size(cg(9)),
                contentAlignment = Alignment.Center
            ) {
                CitrosDirectiveOrb(
                    flavor = flavor,
                    size = cg(9),
                    colorOverride = orbColor,
                    innerOverride = orbInner,
                    glowOverride = orbGlow
                )
                if (visualState == SearchBarVisualState.UNREAD) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .size(cg(2.5f))
                            .background(surfaces.red, CircleShape)
                    )
                }
            }

            Crossfade(
                targetState = visualState,
                animationSpec = tween(durationMillis = 250),
                modifier = Modifier.weight(1f),
                label = "search_bar_center_content"
            ) { state ->
                when (state) {
                    SearchBarVisualState.IDLE -> {
                        Text(
                            text = placeholderText,
                            style = CitrosTypography.bodyMedium.copy(fontSize = 14.sp),
                            color = surfaces.labelTertiary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }

                    SearchBarVisualState.EXECUTING -> {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(cg(2)),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(6.dp)
                                    .background(orbColor.copy(alpha = pulseAlpha), CircleShape)
                            )
                            Text(
                                text = resolvedStatusText,
                                style = CitrosTypography.bodyMedium.copy(
                                    fontSize = 14.sp,
                                    fontWeight = FontWeight.Normal,
                                    fontStyle = androidx.compose.ui.text.font.FontStyle.Italic
                                ),
                                color = surfaces.labelSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f)
                            )
                            Box(
                                modifier = Modifier
                                    .background(surfaces.red, RoundedCornerShape(cg(2.5f)))
                                    .clickable(onClick = onStopAction)
                                    .padding(horizontal = cg(2.5f), vertical = cg(1)),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Stop",
                                    style = CitrosTypography.labelSmall.copy(fontSize = 12.sp),
                                    fontWeight = FontWeight.Bold,
                                    color = Color.White
                                )
                            }
                        }
                    }

                    SearchBarVisualState.COMPLETED -> {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(cg(2)),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = resolvedStatusText,
                                style = CitrosTypography.bodyMedium.copy(fontSize = 14.sp),
                                fontWeight = FontWeight.Medium,
                                color = surfaces.labelPrimary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f)
                            )
                            Box(
                                modifier = Modifier
                                    .size(cg(5))
                                    .background(surfaces.green.copy(alpha = 0.20f), CircleShape),
                                contentAlignment = Alignment.Center
                            ) {
                                CitrosIcon(
                                    imageVector = CitrosIcons.SearchBarCheck,
                                    contentDescription = null,
                                    modifier = Modifier.size(10.dp),
                                    tint = surfaces.green
                                )
                            }
                        }
                    }

                    SearchBarVisualState.FAILED -> {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(cg(2)),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = resolvedStatusText,
                                style = CitrosTypography.bodyMedium.copy(fontSize = 14.sp),
                                fontWeight = FontWeight.Medium,
                                color = surfaces.red,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f)
                            )
                            Box(
                                modifier = Modifier
                                    .size(cg(5))
                                    .background(surfaces.red.copy(alpha = 0.18f), CircleShape),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "!",
                                    style = CitrosTypography.labelSmall.copy(fontSize = 11.sp),
                                    fontWeight = FontWeight.Bold,
                                    color = surfaces.red
                                )
                            }
                        }
                    }

                    SearchBarVisualState.UNREAD -> {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(cg(2)),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = placeholderText,
                                style = CitrosTypography.bodyMedium.copy(fontSize = 14.sp),
                                color = surfaces.labelTertiary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f)
                            )
                            Box(
                                modifier = Modifier
                                    .widthIn(min = cg(5))
                                    .heightIn(min = cg(5))
                                    .background(surfaces.red, CircleShape)
                                    .padding(horizontal = cg(1.5f), vertical = 0.dp),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = maxUnread.toString(),
                                    style = CitrosTypography.labelSmall.copy(fontSize = 11.sp),
                                    fontWeight = FontWeight.Bold,
                                    color = Color.White
                                )
                            }
                        }
                    }
                }
            }

            if (visualState == SearchBarVisualState.IDLE || visualState == SearchBarVisualState.UNREAD) {
                Box(
                    modifier = Modifier
                        .size(cg(9))
                        .clickable(onClick = onExpand),
                    contentAlignment = Alignment.Center
                ) {
                    CitrosIcon(
                        imageVector = CitrosIcons.SearchBarMic,
                        contentDescription = null,
                        modifier = Modifier.size(cg(4)),
                        tint = surfaces.labelTertiary
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
internal fun OverlayDynamicIslandContent(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    currentStepLabel: String,
    unreadCount: Int,
    onExpand: () -> Unit,
    onStopAction: () -> Unit,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    val isExpanded = runState != OverlayRunState.IDLE || unreadCount > 0
    val isFailed = runState == OverlayRunState.FAILED || runState == OverlayRunState.STOPPED
    val isExecuting = runState == OverlayRunState.EXECUTING
    val showErrorBadge = runState == OverlayRunState.FAILED || runState == OverlayRunState.STOPPED
    val orbSize = if (isExpanded) cg(6) else cg(5)
    val islandWidth = if (isExpanded) cg(74) else cg(44)

    val titleText = when (runState) {
        OverlayRunState.IDLE -> if (unreadCount > 0) "Updates" else ""
        OverlayRunState.EXECUTING -> "Working..."
        OverlayRunState.COMPLETED -> "Completed"
        OverlayRunState.FAILED -> "Action failed"
        OverlayRunState.STOPPED -> "Stopped"
    }
    val subtitleText = when (runState) {
        OverlayRunState.IDLE -> if (unreadCount > 0) "$unreadCount unread updates" else "Tap to open"
        OverlayRunState.EXECUTING -> currentStepLabel
        OverlayRunState.COMPLETED -> "Tap to review"
        OverlayRunState.FAILED -> "Tap to open settings"
        OverlayRunState.STOPPED -> "Tap to resume"
    }
    val islandColor = if (isDarkTheme) {
        Color(0xEB1C1C1E)
    } else {
        Color(0xEBF2F2F7)
    }
    val islandBorder = if (isDarkTheme) null else BorderStroke(1.dp, surfaces.separator)

    Surface(
        modifier = modifier
            .width(islandWidth)
            .combinedClickable(
                onClick = onExpand,
                onLongClick = onDismiss
            )
            .semantics { contentDescription = "Dynamic island overlay" },
        shape = RoundedCornerShape(cg(7)),
        color = islandColor,
        border = islandBorder,
        tonalElevation = 8.dp
    ) {
        Box(
            modifier = Modifier.fillMaxWidth()
        ) {
            Box(
                modifier = Modifier
                    .matchParentSize()
                    .background(islandColor)
                    .blur(40.dp)
            )

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = cg(3), vertical = cg(1.25f)),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(cg(1.5f))
            ) {
                Box(
                    modifier = Modifier.size(orbSize + cg(2)),
                    contentAlignment = Alignment.Center
                ) {
                    if (isExecuting) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(orbSize + cg(1)),
                            color = flavorTokens.orbColor,
                            trackColor = if (isDarkTheme) {
                                Color.White.copy(alpha = 0.14f)
                            } else {
                                Color.Black.copy(alpha = 0.14f)
                            },
                            strokeWidth = 1.75.dp
                        )
                    }
                    CitrosDirectiveOrb(
                        flavor = flavor,
                        size = orbSize,
                        colorOverride = if (isFailed) surfaces.red else null,
                        innerOverride = if (isFailed) Color.White.copy(alpha = 0.22f) else null,
                        glowOverride = if (isFailed) surfaces.red.copy(alpha = 0.30f) else flavorTokens.orbGlow
                    )
                    if (showErrorBadge) {
                        Box(
                            modifier = Modifier
                                .align(Alignment.TopEnd)
                                .size(cg(3))
                                .background(surfaces.red, CircleShape),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                text = "!",
                                style = CitrosTypography.labelSmall.copy(fontSize = 9.sp),
                                fontWeight = FontWeight.Bold,
                                color = Color.White
                            )
                        }
                    }
                }
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(1.dp)
                ) {
                    if (titleText.isNotBlank()) {
                        Text(
                            text = titleText,
                            style = CitrosTypography.labelLarge.copy(fontSize = 13.sp),
                            color = surfaces.labelPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }
                    Text(
                        text = subtitleText,
                        style = if (titleText.isBlank()) {
                            CitrosTypography.labelMedium.copy(fontSize = 12.sp)
                        } else {
                            CitrosTypography.labelSmall.copy(fontSize = 11.sp)
                        },
                        color = surfaces.labelSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }
                when (runState) {
                    OverlayRunState.EXECUTING -> {
                        Surface(
                            shape = RoundedCornerShape(cg(2.5f)),
                            color = surfaces.red,
                            modifier = Modifier.clickable(onClick = onStopAction)
                        ) {
                            Text(
                                text = "Stop",
                                style = CitrosTypography.labelSmall.copy(fontSize = 11.sp),
                                color = Color.White,
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier.padding(horizontal = cg(2.5f), vertical = cg(1f))
                            )
                        }
                    }
                    OverlayRunState.COMPLETED -> {
                        Box(
                            modifier = Modifier
                                .size(cg(4.5f))
                                .background(surfaces.green, CircleShape),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                text = "✓",
                                style = CitrosTypography.labelSmall.copy(fontSize = 10.sp),
                                color = contrastOn(surfaces.green)
                            )
                        }
                    }
                    OverlayRunState.FAILED,
                    OverlayRunState.STOPPED -> {
                        // Error indicator is shown as a red badge over the orb.
                    }
                    OverlayRunState.IDLE -> {
                        if (unreadCount > 0) {
                            Box(
                                modifier = Modifier
                                    .size(cg(4.5f))
                                    .background(flavor.primary, CircleShape),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = unreadCount.toString(),
                                    style = CitrosTypography.labelSmall.copy(fontSize = 10.sp),
                                    color = contrastOn(flavor.primary)
                                )
                            }
                        }
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
