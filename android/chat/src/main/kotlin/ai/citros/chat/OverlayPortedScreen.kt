package ai.citros.chat
/**
 * FULL-SCREEN (ported) overlay UI embedded inside ChatActivity.
 *
 * These are IN-APP copies of the overlay composables — NOT the actual floating overlay.
 * The real floating overlay is in OverlayContent.kt (rendered by OverlayService).
 *
 * ⚠️  If you need to change how the floating overlay looks or behaves, edit
 * OverlayContent.kt instead. This file is for the full-screen/in-app experience only.
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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
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
    val Success = CitrosFlavor.LIME.primary
    val Error = CitrosFlavor.BLOOD_ORANGE.primary
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
@OptIn(ExperimentalFoundationApi::class)
@Composable
internal fun OverlayPreviewScreen(
    context: Context,
    onBack: () -> Unit,
    viewModel: ChatViewModel? = null, // Optional: connect to live ChatViewModel
    onOverlayMinimized: (() -> Unit)? = null,
    onNavigateToChat: (() -> Unit)? = null,
    onRequestVoiceInput: (() -> Unit)? = null
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
    var surfaceMode by rememberSaveable { mutableStateOf(OverlaySurfaceMode.DYNAMIC_ISLAND) }
    var runState by rememberSaveable { mutableStateOf(OverlayRunState.EXECUTING) }
    var stepIndex by rememberSaveable { mutableIntStateOf(0) }
    var isUndoStopVisible by rememberSaveable { mutableStateOf(false) }
    var queuedMessageDraft by rememberSaveable { mutableStateOf("") }
    var fullAppMessageDraft by rememberSaveable { mutableStateOf("") }
    val isDarkTheme = LocalCitrosIsDark.current
    val previewBackground = if (isDarkTheme) {
        OverlayColors.PreviewBackground
    } else {
        CitrosColorScheme.surfaceVariant.copy(alpha = 0.44f)
    }
    val unreadCount = viewModel?.unreadCount?.intValue ?: 2
    // Derive overlay state from ChatViewModel if provided, otherwise use demo data
    val liveOverlayState = if (viewModel != null) {
        remember {
            androidx.compose.runtime.derivedStateOf {
                OverlayStateMapper.mapToOverlayState(
                    messages = viewModel.messages.toList(),
                    isLoading = viewModel.isLoading.value,
                    actionPills = viewModel.runtimeActionPills.value
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
    val activeActionPills = liveOverlayState?.actionPills ?: emptyList()
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
        if (viewModel != null && surfaceMode == OverlaySurfaceMode.PANEL) {
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
                    CitrosIconButton(onClick = onBack) {
                        CitrosIcon(CitrosIcons.ArrowBack, contentDescription = "Back")
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
                color = CitrosColorScheme.surface,
                border = BorderStroke(1.dp, CitrosColorScheme.outline.copy(alpha = 0.35f))
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        OverlayModeChip(
                            label = "Search Bar",
                            selected = surfaceMode == OverlaySurfaceMode.SEARCH_BAR,
                            accent = flavor.primary,
                            onClick = {
                                surfaceMode = OverlaySurfaceMode.SEARCH_BAR
                            }
                        )
                        OverlayModeChip(
                            label = "Panel",
                            selected = surfaceMode == OverlaySurfaceMode.PANEL,
                            accent = flavor.primary,
                            onClick = {
                                surfaceMode = OverlaySurfaceMode.PANEL
                            }
                        )
                        OverlayModeChip(
                            label = "Dynamic Island",
                            selected = surfaceMode == OverlaySurfaceMode.DYNAMIC_ISLAND,
                            accent = flavor.primary,
                            onClick = {
                                surfaceMode = OverlaySurfaceMode.DYNAMIC_ISLAND
                            }
                        )
                        OverlayModeChip(
                            label = "Full App",
                            selected = surfaceMode == OverlaySurfaceMode.FULL_APP,
                            accent = flavor.primary,
                            onClick = {
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
                    .background(previewBackground)
                    .border(
                        width = 1.dp,
                        color = CitrosColorScheme.outline.copy(alpha = 0.35f),
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
                                if (viewModel != null) onNavigateToChat?.invoke() else surfaceMode = OverlaySurfaceMode.PANEL
                            },
                            onStopAction = stopAction
                        )
                    }
                    OverlaySurfaceMode.PANEL,
                    OverlaySurfaceMode.SEARCH_BAR,
                    OverlaySurfaceMode.DYNAMIC_ISLAND -> {
                        FakeUnderlyingPhoneSurface()
                        if (surfaceMode == OverlaySurfaceMode.PANEL) {
                            MiniChatOverlayCard(
                                modifier = Modifier
                                    .align(Alignment.BottomCenter)
                                    .padding(horizontal = 10.dp, vertical = 10.dp),
                                flavor = flavor,
                                runState = activeRunState,
                                currentStep = currentStep,
                                lines = activeLines,
                                actionPills = activeActionPills,
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
                                onOpenIsland = { surfaceMode = OverlaySurfaceMode.DYNAMIC_ISLAND },
                                onMinimize = { surfaceMode = OverlaySurfaceMode.SEARCH_BAR },
                                onActionPillTap = { pill ->
                                    viewModel?.onRuntimePillTapped(pill.action)
                                },
                                onVoiceInput = {
                                    if (viewModel != null) {
                                        onRequestVoiceInput?.invoke() ?: onNavigateToChat?.invoke()
                                    }
                                }
                            )
                        }
                        if (surfaceMode == OverlaySurfaceMode.SEARCH_BAR) {
                            val latestSystemLine = activeLines.lastOrNull { it.type == OverlayLineType.SYSTEM }
                                ?.text
                                ?.removePrefix("💥")
                                ?.removePrefix("Error:")
                                ?.trim()
                                .orEmpty()
                            val statusText = when (activeRunState) {
                                OverlayRunState.EXECUTING -> currentStep.label
                                OverlayRunState.COMPLETED,
                                OverlayRunState.FAILED,
                                OverlayRunState.STOPPED -> latestSystemLine.ifBlank { currentStep.label }
                                OverlayRunState.IDLE -> ""
                            }
                            OverlaySearchBarContent(
                                modifier = Modifier
                                    .align(Alignment.BottomCenter)
                                    .padding(horizontal = OverlayUiConstants.SearchBarHorizontalMargin, vertical = 12.dp),
                                flavor = flavor,
                                runState = activeRunState,
                                statusLabel = statusText,
                                unreadCount = unreadCount,
                                onExpand = {
                                    surfaceMode = OverlaySurfaceMode.PANEL
                                },
                                onStopAction = {
                                    stopAction()
                                }
                            )
                        }
                        if (surfaceMode == OverlaySurfaceMode.DYNAMIC_ISLAND) {
                            OverlayDynamicIslandContent(
                                flavor = flavor,
                                runState = activeRunState,
                                currentStepLabel = currentStep.label,
                                unreadCount = unreadCount,
                                onExpand = { surfaceMode = OverlaySurfaceMode.PANEL },
                                onStopAction = stopAction,
                                onDismiss = {
                                    if (viewModel != null) {
                                        onOverlayMinimized?.invoke()
                                    } else {
                                        surfaceMode = OverlaySurfaceMode.FULL_APP
                                    }
                                },
                                modifier = Modifier
                                    .align(Alignment.TopCenter)
                                    .padding(top = 12.dp)
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
        color = if (selected) accent else CitrosColorScheme.surfaceVariant,
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
            style = CitrosTypography.labelMedium,
            color = if (selected) contrastOn(accent) else CitrosColorScheme.onSurfaceVariant
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
        color = if (selected) tint.copy(alpha = 0.2f) else CitrosColorScheme.surface,
        border = BorderStroke(1.dp, if (selected) tint.copy(alpha = 0.65f) else CitrosColorScheme.outline.copy(alpha = 0.4f))
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 6.dp),
            contentAlignment = Alignment.Center
        ) {
            Text(
                text = label,
                style = CitrosTypography.labelSmall,
                color = if (selected) tint else CitrosColorScheme.onSurfaceVariant
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
    val isDarkTheme = LocalCitrosIsDark.current
    val appChromeColor = if (isDarkTheme) {
        OverlayColors.AppChrome
    } else {
        CitrosColorScheme.surfaceVariant.copy(alpha = 0.84f)
    }
    // Auto-scroll to bottom when lines change or last line content updates.
    val fullLastLineText = lines.lastOrNull()?.text
    LaunchedEffect(lines.size, fullLastLineText) {
        kotlinx.coroutines.yield()
        kotlinx.coroutines.delay(100)
        fullScrollState.animateScrollTo(fullScrollState.maxValue)
        // Second pass: content may still be measuring (e.g. markdown rendering).
        kotlinx.coroutines.delay(300)
        if (fullScrollState.maxValue > fullScrollState.value) {
            fullScrollState.animateScrollTo(fullScrollState.maxValue)
        }
    }
    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(appChromeColor)
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                "Citros",
                style = CitrosTypography.titleMedium,
                color = contrastOn(appChromeColor),
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f)
            )
            TextButton(onClick = onReturnToOverlay, contentPadding = OverlayUiConstants.StandardChipPadding) {
                Text("Overlay", style = CitrosTypography.labelMedium)
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
                        style = CitrosTypography.labelLarge,
                        color = CitrosColorScheme.onSurface
                    )
                    Text(
                        text = "${currentStep.label} - Step ${currentStep.step}/${currentStep.total}",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onSurface.copy(alpha = 0.72f)
                    )
                }
                Button(
                    onClick = onReturnToOverlay,
                    colors = ButtonDefaults.buttonColors(containerColor = flavor.primary),
                    contentPadding = OverlayUiConstants.ActionChipPadding,
                    modifier = Modifier.semantics { contentDescription = "Return to overlay" }
                ) {
                    Text("Return", style = CitrosTypography.labelSmall, color = contrastOn(flavor.primary))
                }
                OutlinedButton(
                    onClick = onStopAction,
                    contentPadding = OverlayUiConstants.ActionChipPadding,
                    modifier = Modifier.semantics { contentDescription = "Stop current action" }
                ) {
                    Text("Stop", style = CitrosTypography.labelSmall, color = OverlayColors.Error)
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
                color = CitrosColorScheme.surface.copy(alpha = 0.96f),
                border = BorderStroke(1.dp, CitrosColorScheme.outline.copy(alpha = 0.3f))
            ) {
                Text(
                    text = "On it. I will open Settings and turn on Wi-Fi for you.",
                    modifier = Modifier.padding(12.dp),
                    style = CitrosTypography.bodyMedium
                )
            }
            lines.filter { it.type == OverlayLineType.SYSTEM }.take(3).forEach { line ->
                Surface(
                    shape = RoundedCornerShape(OverlayUiConstants.StandardCardCornerRadius),
                    color = CitrosColorScheme.surface.copy(alpha = 0.92f),
                    border = BorderStroke(1.dp, CitrosColorScheme.outline.copy(alpha = 0.3f))
                ) {
                    MarkdownText(
                        text = line.text,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onSurface.copy(alpha = 0.8f)
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
                contentDescription = "Message input",
                modifier = Modifier
                    .weight(1f)
                    .onFocusChanged { focusState ->
                        if (focusState.isFocused) {
                            OverlayService.instance?.moveOverlayToTop()
                        } else {
                            OverlayService.instance?.moveOverlayToBottom()
                        }
                    },
                placeholder = { Text("Message Citros...") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = { onSendMessage() })
            )
            CitrosIconButton(
                onClick = onSendMessage,
                enabled = messageDraft.trim().isNotBlank()
            ) {
                CitrosIcon(
                    imageVector = CitrosIcons.Send,
                    contentDescription = "Send message",
                    tint = if (messageDraft.trim().isNotBlank()) flavor.primary else Color.Gray
                )
            }
        }
    }
}
@Composable
private fun FakeUnderlyingPhoneSurface() {
    val isDarkTheme = LocalCitrosIsDark.current
    val baseColor = if (isDarkTheme) OverlayColors.FakePhoneBase else CitrosColorScheme.surface
    val barColor = if (isDarkTheme) OverlayColors.FakePhoneBar else CitrosColorScheme.surfaceVariant
    val surfaceColor = if (isDarkTheme) OverlayColors.FakePhoneSurface else CitrosColorScheme.background
    val borderColor = if (isDarkTheme) OverlayColors.FakePhoneBorder else CitrosColorScheme.outline.copy(alpha = 0.24f)
    val mutedTextColor = if (isDarkTheme) OverlayColors.FakePhoneTextMuted else CitrosColorScheme.onSurface.copy(alpha = 0.58f)
    val brightTextColor = if (isDarkTheme) OverlayColors.FakePhoneTextBright else CitrosColorScheme.onSurface
    val dimTextColor = if (isDarkTheme) OverlayColors.FakePhoneTextDim else CitrosColorScheme.onSurface.copy(alpha = 0.74f)
    val accentColor = if (isDarkTheme) OverlayColors.FakePhoneAccent else CitrosColorScheme.primary
    val chevronColor = if (isDarkTheme) OverlayColors.FakePhoneChevron else CitrosColorScheme.onSurfaceVariant
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(baseColor)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(barColor)
                .padding(horizontal = 12.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            Text("9:41", style = CitrosTypography.labelSmall, color = mutedTextColor)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("NET", style = CitrosTypography.labelSmall, color = mutedTextColor)
                Text("BAT", style = CitrosTypography.labelSmall, color = mutedTextColor)
            }
        }
        Text(
            text = "Settings",
            modifier = Modifier
                .fillMaxWidth()
                .background(barColor)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            style = CitrosTypography.titleMedium,
            color = brightTextColor,
            fontWeight = FontWeight.Medium
        )
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(surfaceColor)
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
                        .border(width = 0.5.dp, color = borderColor, shape = RoundedCornerShape(OverlayUiConstants.PhoneItemCornerRadius))
                        .padding(horizontal = 8.dp, vertical = 8.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = item,
                        style = CitrosTypography.bodySmall,
                        color = if (index == 0) accentColor else dimTextColor
                    )
                    Text(">", style = CitrosTypography.bodySmall, color = chevronColor)
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
    actionPills: List<ActionPill> = emptyList(),
    queuedMessageDraft: String,
    onQueuedDraftChange: (String) -> Unit,
    onSubmitQueuedMessage: () -> Unit,
    isUndoStopVisible: Boolean,
    onResumeOrRetry: () -> Unit,
    onStopAction: () -> Unit,
    onOpenFull: () -> Unit,
    onOpenIsland: () -> Unit,
    onActionPillTap: (ActionPill) -> Unit = {},
    onVoiceInput: () -> Unit = {},
    onMinimize: () -> Unit = {},
    modifier: Modifier = Modifier
) {
    val scrollState = rememberScrollState()
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    val panelColor = if (isDarkTheme) {
        Color(0xEB1C1C1E)
    } else {
        Color(0xEBF2F2F7)
    }
    val statusColor = when (runState) {
        OverlayRunState.EXECUTING, OverlayRunState.COMPLETED -> OverlayColors.Success
        OverlayRunState.FAILED, OverlayRunState.STOPPED -> OverlayColors.Error
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
    // The delay ensures Compose has laid out new content (especially after
    // overlay activation, when the view may not have measured yet).
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
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = OverlayUiConstants.MiniChatMaxHeight),
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
                    OverlayLineBubble(
                        line = line,
                        flavor = flavor,
                        surfaces = surfaces,
                        flavorTokens = flavorTokens
                    )
                }
                if (runState == OverlayRunState.EXECUTING) {
                    // Preview status chip (read-only in demo mode).
                    AssistChip(
                        onClick = {},
                        label = { Text("Step ${currentStep.step} of ${currentStep.total}") },
                        colors = AssistChipDefaults.assistChipColors(
                            containerColor = surfaces.surface2,
                            labelColor = surfaces.labelSecondary
                        )
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
                        modifier = Modifier.clickable(onClick = onResumeOrRetry)
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
                        color = OverlayColors.Error.copy(alpha = 0.72f),
                        modifier = Modifier.weight(1f)
                    )
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = surfaces.surface2,
                        border = BorderStroke(1.dp, surfaces.separatorLight),
                        modifier = Modifier.clickable(onClick = onResumeOrRetry)
                    ) {
                        Text(
                            "Retry",
                            style = CitrosTypography.labelSmall,
                            color = OverlayColors.Error,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp)
                        )
                    }
                }
            }
            if (actionPills.isNotEmpty()) {
                RuntimeActionPillRow(
                    pills = actionPills,
                    onPillTapped = onActionPillTap,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp)
                )
            }
            // Input row — always visible (#473)
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
                    modifier = Modifier
                        .weight(1f),
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
                            contentDescription = "Message input",
                            modifier = Modifier
                                .weight(1f)
                                .heightIn(max = 132.dp)
                                .onFocusChanged { focusState ->
                                    if (focusState.isFocused) {
                                        OverlayService.instance?.moveOverlayToTop()
                                    } else {
                                        OverlayService.instance?.moveOverlayToBottom()
                                    }
                                },
                            placeholder = {
                                Text(
                                    text = if (runState == OverlayRunState.EXECUTING) "Steer or queue..." else "Message",
                                    style = CitrosTypography.bodyLarge
                                )
                            },
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                            keyboardActions = KeyboardActions(onSend = { onSubmitQueuedMessage() }),
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
                                    onClick = {
                                        if (hasInputText) {
                                            onQueuedDraftChange("")
                                        } else {
                                            onVoiceInput()
                                        }
                                    }
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
                            showStopButton -> onStopAction()
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
