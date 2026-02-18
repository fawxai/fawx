package ai.citros.chat

/**
 * Overlay UI for phone control task execution.
 *
 * Supports two modes:
 * - **Live mode**: pass a [ChatViewModel] to display real-time tool execution state
 *   via [OverlayStateMapper], with working stop/resume, queued messages, and unread badges.
 * - **Preview mode**: omit the viewModel to use simulated sample data for UI iteration.
 */

import android.content.Context
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
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.ImeAction
import ai.citros.core.*
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

// Uses OverlaySurfaceMode from OverlayController.kt

private object OverlayColors {
    val AppChrome = Color(0xFF101423)
    val PreviewBackground = Color(0xFF121727)
    val FakePhoneBase = Color(0xFF1A1A2E)
    val FakePhoneBar = Color(0xFF16213E)
    val FakePhoneSurface = Color(0xFF0F3460)
    val FakePhoneBorder = Color(0xFF1A1A3E)
    val FakePhoneTextMuted = Color(0xFF9FA6B2)
    val FakePhoneTextBright = Color(0xFFE0E0E0)
    val FakePhoneTextDim = Color(0xFFBBBBBB)
    val FakePhoneAccent = Color(0xFFFFD600)
    val FakePhoneChevron = Color(0xFF6B7280)

    val Success = Color(0xFF22C55E)
    val Error = Color(0xFFEF4444)
}

// Preview-only demo strings/data. These stay local for fast UI iteration and are not production i18n resources.
private val overlaySteps = listOf(
    OverlayStep(step = 1, total = 5, label = "Opening Settings"),
    OverlayStep(step = 2, total = 5, label = "Scrolling to Wi-Fi"),
    OverlayStep(step = 3, total = 5, label = "Tapping Wi-Fi toggle"),
    OverlayStep(step = 4, total = 5, label = "Connecting to network"),
    OverlayStep(step = 5, total = 5, label = "Verifying connection")
)

private val overlayLines = listOf(
    OverlayLine(id = 1, type = OverlayLineType.USER, text = "Turn on Wi-Fi and connect to home network"),
    OverlayLine(id = 2, type = OverlayLineType.SYSTEM, text = "Opening Settings app..."),
    OverlayLine(id = 3, type = OverlayLineType.SYSTEM, text = "Navigating to Network & internet"),
    OverlayLine(id = 4, type = OverlayLineType.SYSTEM, text = "Found Wi-Fi section, scrolling..."),
    OverlayLine(id = 5, type = OverlayLineType.SYSTEM, text = "Enabling Wi-Fi toggle"),
    OverlayLine(id = 6, type = OverlayLineType.QUEUED, text = "also check bluetooth")
)

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
internal fun OverlayPreviewScreen(
    context: Context,
    onBack: () -> Unit,
    viewModel: ChatViewModel? = null, // Optional: connect to live ChatViewModel
    onOverlayMinimized: (() -> Unit)? = null,
    onNavigateToChat: (() -> Unit)? = null
) {
    val onboardingPrefs = remember(context) {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    }
    var flavor by rememberSaveable {
        mutableStateOf(readSelectedFlavor(context))
    }
    DisposableEffect(onboardingPrefs) {
        val listener = android.content.SharedPreferences.OnSharedPreferenceChangeListener { prefs, key ->
            if (key == PREF_SELECTED_FLAVOR) {
                flavor = CitrosFlavor.fromStorage(
                    prefs.getString(PREF_SELECTED_FLAVOR, CitrosFlavor.TANGERINE.storageValue)
                )
            }
        }
        onboardingPrefs.registerOnSharedPreferenceChangeListener(listener)
        onDispose {
            onboardingPrefs.unregisterOnSharedPreferenceChangeListener(listener)
        }
    }
    var surfaceMode by rememberSaveable { mutableStateOf(OverlaySurfaceMode.MINI_CHAT) }
    var runState by rememberSaveable { mutableStateOf(OverlayRunState.EXECUTING) }
    var stepIndex by rememberSaveable { mutableIntStateOf(0) }
    var showBubbleQuickActions by rememberSaveable { mutableStateOf(false) }
    var isUndoStopVisible by rememberSaveable { mutableStateOf(false) }
    var queuedMessageDraft by rememberSaveable { mutableStateOf("") }
    var isDismissingOverlay by rememberSaveable { mutableStateOf(false) }
    var fullAppMessageDraft by rememberSaveable { mutableStateOf("") }
    val unreadCount = viewModel?.unreadCount?.intValue ?: 2
    val coroutineScope = rememberCoroutineScope()

    // Derive overlay state from ChatViewModel if provided, otherwise use demo data
    val liveOverlayState = if (viewModel != null) {
        remember {
            androidx.compose.runtime.derivedStateOf {
                OverlayStateMapper.mapToOverlayState(
                    messages = viewModel.messages.toList(),
                    isLoading = viewModel.isLoading.value
                )
            }
        }.value
    } else null
    
    // Use live state if available, otherwise use demo state
    val activeSteps = liveOverlayState?.steps ?: overlaySteps
    val activeLines = if (liveOverlayState != null) {
        val queued = viewModel?.queuedMessage?.value
        if (queued.isNullOrBlank()) liveOverlayState.lines else liveOverlayState.lines +
            OverlayLine(
                id = (liveOverlayState.lines.maxOfOrNull { it.id } ?: 0) + 1,
                type = OverlayLineType.QUEUED,
                text = queued
            )
    } else {
        overlayLines
    }
    val activeRunState = liveOverlayState?.runState ?: runState
    val activeStepIndex = liveOverlayState?.currentStepIndex ?: stepIndex

    val currentStep = if (activeSteps.isEmpty()) {
        overlaySteps.first()
    } else {
        activeSteps.getOrElse(activeStepIndex.coerceIn(0, activeSteps.lastIndex)) { 
            overlaySteps.first() 
        }
    }

    // Step ticker for demo mode only (not used when viewModel is provided)
    LaunchedEffect(runState) {
        if (viewModel == null && runState == OverlayRunState.EXECUTING) {
            while (isActive) {
                delay(OverlayUiConstants.STEP_TICKER_DELAY_MS)
                stepIndex = (stepIndex + 1) % overlaySteps.size
            }
        }
    }

    // No auto-hide timer for undo banner — stays until user sends a message or taps Resume (#473)


    LaunchedEffect(viewModel?.queuedMessage?.value) {
        if (viewModel != null) {
            queuedMessageDraft = viewModel.queuedMessage.value.orEmpty()
        }
    }

    LaunchedEffect(surfaceMode, viewModel) {
        if (viewModel != null && surfaceMode == OverlaySurfaceMode.MINI_CHAT) {
            viewModel.resetUnreadCount()
        }
    }

    val stopAction = {
        if (viewModel != null) {
            viewModel.cancelToolExecution()
        } else {
            runState = OverlayRunState.STOPPED
        }
        isUndoStopVisible = true
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Phone Control Overlay") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 12.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(OverlayUiConstants.ControlPanelCornerRadius),
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.35f))
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        OverlayModeChip(
                            label = "Mini-Chat",
                            selected = surfaceMode == OverlaySurfaceMode.MINI_CHAT,
                            accent = flavor.primary,
                            onClick = {
                                surfaceMode = OverlaySurfaceMode.MINI_CHAT
                                showBubbleQuickActions = false
                            }
                        )
                        OverlayModeChip(
                            label = "Bubble",
                            selected = surfaceMode == OverlaySurfaceMode.BUBBLE,
                            accent = flavor.primary,
                            onClick = {
                                surfaceMode = OverlaySurfaceMode.BUBBLE
                                showBubbleQuickActions = false
                            }
                        )
                        OverlayModeChip(
                            label = "Full App",
                            selected = surfaceMode == OverlaySurfaceMode.FULL_APP,
                            accent = flavor.primary,
                            onClick = {
                                showBubbleQuickActions = false
                                if (viewModel != null) {
                                    onNavigateToChat?.invoke()
                                } else {
                                    surfaceMode = OverlaySurfaceMode.FULL_APP
                                }
                            }
                        )
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(6.dp)
                    ) {
                        OverlayStateChip(
                            label = "Run",
                            selected = activeRunState == OverlayRunState.EXECUTING,
                            onClick = {
                                if (viewModel == null) {
                                    runState = OverlayRunState.EXECUTING
                                    isUndoStopVisible = false
                                } else {
                                    viewModel.resumeExecution()
                                }
                            },
                            tint = flavor.primary,
                            modifier = Modifier.weight(1f)
                        )
                        OverlayStateChip(
                            label = "Done",
                            selected = activeRunState == OverlayRunState.COMPLETED,
                            onClick = {
                                if (viewModel == null) {
                                    runState = OverlayRunState.COMPLETED
                                    isUndoStopVisible = false
                                }
                            },
                            tint = OverlayColors.Success,
                            modifier = Modifier.weight(1f)
                        )
                        OverlayStateChip(
                            label = "Fail",
                            selected = activeRunState == OverlayRunState.FAILED,
                            onClick = {
                                if (viewModel == null) {
                                    runState = OverlayRunState.FAILED
                                    isUndoStopVisible = false
                                }
                            },
                            tint = OverlayColors.Error,
                            modifier = Modifier.weight(1f)
                        )
                        OverlayStateChip(
                            label = "Stop",
                            selected = activeRunState == OverlayRunState.STOPPED,
                            onClick = {
                                stopAction()
                            },
                            tint = OverlayColors.Error,
                            modifier = Modifier.weight(1f)
                        )
                    }
                }
            }

            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(OverlayUiConstants.PreviewCornerRadius))
                    .background(OverlayColors.PreviewBackground)
                    .border(
                        width = 1.dp,
                        color = MaterialTheme.colorScheme.outline.copy(alpha = 0.35f),
                        shape = RoundedCornerShape(OverlayUiConstants.PreviewCornerRadius)
                    )
            ) {
                when (surfaceMode) {
                    OverlaySurfaceMode.FULL_APP -> {
                        FullAppOverlayContent(
                            flavor = flavor,
                            runState = activeRunState,
                            currentStep = currentStep,
                            lines = activeLines,
                            messageDraft = fullAppMessageDraft,
                            onMessageDraftChange = { fullAppMessageDraft = it },
                            onSendMessage = {
                                val draft = fullAppMessageDraft.trim()
                                if (draft.isNotEmpty()) {
                                    viewModel?.sendMessage(draft)
                                    fullAppMessageDraft = ""
                                }
                            },
                            onReturnToOverlay = {
                                if (viewModel != null) onNavigateToChat?.invoke() else surfaceMode = OverlaySurfaceMode.MINI_CHAT
                            },
                            onStopAction = stopAction
                        )
                    }

                    OverlaySurfaceMode.MINI_CHAT,
                    OverlaySurfaceMode.BUBBLE -> {
                        FakeUnderlyingPhoneSurface()

                        if (surfaceMode == OverlaySurfaceMode.MINI_CHAT) {
                            MiniChatOverlayCard(
                                modifier = Modifier
                                    .align(Alignment.BottomCenter)
                                    .padding(horizontal = 10.dp, vertical = 10.dp),
                                flavor = flavor,
                                runState = activeRunState,
                                currentStep = currentStep,
                                lines = activeLines,
                                queuedMessageDraft = queuedMessageDraft,
                                onQueuedDraftChange = {
                                    queuedMessageDraft = it
                                    viewModel?.setQueuedMessage(it)
                                },
                                onSubmitQueuedMessage = {
                                    val draft = queuedMessageDraft.trim()
                                    if (draft.isNotEmpty() && viewModel != null) {
                                        if (viewModel.isLoading.value) {
                                            // Model is busy — route through steer queue
                                            // so the message is injected at the next tool
                                            // boundary instead of launching a concurrent request.
                                            // Note: if isLoading flips false between the check
                                            // and this call, steerMessage handles it gracefully
                                            // by falling back to sendMessage internally.
                                            viewModel.steerMessage(draft)
                                        } else {
                                            viewModel.sendMessage(draft)
                                        }
                                        queuedMessageDraft = ""
                                        viewModel.setQueuedMessage("")
                                        isUndoStopVisible = false
                                    }
                                },
                                isUndoStopVisible = isUndoStopVisible,
                                onResumeOrRetry = {
                                    if (viewModel != null) {
                                        viewModel.resumeExecution()
                                    } else {
                                        runState = OverlayRunState.EXECUTING
                                    }
                                    isUndoStopVisible = false
                                },
                                onStopAction = stopAction,
                                onOpenFull = { if (viewModel != null) onNavigateToChat?.invoke() else surfaceMode = OverlaySurfaceMode.FULL_APP },
                                onOpenBubble = { surfaceMode = OverlaySurfaceMode.BUBBLE }
                            )
                        }

                        if (surfaceMode == OverlaySurfaceMode.BUBBLE) {
                            BubbleOverlay(
                                modifier = Modifier
                                    .align(Alignment.BottomEnd)
                                    .padding(end = 12.dp, bottom = 14.dp),
                                flavor = flavor,
                                runState = activeRunState,
                                unreadCount = unreadCount,
                                isQuickActionsOpen = showBubbleQuickActions,
                                onToggleQuickActions = { showBubbleQuickActions = !showBubbleQuickActions },
                                onExpand = {
                                    surfaceMode = OverlaySurfaceMode.MINI_CHAT
                                    showBubbleQuickActions = false
                                },
                                onStopAction = {
                                    stopAction()
                                    showBubbleQuickActions = false
                                },
                                onDismissQuickActions = { showBubbleQuickActions = false },
                                onDismissOverlay = {
                                    if (isDismissingOverlay) return@BubbleOverlay
                                    showBubbleQuickActions = false
                                    if (viewModel != null) {
                                        onOverlayMinimized?.invoke()
                                    } else {
                                        isDismissingOverlay = true
                                        coroutineScope.launch {
                                            runState = OverlayRunState.COMPLETED
                                            delay(OverlayUiConstants.DISMISS_ANIMATION_DELAY_MS)
                                            surfaceMode = OverlaySurfaceMode.FULL_APP
                                            isDismissingOverlay = false
                                        }
                                    }
                                }
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun OverlayModeChip(
    label: String,
    selected: Boolean,
    accent: Color,
    onClick: () -> Unit
) {
    Surface(
        shape = RoundedCornerShape(OverlayUiConstants.PillCornerRadius),
        color = if (selected) accent else MaterialTheme.colorScheme.surfaceVariant,
        modifier = Modifier
            .clip(RoundedCornerShape(OverlayUiConstants.PillCornerRadius))
            .semantics {
                contentDescription = "$label mode ${if (selected) "selected" else "not selected"}"
            }
            .clickable(onClick = onClick)
    ) {
        Text(
            text = label,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
            style = MaterialTheme.typography.labelMedium,
            color = if (selected) Color.White else MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}

@Composable
private fun OverlayStateChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    tint: Color,
    modifier: Modifier = Modifier
) {
    Surface(
        modifier = modifier
            .clip(RoundedCornerShape(OverlayUiConstants.ModeChipCornerRadius))
            .semantics {
                contentDescription = "$label state ${if (selected) "selected" else "not selected"}"
            }
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(OverlayUiConstants.ModeChipCornerRadius),
        color = if (selected) tint.copy(alpha = 0.2f) else MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, if (selected) tint.copy(alpha = 0.65f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.4f))
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 6.dp),
            contentAlignment = Alignment.Center
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelSmall,
                color = if (selected) tint else MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

private fun overlayRunStateColor(runState: OverlayRunState): Color = when (runState) {
    OverlayRunState.IDLE -> Color.Unspecified
    OverlayRunState.EXECUTING, OverlayRunState.COMPLETED -> OverlayColors.Success
    OverlayRunState.FAILED, OverlayRunState.STOPPED -> OverlayColors.Error
}

@Composable
private fun OverlayRunStateDot(runState: OverlayRunState) {
    Box(
        modifier = Modifier
            .size(OverlayUiConstants.HeaderStatusDotSize)
            .clip(CircleShape)
            .background(overlayRunStateColor(runState))
    )
}

@Composable
internal fun FullAppOverlayContent(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    currentStep: OverlayStep,
    lines: List<OverlayLine>,
    messageDraft: String,
    onMessageDraftChange: (String) -> Unit,
    onSendMessage: () -> Unit,
    onReturnToOverlay: () -> Unit,
    onStopAction: () -> Unit
) {
    val fullScrollState = rememberScrollState()

    // Auto-scroll to bottom when lines change
    LaunchedEffect(lines.size) {
        kotlinx.coroutines.yield()
        fullScrollState.animateScrollTo(fullScrollState.maxValue)
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(OverlayColors.AppChrome)
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                "Citros",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f)
            )
            TextButton(onClick = onReturnToOverlay, contentPadding = OverlayUiConstants.StandardChipPadding) {
                Text("Overlay", style = MaterialTheme.typography.labelMedium)
            }
        }

        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 10.dp, vertical = 8.dp),
            shape = RoundedCornerShape(OverlayUiConstants.StandardCardCornerRadius),
            color = flavor.primary.copy(alpha = 0.12f),
            border = BorderStroke(1.dp, flavor.primary.copy(alpha = 0.4f))
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 9.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                OverlayRunStateDot(runState = runState)
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = when (runState) {
                            OverlayRunState.IDLE -> "Ready"
                            OverlayRunState.EXECUTING -> "Citros is executing actions"
                            OverlayRunState.COMPLETED -> "Action completed"
                            OverlayRunState.FAILED -> "Action failed"
                            OverlayRunState.STOPPED -> "Action stopped"
                        },
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                    Text(
                        text = "${currentStep.label} - Step ${currentStep.step}/${currentStep.total}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
                    )
                }
                Button(
                    onClick = onReturnToOverlay,
                    colors = ButtonDefaults.buttonColors(containerColor = flavor.primary),
                    contentPadding = OverlayUiConstants.ActionChipPadding,
                    modifier = Modifier.semantics { contentDescription = "Return to overlay" }
                ) {
                    Text("Return", style = MaterialTheme.typography.labelSmall, color = Color.White)
                }
                OutlinedButton(
                    onClick = onStopAction,
                    contentPadding = OverlayUiConstants.ActionChipPadding,
                    modifier = Modifier.semantics { contentDescription = "Stop current action" }
                ) {
                    Text("Stop", style = MaterialTheme.typography.labelSmall, color = OverlayColors.Error)
                }
            }
        }

        Column(
            modifier = Modifier
                .weight(1f)
                .verticalScroll(fullScrollState)
                .padding(horizontal = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Surface(
                shape = RoundedCornerShape(OverlayUiConstants.StandardCardCornerRadius),
                color = MaterialTheme.colorScheme.surface.copy(alpha = 0.96f),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.3f))
            ) {
                Text(
                    text = "On it. I will open Settings and turn on Wi-Fi for you.",
                    modifier = Modifier.padding(12.dp),
                    style = MaterialTheme.typography.bodyMedium
                )
            }
            lines.filter { it.type == OverlayLineType.SYSTEM }.take(3).forEach { line ->
                Surface(
                    shape = RoundedCornerShape(OverlayUiConstants.StandardCardCornerRadius),
                    color = MaterialTheme.colorScheme.surface.copy(alpha = 0.92f),
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.3f))
                ) {
                    Text(
                        text = line.text,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.8f)
                    )
                }
            }
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            TextField(
                value = messageDraft,
                onValueChange = onMessageDraftChange,
                modifier = Modifier
                    .weight(1f)
                    .onFocusChanged { focusState ->
                        if (focusState.isFocused) {
                            OverlayService.instance?.moveOverlayToTop()
                        } else {
                            OverlayService.instance?.moveOverlayToBottom()
                        }
                    }
                    .semantics { contentDescription = "Message input" },
                placeholder = { Text("Message Citros...") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = { onSendMessage() })
            )
            IconButton(
                onClick = onSendMessage,
                enabled = messageDraft.trim().isNotBlank()
            ) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.Send,
                    contentDescription = "Send message",
                    tint = if (messageDraft.trim().isNotBlank()) flavor.primary else Color.Gray
                )
            }
        }
    }
}

@Composable
private fun FakeUnderlyingPhoneSurface() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(OverlayColors.FakePhoneBase)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(OverlayColors.FakePhoneBar)
                .padding(horizontal = 12.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            Text("9:41", style = MaterialTheme.typography.labelSmall, color = OverlayColors.FakePhoneTextMuted)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("NET", style = MaterialTheme.typography.labelSmall, color = OverlayColors.FakePhoneTextMuted)
                Text("BAT", style = MaterialTheme.typography.labelSmall, color = OverlayColors.FakePhoneTextMuted)
            }
        }

        Text(
            text = "Settings",
            modifier = Modifier
                .fillMaxWidth()
                .background(OverlayColors.FakePhoneBar)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            style = MaterialTheme.typography.titleMedium,
            color = OverlayColors.FakePhoneTextBright,
            fontWeight = FontWeight.Medium
        )

        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(OverlayColors.FakePhoneSurface)
                .padding(horizontal = 16.dp, vertical = 8.dp)
        ) {
            listOf(
                "Network & internet",
                "Connected devices",
                "Apps",
                "Notifications",
                "Battery",
                "Storage",
                "Sound & vibration",
                "Display"
            ).forEachIndexed { index, item ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 9.dp)
                        .border(width = 0.5.dp, color = OverlayColors.FakePhoneBorder, shape = RoundedCornerShape(OverlayUiConstants.PhoneItemCornerRadius))
                        .padding(horizontal = 8.dp, vertical = 8.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = item,
                        style = MaterialTheme.typography.bodySmall,
                        color = if (index == 0) OverlayColors.FakePhoneAccent else OverlayColors.FakePhoneTextDim
                    )
                    Text(">", style = MaterialTheme.typography.bodySmall, color = OverlayColors.FakePhoneChevron)
                }
            }
        }
    }
}

@Composable
internal fun MiniChatOverlayCard(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    currentStep: OverlayStep,
    lines: List<OverlayLine>,
    queuedMessageDraft: String,
    onQueuedDraftChange: (String) -> Unit,
    onSubmitQueuedMessage: () -> Unit,
    isUndoStopVisible: Boolean,
    onResumeOrRetry: () -> Unit,
    onStopAction: () -> Unit,
    onOpenFull: () -> Unit,
    onOpenBubble: () -> Unit,
    modifier: Modifier = Modifier
) {
    val scrollState = rememberScrollState()

    // Auto-scroll to bottom when lines change
    LaunchedEffect(lines.size) {
        // Brief yield to let Compose lay out the new content before scrolling
        kotlinx.coroutines.yield()
        scrollState.animateScrollTo(scrollState.maxValue)
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
                        else -> overlayRunStateColor(runState)
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
                            OverlayLineType.USER -> Text(">", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            OverlayLineType.SYSTEM -> Text("-", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
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
                        Text(
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
                    // Preview status chip (read-only in demo mode).
                    AssistChip(
                        onClick = {},
                        label = { Text("Step ${currentStep.step} of ${currentStep.total}") }
                    )
                }

                // Inline error card removed — Failed state is handled by the
                // contextual banner with a functional Retry button below (#473).
            }

            // Contextual banner: Stopped → Resume, Failed → Retry (#473)
            if (isUndoStopVisible) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "Stopped",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f),
                        modifier = Modifier.weight(1f)
                    )
                    OutlinedButton(
                        onClick = onResumeOrRetry,
                        contentPadding = OverlayUiConstants.CompactActionPadding
                    ) {
                        Text("Resume", style = MaterialTheme.typography.labelSmall, color = flavor.primary)
                    }
                }
            } else if (runState == OverlayRunState.FAILED) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "Failed",
                        style = MaterialTheme.typography.bodySmall,
                        color = OverlayColors.Error.copy(alpha = 0.72f),
                        modifier = Modifier.weight(1f)
                    )
                    OutlinedButton(
                        onClick = onResumeOrRetry,
                        contentPadding = OverlayUiConstants.CompactActionPadding
                    ) {
                        Text("Retry", style = MaterialTheme.typography.labelSmall, color = OverlayColors.Error)
                    }
                }
            }

            // Input row — always visible (#473)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                TextField(
                    value = queuedMessageDraft,
                    onValueChange = onQueuedDraftChange,
                    modifier = Modifier
                        .weight(1f)
                        .onFocusChanged { focusState ->
                            if (focusState.isFocused) {
                                OverlayService.instance?.moveOverlayToTop()
                            } else {
                                OverlayService.instance?.moveOverlayToBottom()
                            }
                        }
                        .semantics { contentDescription = "Message input" },
                    singleLine = true,
                    placeholder = {
                        Text(
                            if (runState == OverlayRunState.EXECUTING) "Steer or queue..."
                            else "Message..."
                        )
                    },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                    keyboardActions = KeyboardActions(onSend = { onSubmitQueuedMessage() }),
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
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = flavor.primary),
                    enabled = queuedMessageDraft.trim().isNotBlank()
                ) { Text("Send", style = MaterialTheme.typography.labelSmall, color = flavor.primary) }

                if (runState == OverlayRunState.EXECUTING) {
                    Button(
                        onClick = onStopAction,
                        colors = ButtonDefaults.buttonColors(containerColor = OverlayColors.Error),
                        contentPadding = OverlayUiConstants.PrimaryActionPadding
                    ) {
                        Text("Stop", style = MaterialTheme.typography.labelSmall, color = Color.White)
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BubbleOverlay(
    flavor: CitrosFlavor,
    runState: OverlayRunState,
    unreadCount: Int,
    isQuickActionsOpen: Boolean,
    onToggleQuickActions: () -> Unit,
    onExpand: () -> Unit,
    onStopAction: () -> Unit,
    onDismissQuickActions: () -> Unit,
    onDismissOverlay: () -> Unit,
    modifier: Modifier = Modifier
) {
    Box(modifier = modifier) {
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
                        onClick = onStopAction,
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Stop action" }
                    ) { Text("Stop Action") }
                    TextButton(
                        onClick = onExpand,
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Expand overlay" }
                    ) { Text("Expand") }
                    TextButton(
                        onClick = onDismissOverlay,
                        modifier = Modifier
                            .fillMaxWidth()
                            .semantics { contentDescription = "Dismiss overlay" }
                    ) { Text("Dismiss Overlay") }
                }
            }
        }

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
                                        onDismissQuickActions()
                                    } else {
                                        onExpand()
                                    }
                                },
                                onLongClick = onToggleQuickActions
                            )
                    )
                }
            }

            if (runState == OverlayRunState.COMPLETED || runState == OverlayRunState.FAILED || unreadCount > 0) {
                Surface(
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .size(OverlayUiConstants.BubbleBadgeSize),
                    shape = CircleShape,
                    color = when {
                        runState == OverlayRunState.COMPLETED -> OverlayColors.Success
                        runState == OverlayRunState.FAILED -> OverlayColors.Error
                        else -> flavor.primary
                    }
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            text = when {
                                runState == OverlayRunState.COMPLETED -> "C"
                                runState == OverlayRunState.FAILED -> "!"
                                else -> unreadCount.toString()
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White
                        )
                    }
                }
            }
        }
    }
}
