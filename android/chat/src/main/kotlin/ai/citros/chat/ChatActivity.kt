package ai.citros.chat
import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import ai.citros.core.ModelManager
import ai.citros.core.SherpaOnnxSpeechToText
import ai.citros.core.AndroidSpeechToText
import ai.citros.core.AndroidTextToSpeech
import ai.citros.core.SherpaOnnxTextToSpeech
import ai.citros.core.VoiceManager
import ai.citros.core.SpeechEvent
import ai.citros.core.SpeechToTextProvider
import ai.citros.core.SpeechError
import ai.citros.core.VoiceAccumulator
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import androidx.browser.customtabs.CustomTabsIntent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.Icon
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.annotation.VisibleForTesting
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import ai.citros.core.AgentFileManager
import ai.citros.core.AnthropicClient
import ai.citros.core.ClaudeClient
import ai.citros.core.CodexOauthBridgeClient
import ai.citros.core.EmbeddedCodexOauthBridgeServer
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.ProviderConfig
import ai.citros.core.PhoneAgentPrompts
import ai.citros.core.ScreenReader
import ai.citros.core.SensorProvider
import ai.citros.core.WalletManager
import ai.citros.core.WalletState
import ai.citros.core.ModelConfig
import ai.citros.core.Conversation
import ai.citros.core.OpenAiClient
import ai.citros.core.OpenRouterClient
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayState
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import androidx.compose.runtime.collectAsState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.util.UUID
import kotlin.math.cos
import kotlin.math.sin
class ChatActivity : ComponentActivity() {
    private val walletDependencies by lazy { provideWalletDependencies(this) }
    internal var memoryDb: android.database.sqlite.SQLiteDatabase? = null
    private val overlayVoiceStartRequests = MutableStateFlow(0)
    override fun onDestroy() {
        OverlayController.setChatInForeground(false)
        // Clear hooks to avoid leaking this Activity (#436, #457)
        ScreenReader.toolLoopOverlayHideHook = null
        ScreenReader.toolLoopOverlayRestoreHook = null
        ScreenReader.screenshotOverlayHook = null
        ScreenReader.screenshotOverlayRestoreHook = null
        memoryDb?.close()
        memoryDb = null
        super.onDestroy()
    }
    // Using onPause/onResume rather than onStop/onStart for overlay suppression (#627):
    // onPause fires faster when the user switches away, providing snappier overlay restore.
    // Trade-off: in multi-window or transparent-activity scenarios, onPause fires while
    // ChatActivity is still partially visible — acceptable since those are rare edge cases.
    override fun onPause() {
        super.onPause()
        OverlayController.setChatInForeground(false)
    }
    override fun onResume() {
        super.onResume()
        OverlayController.setChatInForeground(true)
        val prefs = getSharedPreferences(CITROS_PREFS, MODE_PRIVATE)
        val timeoutMs = prefs.getLong(PREF_IDLE_TIMEOUT_MS, ConversationLifecycle.DEFAULT_TIMEOUT_MS)
        val lastDate = prefs.getString(PREF_LAST_CONVERSATION_DATE, null)
        val chatViewModel = androidx.lifecycle.ViewModelProvider(this)[ChatViewModel::class.java]
        val reason = chatViewModel.checkLifecycleAndClear(
            timeoutMs = timeoutMs,
            lastConversationDate = lastDate
        )
        // Update stored date on any resume so daily reset works next time
        val today = ConversationLifecycle.todayDateString()
        prefs.edit().putString(PREF_LAST_CONVERSATION_DATE, today).apply()
        if (reason != null) {
            android.util.Log.d("CitrosLifecycle", "Conversation cleared: $reason")
        }
    }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleOauthCallbackIntent(intent)
        handleOverlayVoiceInputIntent(intent)
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleOauthCallbackIntent(intent)
        handleOverlayVoiceInputIntent(intent)
        enableEdgeToEdge()
        configureScreenReaderPrivacyList(applicationContext)
        runCatching {
            syncLauncherIconWithPreferences(this)
        }.onFailure { error ->
            Log.w("ChatActivity", "Failed to sync launcher icon", error)
        }
        // Wire overlay hooks so the overlay doesn't interfere with agent actions:
        //
        // - Tool loop hooks (#457): collapse panel/search bar to Dynamic Island
        //   during tool execution so status stays visible while large overlays
        //   are out of the way of target-app gestures.
        //
        // - Screenshot hooks (#436): hide overlay during capture so it doesn't
        //   appear in screenshots.
        //
        // These are separate because screenshot hide/restore can run during the
        // tool loop — the double-hide guard in OverlayService (savedVisibility
        // sentinel) makes nested calls safe.
        //
        // All hooks run on Main dispatcher since they touch View visibility.
        var toolLoopRestoreMode: OverlaySurfaceMode? = null
        var toolLoopRestorePinned: Boolean? = null
        ScreenReader.toolLoopOverlayHideHook = {
            withContext(Dispatchers.Main) {
                if (!OverlayPermission.canDrawOverlays(this@ChatActivity)) {
                    return@withContext
                }
                val wasActive = OverlayController.isOverlayActive.value
                val currentMode = OverlayController.surfaceMode.value
                if (wasActive && currentMode != OverlaySurfaceMode.FULL_APP) {
                    if (currentMode != OverlaySurfaceMode.DYNAMIC_ISLAND) {
                        toolLoopRestoreMode = currentMode
                        toolLoopRestorePinned = OverlayController.userPanelPinned.value
                        OverlayController.updateSurfaceMode(OverlaySurfaceMode.DYNAMIC_ISLAND)
                    }
                } else {
                    // Ensure execution status stays visible even when bar/panel are hidden.
                    toolLoopRestoreMode = OverlayController.preferredIdleSurfaceMode()
                    toolLoopRestorePinned = false
                    OverlayController.updateSurfaceMode(OverlaySurfaceMode.DYNAMIC_ISLAND)
                    OverlayController.activateOverlay()
                }
            }
        }
        ScreenReader.toolLoopOverlayRestoreHook = {
            withContext(Dispatchers.Main) {
                if (OverlayPermission.canDrawOverlays(this@ChatActivity) &&
                    !OverlayController.isOverlayActive.value
                ) {
                    OverlayController.updateSurfaceMode(OverlayController.preferredIdleSurfaceMode())
                    OverlayController.activateOverlay()
                }
                val restoreMode = toolLoopRestoreMode
                val restorePinned = toolLoopRestorePinned
                if (restoreMode != null && OverlayController.isOverlayActive.value) {
                    OverlayController.updateSurfaceMode(
                        restoreMode,
                        fromUser = restoreMode == OverlaySurfaceMode.PANEL && restorePinned == true
                    )
                    if (restoreMode == OverlaySurfaceMode.PANEL && restorePinned == true) {
                        OverlayController.setUserPanelPinned(true)
                    }
                }
                toolLoopRestoreMode = null
                toolLoopRestorePinned = null
                OverlayService.instance?.restoreOverlayVisibility()
            }
        }
        ScreenReader.screenshotOverlayHook = {
            withContext(Dispatchers.Main) {
                OverlayService.instance?.hideOverlayForScreenshot()
            }
        }
        ScreenReader.screenshotOverlayRestoreHook = {
            withContext(Dispatchers.Main) {
                OverlayService.instance?.restoreOverlayVisibility()
            }
        }
        setContent {
            val overlayVoiceStartToken by overlayVoiceStartRequests.collectAsState()
            val onboardingPrefs = remember {
                getSharedPreferences(ONBOARDING_PREFS, MODE_PRIVATE)
            }
            val chatPrefs = remember {
                getSharedPreferences(CITROS_PREFS, MODE_PRIVATE)
            }
            var themeMode by remember {
                mutableStateOf(
                    onboardingPrefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT
                )
            }
            var selectedFlavor by remember {
                mutableStateOf(
                    runCatching { readSelectedFlavor(this@ChatActivity) }
                        .getOrDefault(CitrosFlavor.TANGERINE)
                )
            }
            DisposableEffect(onboardingPrefs) {
                val listener = android.content.SharedPreferences.OnSharedPreferenceChangeListener { prefs, key ->
                    when (key) {
                        PREF_THEME_MODE -> {
                            themeMode = prefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT
                        }
                        PREF_SELECTED_FLAVOR -> {
                            selectedFlavor = CitrosFlavor.fromStorage(
                                prefs.getString(PREF_SELECTED_FLAVOR, CitrosFlavor.TANGERINE.storageValue)
                            )
                        }
                    }
                }
                onboardingPrefs.registerOnSharedPreferenceChangeListener(listener)
                onDispose {
                    onboardingPrefs.unregisterOnSharedPreferenceChangeListener(listener)
                }
            }
            DisposableEffect(chatPrefs) {
                OverlayController.updateIdleSurfacePreference(
                    chatPrefs.getBoolean(
                        PREF_OVERLAY_USE_ISLAND_WHEN_IDLE,
                        PREF_OVERLAY_USE_ISLAND_WHEN_IDLE_DEFAULT
                    )
                )
                OverlayController.updateSearchBarIdlePreference(
                    chatPrefs.getBoolean(
                        PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE,
                        PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE_DEFAULT
                    )
                )
                val listener = android.content.SharedPreferences.OnSharedPreferenceChangeListener { prefs, key ->
                    if (key == PREF_OVERLAY_USE_ISLAND_WHEN_IDLE) {
                        OverlayController.updateIdleSurfacePreference(
                            prefs.getBoolean(
                                PREF_OVERLAY_USE_ISLAND_WHEN_IDLE,
                                PREF_OVERLAY_USE_ISLAND_WHEN_IDLE_DEFAULT
                            )
                        )
                    } else if (key == PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE) {
                        OverlayController.updateSearchBarIdlePreference(
                            prefs.getBoolean(
                                PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE,
                                PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE_DEFAULT
                            )
                        )
                    }
                }
                chatPrefs.registerOnSharedPreferenceChangeListener(listener)
                onDispose {
                    chatPrefs.unregisterOnSharedPreferenceChangeListener(listener)
                }
            }
            CitrosChatTheme(themeMode = themeMode, flavor = selectedFlavor) {
                CompositionLocalProvider(LocalWalletDependencies provides walletDependencies) {
                    ChatNavHost(
                        walletDependencies = walletDependencies,
                        overlayVoiceStartToken = overlayVoiceStartToken,
                        onOverlayVoiceInputRequest = {
                            overlayVoiceStartRequests.value = overlayVoiceStartRequests.value + 1
                        }
                    )
                }
            }
        }
    }
    private fun handleOauthCallbackIntent(intent: Intent?) {
        val uri = intent?.data ?: return
        if (
            uri.scheme == OAUTH_CALLBACK_SCHEME &&
            uri.host == OAUTH_CALLBACK_HOST &&
            uri.path == OAUTH_CALLBACK_PATH
        ) {
            oauthCallbackState.value = uri
        }
    }

    private fun handleOverlayVoiceInputIntent(intent: Intent?) {
        if (intent?.getBooleanExtra(EXTRA_START_VOICE_INPUT, false) == true) {
            Log.d("ChatActivity", "Received overlay voice input request intent")
            overlayVoiceStartRequests.value = overlayVoiceStartRequests.value + 1
            intent.removeExtra(EXTRA_START_VOICE_INPUT)
        }
    }
    companion object {
        const val OAUTH_CALLBACK_SCHEME = "citros"
        const val OAUTH_CALLBACK_HOST = "oauth"
        const val OAUTH_CALLBACK_PATH = "/callback"
        const val OAUTH_CALLBACK_URI = "$OAUTH_CALLBACK_SCHEME://$OAUTH_CALLBACK_HOST$OAUTH_CALLBACK_PATH"
        private val oauthCallbackState = MutableStateFlow<Uri?>(null)
        fun oauthCallbackFlow() = oauthCallbackState.asStateFlow()
        fun clearOauthCallback() {
            oauthCallbackState.value = null
        }

        internal fun configureScreenReaderPrivacyList(context: Context) {
            val privacyList = SharedPrefsPrivacyList(context.applicationContext)
            privacyList.seedDefaultsIfNeeded()
            ScreenReader.configurePrivacyList(privacyList)
        }
    }
}
@Composable
private fun ChatNavHost(
    walletDependencies: WalletDependencies,
    overlayVoiceStartToken: Int = 0,
    onOverlayVoiceInputRequest: () -> Unit = {}
) {
    val navController = rememberNavController()
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val sharedChatViewModel: ChatViewModel = viewModel()
    val startDestination = remember {
        if (shouldShowOnboarding(context)) "onboarding" else "chat"
    }
    var isAppForeground by remember { mutableStateOf(true) }
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> isAppForeground = true
                Lifecycle.Event.ON_PAUSE,
                Lifecycle.Event.ON_STOP -> isAppForeground = false
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    // ── Voice I/O initialization ──
    // Extract models in background, then create VoiceManager.
    // Voice features are disabled until extraction completes.
    // LaunchedEffect(Unit) is intentional: this one-shot only handles process-death recovery on
    // initial composition entry, not every lifecycle resume/start.
    LaunchedEffect(Unit) {
        val appContext = context.applicationContext
        withContext(Dispatchers.IO) {
            try {
                val modelManager = ModelManager(appContext)
                modelManager.ensureExtracted()
                modelManager.ensureTtsExtracted()
                val sherpaProvider = SherpaOnnxSpeechToText(modelManager.modelDir)
                sherpaProvider.initialize(appContext)
                val androidStt = AndroidSpeechToText()
                androidStt.initialize(appContext)
                val sherpaTts = SherpaOnnxTextToSpeech(modelManager.ttsModelDir)
                sherpaTts.initialize(appContext)
                val androidTts = AndroidTextToSpeech()
                androidTts.initialize(appContext)
                val voiceManager = VoiceManager(
                    context = appContext,
                    sttProviders = listOf(sherpaProvider, androidStt),
                    ttsProviders = listOf(sherpaTts, androidTts)
                )
                withContext(Dispatchers.Main) {
                    sharedChatViewModel.setVoiceManager(voiceManager)
                }
            } catch (e: Exception) {
                // Model extraction failed — fall back to Android STT only
                android.util.Log.w("VoiceInit", "Sherpa model extraction failed, falling back to Android STT", e)
                try {
                    val androidStt = AndroidSpeechToText()
                    androidStt.initialize(appContext)
                    val androidTts = AndroidTextToSpeech()
                    androidTts.initialize(appContext)
                    val voiceManager = VoiceManager(
                        context = appContext,
                        sttProviders = listOf(androidStt),
                        ttsProviders = listOf(androidTts)
                    )
                    withContext(Dispatchers.Main) {
                        sharedChatViewModel.setVoiceManager(voiceManager)
                    }
                } catch (fallbackError: Exception) {
                    android.util.Log.e("VoiceInit", "Voice initialization failed entirely", fallbackError)
                }
            }
        }
    }
    // Overlay action mediator stays active across all destinations (chat/settings/overlay).
    LaunchedEffect(Unit) {
        OverlayController.actions.collect { action ->
            when (action) {
                is OverlayAction.QueueMessage -> {
                    // Route through steerMessage which checks isLoading internally
                    // and falls back to sendMessage when idle (#603).
                    sharedChatViewModel.steerMessage(action.text)
                }
                is OverlayAction.StopExecution -> {
                    sharedChatViewModel.cancelToolExecution()
                }
                is OverlayAction.ResumeExecution -> {
                    sharedChatViewModel.resumeExecution()
                }
                is OverlayAction.SetSurfaceMode -> {
                    OverlayController.updateSurfaceMode(action.mode, fromUser = true)
                }
                is OverlayAction.Deactivate -> {
                    OverlayController.deactivateOverlay()
                }
                is OverlayAction.ExpandFromSearchBar -> {
                    OverlayController.updateSurfaceMode(OverlaySurfaceMode.PANEL, fromUser = true)
                    sharedChatViewModel.resetUnreadCount()
                }
            }
        }
    }
    // Keep overlay state synced from ViewModel even when not on chat route.
    LaunchedEffect(
        sharedChatViewModel.isLoading.value,
        sharedChatViewModel.messages.size,
        sharedChatViewModel.currentToolStatus.value,
        sharedChatViewModel.queuedMessage.value,
        isAppForeground
    ) {
        val overlayState = OverlayStateMapper.mapToOverlayState(
            messages = sharedChatViewModel.messages.toList(),
            isLoading = sharedChatViewModel.isLoading.value
        )
        OverlayController.updateOverlayState(overlayState)
        OverlayController.updateUnreadCount(sharedChatViewModel.unreadCount.intValue)
        OverlayController.updateQueuedMessage(sharedChatViewModel.queuedMessage.value)
        OverlayController.updateToolStatus(sharedChatViewModel.currentToolStatus.value)
        OverlayController.updateInteractionDemand(
            deriveOverlayInteractionDemand(
                overlayState = overlayState,
                toolStatus = sharedChatViewModel.currentToolStatus.value
            )
        )
        val isToolExecutionActive = overlayState.runState == OverlayRunState.EXECUTING
            && sharedChatViewModel.currentToolStatus.value != null
        // Keep overlay alive whenever the model is still processing (#730).
        // Without this, open_app clears currentToolStatus on result, causing
        // the overlay to deactivate while the model generates its response.
        val isModelBusy = sharedChatViewModel.isLoading.value &&
            overlayState.runState == OverlayRunState.EXECUTING
        val idleSurfaceMode = OverlayController.preferredIdleSurfaceMode()
        val backgroundSurfaceMode = deriveBackgroundSurfaceMode(
            isToolExecutionActive = isToolExecutionActive,
            isModelBusy = isModelBusy,
            idleSurfaceMode = idleSurfaceMode
        )
        val shouldShowOverlayInBackground = backgroundSurfaceMode != OverlaySurfaceMode.FULL_APP
        if (!isAppForeground && OverlayPermission.canDrawOverlays(context) && shouldShowOverlayInBackground) {
            val currentMode = OverlayController.surfaceMode.value
            if (!OverlayController.isOverlayActive.value || currentMode == OverlaySurfaceMode.FULL_APP) {
                OverlayController.updateSurfaceMode(backgroundSurfaceMode)
            } else if (isToolExecutionActive && currentMode != OverlaySurfaceMode.DYNAMIC_ISLAND) {
                OverlayController.updateSurfaceMode(OverlaySurfaceMode.DYNAMIC_ISLAND)
            }
            OverlayController.activateOverlay()
        } else if (!isAppForeground &&
            !shouldShowOverlayInBackground &&
            OverlayController.isOverlayActive.value &&
            !isToolExecutionActive
        ) {
            OverlayController.deactivateOverlay()
        }
    }
    // Overlay service lifecycle should not depend on which screen is visible.
    val appContext = context.applicationContext
    val isOverlayActive by OverlayController.isOverlayActive.collectAsState()
    var serviceStarted by remember { mutableStateOf(false) }
    LaunchedEffect(isOverlayActive, isAppForeground) {
        if (!isAppForeground && isOverlayActive && OverlayPermission.canDrawOverlays(context) && !serviceStarted) {
            appContext.startForegroundService(
                OverlayService.startIntent(appContext)
            )
            serviceStarted = true
        } else {
            appContext.stopService(OverlayService.stopIntent(appContext))
            serviceStarted = false
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            val isExecuting = OverlayController.overlayState.value.runState == OverlayRunState.EXECUTING
            val surfaceMode = OverlayController.surfaceMode.value
            val isActive = OverlayController.isOverlayActive.value
            if (isActive && (isExecuting || surfaceMode != OverlaySurfaceMode.FULL_APP)) {
                // Preserve service while active outside the app.
            } else if (isActive) {
                appContext.stopService(OverlayService.stopIntent(appContext))
            }
        }
    }
    LaunchedEffect(overlayVoiceStartToken) {
        if (overlayVoiceStartToken > 0) {
            navController.navigate("chat") { launchSingleTop = true }
        }
    }
    NavHost(navController = navController, startDestination = startDestination) {
        composable("onboarding") {
            OnboardingFlow(
                context = context,
                walletDependencies = walletDependencies,
                onFinished = {
                    navController.navigate("chat") {
                        popUpTo("onboarding") { inclusive = true }
                        launchSingleTop = true
                    }
                }
            )
        }
        composable("chat") {
            ChatScreen(
                viewModel = sharedChatViewModel,
                onOpenSettings = { navController.navigate("settings") },
                onOpenApiKeys = { navController.navigate(ROUTE_SETTINGS_WALLET) },
                onOpenOverlay = { navController.navigate("overlay") { launchSingleTop = true } },
                startVoiceInputToken = overlayVoiceStartToken
            )
        }
        composable("overlay") {
            // Guard: if OverlayService died (e.g. process death), bounce back to chat (#546)
            LaunchedEffect(Unit) {
                if (shouldPopOverlayRoute(OverlayService.instance)) {
                    navController.popBackStack("chat", false)
                }
            }
            OverlayPreviewScreen(
                context = context,
                // Intentional: allow back navigation even while isLoading=true.
                // ViewModel execution continues in background so users never get stuck (#546).
                onBack = { navController.popBackStack() },
                viewModel = sharedChatViewModel,
                onOverlayMinimized = { navController.popBackStack() },
                onNavigateToChat = { navController.popBackStack("chat", false) },
                onRequestVoiceInput = {
                    onOverlayVoiceInputRequest()
                    navController.popBackStack("chat", false)
                }
            )
        }
        composable("settings") {
            SettingsHubScreen(
                context = context,
                walletManager = walletDependencies.walletManager,
                onBack = { navController.popBackStack() },
                onOpenWallet = { navController.navigate(ROUTE_SETTINGS_WALLET) },
                onOpenModels = { navController.navigate("settings_models") },
                onOpenTrust = { navController.navigate("settings_trust") },
                onOpenPhoneControl = { navController.navigate("settings_phone_control") },
                onOpenToolCategories = { navController.navigate("settings_tool_categories") },
                onOpenSound = { navController.navigate("settings_sound") },
                onOpenAppearance = { navController.navigate("settings_appearance") },
                onOpenAbout = { navController.navigate("settings_about") }
            )
        }
        composable(ROUTE_SETTINGS_WALLET) {
            ApiKeysSettingsScreen(
                walletManager = walletDependencies.walletManager,
                keyStore = walletDependencies.keyStore,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_trust") {
            TrustSettingsScreen(
                context = context,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_phone_control") {
            PhoneControlSettingsScreen(
                context = context,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_sound") {
            SoundSettingsScreen(
                voiceManager = sharedChatViewModel.voiceManager.collectAsState().value,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_models") {
            ModelsSettingsScreen(
                walletManager = walletDependencies.walletManager,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_appearance") {
            AppearanceSettingsScreen(
                context = context,
                onBack = { navController.popBackStack() }
            )
        }
        composable("settings_about") {
            AboutSettingsScreen(onBack = { navController.popBackStack() })
        }
        composable("settings_tool_categories") {
            ToolCategoriesSettingsScreen(onBack = { navController.popBackStack() })
        }
    }
}
enum class CloudAuthKind {
    ANTHROPIC_CREDENTIAL,
    OPENAI_API_KEY,
    OPENAI_CODEX_OAUTH,
    OPENAI_DEVICE_CODE,
    OPENROUTER_API_KEY
}
enum class CodexOauthBridgeMode {
    EMBEDDED,
    EXTERNAL
}
data class CodexOauthStartRequest(
    val mode: CodexOauthBridgeMode,
    val externalBridgeUrl: String? = null,
    val embeddedConfig: EmbeddedCodexOauthBridgeServer.Config? = null
)
internal const val CITROS_PREFS = "citros"
/** Number of items from the end of the list to consider "near bottom" for auto-scroll. */
private const val NEAR_BOTTOM_THRESHOLD = 3
private const val ROUTE_SETTINGS_WALLET = "settings_wallet"

private const val PREF_CLOUD_TOKEN = "cloud_token"
private const val PREF_CLOUD_PROVIDER = "cloud_provider"
private const val PREF_CLOUD_AUTH_KIND = "cloud_auth_kind"
private const val PREF_LOCAL_URL = "local_url"
private const val PREF_LOCAL_MODEL = "local_model"
private const val PREF_LEGACY_ANTHROPIC_TOKEN = "anthropic_token"
private const val PREF_DEVICE_CODE_REFRESH_TOKEN = "device_code_refresh_token"
private const val PREF_DEVICE_CODE_LAST_STATUS = "device_code_last_status"
private const val PREF_DEVICE_CODE_LAST_ERROR = "device_code_last_error"
private const val PREF_DEVICE_CODE_LAST_DIAGNOSTICS = "device_code_last_diagnostics"
private const val PREF_CODEX_OAUTH_STATE = "codex_oauth_state"
private const val PREF_CODEX_OAUTH_STATE_TIMESTAMP = "codex_oauth_state_ts"
private const val PREF_CODEX_OAUTH_BRIDGE = "codex_oauth_bridge_url"
private const val PREF_CODEX_OAUTH_LOGIN_ID = "codex_oauth_login_id"
private const val PREF_CODEX_OAUTH_CODE_VERIFIER = "codex_oauth_code_verifier"
private const val PREF_CODEX_OAUTH_BRIDGE_MODE = "codex_oauth_bridge_mode"
private const val PREF_CODEX_OAUTH_AUTH_URL = "codex_oauth_auth_url"
private const val PREF_CODEX_OAUTH_TOKEN_URL = "codex_oauth_token_url"
private const val PREF_CODEX_OAUTH_CLIENT_ID = "codex_oauth_client_id"
// Optional for public PKCE clients. Most Codex OAuth flows use PKCE without a client secret.
private const val PREF_CODEX_OAUTH_CLIENT_SECRET = "codex_oauth_client_secret"
private const val PREF_CODEX_OAUTH_SCOPE = "codex_oauth_scope"
private const val PREF_IDLE_TIMEOUT_MS = "idle_timeout_ms"
private const val PREF_LAST_CONVERSATION_DATE = "last_conversation_date"
internal const val PREF_SEARCH_BASE_URL = "search_base_url"
internal const val PREF_BRAVE_API_KEY = "brave_api_key"
internal const val PREF_TINYFISH_API_KEY = "tinyfish_api_key"
internal const val PREF_SENSOR_CONTEXT_ENABLED = "sensor_context_enabled"
internal const val PREF_SENSOR_CONTEXT_ENABLED_DEFAULT = false
internal const val PREF_OVERLAY_USE_ISLAND_WHEN_IDLE = "overlay_use_island_when_idle"
internal const val PREF_OVERLAY_USE_ISLAND_WHEN_IDLE_DEFAULT = true
internal const val PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE = "overlay_show_search_bar_when_idle"
internal const val PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE_DEFAULT = true
internal const val PREF_OVERLAY_DYNAMIC_ISLAND_DEBUG_BADGE = "overlay_dynamic_island_debug_badge"
internal const val PREF_OVERLAY_DYNAMIC_ISLAND_DEBUG_BADGE_DEFAULT = false
internal const val EXTRA_START_VOICE_INPUT = "extra_start_voice_input"
private const val TOKEN_PREVIEW_LIMIT = 80
private const val DIAGNOSTIC_PREVIEW_LIMIT = 60
private const val OAUTH_STATE_EXPIRY_MS = 600_000L // 10 minutes
private const val API_VALIDATION_TIMEOUT_MS = 10_000L

private val OverlayPermissionKeywords = listOf(
    "permission",
    "grant access",
    "grant permission",
    "allow access",
    "enable accessibility",
    "overlay permission",
    "notification access"
)

private val OverlayInputKeywords = listOf(
    "should i",
    "would you like",
    "which option",
    "choose",
    "pick one",
    "confirm",
    "need your input",
    "tap to continue"
)

internal fun deriveBackgroundSurfaceMode(
    isToolExecutionActive: Boolean,
    isModelBusy: Boolean,
    idleSurfaceMode: OverlaySurfaceMode
): OverlaySurfaceMode = if (isToolExecutionActive || isModelBusy) {
    OverlaySurfaceMode.DYNAMIC_ISLAND
} else {
    idleSurfaceMode
}

@VisibleForTesting
internal fun shouldPopOverlayRoute(overlayServiceInstance: OverlayService?): Boolean {
    return overlayServiceInstance == null
}

internal fun deriveOverlayInteractionDemand(
    overlayState: OverlayState,
    toolStatus: String?
): OverlayInteractionDemand {
    return when (overlayState.runState) {
        OverlayRunState.FAILED -> OverlayInteractionDemand.ERROR_ACTION_REQUIRED
        OverlayRunState.EXECUTING -> {
            val latestSystem = overlayState.lines
                .lastOrNull { it.type == OverlayLineType.SYSTEM }
                ?.text
                ?.lowercase()
                .orEmpty()
            val normalized = buildString {
                append(toolStatus?.lowercase().orEmpty())
                append(' ')
                append(latestSystem)
            }.trim()
            if (normalized.isBlank()) return OverlayInteractionDemand.NONE
            if (OverlayPermissionKeywords.any { normalized.contains(it) }) {
                return OverlayInteractionDemand.PERMISSION_REQUIRED
            }
            val endsWithQuestion = latestSystem.trimEnd().endsWith("?")
            if (endsWithQuestion || OverlayInputKeywords.any { normalized.contains(it) }) {
                return OverlayInteractionDemand.INPUT_REQUIRED
            }
            OverlayInteractionDemand.NONE
        }
        // Only force panel interaction while the run is actively executing.
        // Without this gate, stale question-like system lines from completed/idle/stopped runs
        // can keep interaction demand pinned and prevent idle-surface transitions.
        OverlayRunState.IDLE,
        OverlayRunState.COMPLETED,
        OverlayRunState.STOPPED -> OverlayInteractionDemand.NONE
    }
}

private fun generateOauthState(): String = UUID.randomUUID().toString()
private fun openInCustomTab(context: Context, url: String): Result<Unit> {
    return runCatching {
        val customTabsIntent = CustomTabsIntent.Builder().setShowTitle(true).build()
        customTabsIntent.launchUrl(context, Uri.parse(url))
    }.recoverCatching { customTabError ->
        Log.w("ChatActivity", "Custom tab failed, falling back to browser", customTabError)
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    }.onFailure { fallbackError ->
        Log.e("ChatActivity", "Failed to open URL", fallbackError)
    }.map { Unit }
}
private fun Uri.getOauthParameter(name: String): String? {
    val queryValue = getQueryParameter(name)?.trim()
    if (!queryValue.isNullOrBlank()) {
        return queryValue
    }
    val fragment = fragment ?: return null
    val pairs = fragment.split("&")
    for (pair in pairs) {
        val parts = pair.split("=", limit = 2)
        if (parts.size != 2) continue
        if (parts[0] != name) continue
        val value = Uri.decode(parts[1]).trim()
        if (value.isNotBlank()) {
            return value
        }
    }
    return null
}
private fun Uri.extractOauthTokenFromCallback(): String? {
    val keys = listOf(
        "token",
        "access_token",
        "accessToken",
        "oauth_token",
        "oauthToken"
    )
    for (key in keys) {
        val value = getOauthParameter(key)
        if (!value.isNullOrBlank()) {
            return value
        }
    }
    return null
}
private fun readCodexBridgeMode(prefs: android.content.SharedPreferences): CodexOauthBridgeMode {
    val raw = prefs.getString(PREF_CODEX_OAUTH_BRIDGE_MODE, CodexOauthBridgeMode.EMBEDDED.name)
    return runCatching { CodexOauthBridgeMode.valueOf(raw ?: CodexOauthBridgeMode.EMBEDDED.name) }
        .getOrDefault(CodexOauthBridgeMode.EMBEDDED)
}
private fun readEmbeddedCodexConfig(
    prefs: android.content.SharedPreferences,
    clientSecret: String?
): EmbeddedCodexOauthBridgeServer.Config {
    return EmbeddedCodexOauthBridgeServer.Config(
        authorizeUrl = prefs.getString(
            PREF_CODEX_OAUTH_AUTH_URL,
            EmbeddedCodexOauthBridgeServer.DEFAULT_AUTHORIZE_URL
        ) ?: EmbeddedCodexOauthBridgeServer.DEFAULT_AUTHORIZE_URL,
        tokenUrl = prefs.getString(
            PREF_CODEX_OAUTH_TOKEN_URL,
            EmbeddedCodexOauthBridgeServer.DEFAULT_TOKEN_URL
        ) ?: EmbeddedCodexOauthBridgeServer.DEFAULT_TOKEN_URL,
        clientId = prefs.getString(PREF_CODEX_OAUTH_CLIENT_ID, "") ?: "",
        clientSecret = clientSecret,
        scope = prefs.getString(
            PREF_CODEX_OAUTH_SCOPE,
            EmbeddedCodexOauthBridgeServer.DEFAULT_SCOPE
        ) ?: EmbeddedCodexOauthBridgeServer.DEFAULT_SCOPE
    )
}
private fun isRecoverableOauthSessionError(message: String?): Boolean {
    val normalized = message?.lowercase() ?: return false
    return normalized.contains("missing login_id") ||
        normalized.contains("unknown or expired login_id") ||
        normalized.contains("unknown login_id")
}
@VisibleForTesting
internal fun mapDeviceCodePollError(errorCode: String, description: String?): String {
    return when (errorCode) {
        "timeout" -> "The code expired. Request a new code to try again."
        "access_denied" -> "You denied the authorization request. To sign in, approve access in your browser."
        "not_enabled" -> "Device code login is not enabled. Ask your workspace admin to enable it, or use an API key."
        else -> "Sign-in failed: ${description?.take(TOKEN_PREVIEW_LIMIT) ?: errorCode}. Check your internet connection or try API key instead."
    }
}
@VisibleForTesting
internal fun formatDeviceCodeDiagnostics(
    diagnostics: ai.citros.core.DeviceCodeAuthClient.PollDiagnostics?
): String? {
    diagnostics ?: return null
    val lastStatus = diagnostics.lastHttpStatus?.toString() ?: "network"
    val preview = diagnostics.lastResponsePreview
        ?.trim()
        ?.replace('\n', ' ')
        ?.take(DIAGNOSTIC_PREVIEW_LIMIT)
        ?.takeIf { it.isNotBlank() }
        ?: "none"
    return "Attempts=${diagnostics.attempts}, elapsed=${diagnostics.elapsedSeconds}s, " +
        "pending403=${diagnostics.pending403Count}, pending404=${diagnostics.pending404Count}, " +
        "networkErrors=${diagnostics.networkErrorCount}, lastStatus=$lastStatus, preview=$preview"
}
@VisibleForTesting
internal fun formatDeviceCodeSessionInfo(
    response: ai.citros.core.DeviceCodeAuthClient.DeviceCodeResponse
): String {
    val authIdSuffix = response.deviceAuthId.takeLast(8)
    return "Session=$authIdSuffix, pollInterval=${response.interval}s"
}
@VisibleForTesting
internal suspend fun validateApiCredential(
    token: String,
    provider: Provider
): ApiKeyValidationStatus {
    return try {
        withTimeout(API_VALIDATION_TIMEOUT_MS) {
            val config = when (provider) {
                Provider.ANTHROPIC -> ProviderConfig.anthropic(token)
                Provider.OPENAI -> ProviderConfig.openAi(token)
                Provider.OPENROUTER -> ProviderConfig.openRouter(token)
            }
            val client: ProviderClient = when (provider) {
                Provider.ANTHROPIC -> AnthropicClient(config = config, systemPrompt = PhoneAgentPrompts.buildSystemPrompt())
                Provider.OPENAI -> OpenAiClient(config = config, systemPrompt = PhoneAgentPrompts.buildSystemPrompt())
                Provider.OPENROUTER -> OpenRouterClient(config = config, systemPrompt = PhoneAgentPrompts.buildSystemPrompt())
            }
            val conversation = Conversation().apply { addUser("ping") }
            val result = client.chat(conversation)
            if (result.isSuccess) {
                ApiKeyValidationStatus.VALID
            } else {
                ApiKeyValidationStatus.INVALID
            }
        }
    } catch (_: TimeoutCancellationException) {
        ApiKeyValidationStatus.UNKNOWN
    } catch (error: Throwable) {
        ApiKeyValidationStatus.UNKNOWN
    }
}
@VisibleForTesting
internal data class ResolvedWalletScope(
    val keyStore: ai.citros.core.KeyStore,
    val walletStorage: ai.citros.core.WalletStorage,
    val walletManager: WalletManager
)
@VisibleForTesting
internal fun resolveWalletScope(
    scopedWalletDependencies: WalletDependencies?,
    keyStoreOverride: ai.citros.core.KeyStore?,
    walletStorageOverride: ai.citros.core.WalletStorage?
): ResolvedWalletScope {
    val walletKeyStore = keyStoreOverride
        ?: scopedWalletDependencies?.keyStore
        ?: error("WalletDependencies must be provided when keyStoreOverride is null")
    val walletStorage = walletStorageOverride
        ?: scopedWalletDependencies?.walletStorage
        ?: error("WalletDependencies must be provided when walletStorageOverride is null")
    val walletManager = if (keyStoreOverride == null && walletStorageOverride == null) {
        scopedWalletDependencies?.walletManager
            ?: error("WalletDependencies must provide walletManager when overrides are absent")
    } else {
        WalletManager(walletStorage, walletKeyStore)
    }
    return ResolvedWalletScope(walletKeyStore, walletStorage, walletManager)
}
@Composable
internal fun CitrosChatTheme(
    themeMode: String = THEME_MODE_DEFAULT,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    content: @Composable () -> Unit
) {
    val useDark = when (themeMode) {
        "dark" -> true
        "light" -> false
        "system" -> isSystemInDarkTheme()
        else -> false // THEME_MODE_DEFAULT or fallback
    }
    ProvideCitrosSplashVisualTokens(flavor = flavor, isDark = useDark) {
        content()
    }
}

internal fun resolveSensorProviderForPreference(
    sensorContextEnabled: Boolean,
    appContext: Context
): SensorProvider? = if (sensorContextEnabled) {
    AndroidSensorProvider(appContext)
} else {
    null
}

internal fun applySensorContextPreference(
    prefs: android.content.SharedPreferences,
    appContext: Context,
    viewModel: ChatViewModel
) {
    val enabled = prefs.getBoolean(PREF_SENSOR_CONTEXT_ENABLED, PREF_SENSOR_CONTEXT_ENABLED_DEFAULT)
    viewModel.setSensorProvider(resolveSensorProviderForPreference(enabled, appContext))
}

internal fun createSensorContextPreferenceChangeListener(
    prefs: android.content.SharedPreferences,
    appContext: Context,
    viewModel: ChatViewModel
): android.content.SharedPreferences.OnSharedPreferenceChangeListener {
    return android.content.SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == PREF_SENSOR_CONTEXT_ENABLED) {
            applySensorContextPreference(prefs = prefs, appContext = appContext, viewModel = viewModel)
        }
    }
}

@Composable
fun ChatScreen(
    viewModel: ChatViewModel = viewModel(),
    onOpenSettings: () -> Unit = {},
    onOpenApiKeys: () -> Unit = onOpenSettings,
    onOpenOverlay: () -> Unit = {},
    startVoiceInputToken: Int = 0,
    keyStoreOverride: ai.citros.core.KeyStore? = null,
    walletStorageOverride: ai.citros.core.WalletStorage? = null,
    secureStoreOverride: CredentialStore? = null
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val listState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val prefs = remember(context) {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
    }
    val resolvedWalletScope = resolveWalletScope(
        scopedWalletDependencies = LocalWalletDependencies.current,
        keyStoreOverride = keyStoreOverride,
        walletStorageOverride = walletStorageOverride
    )
    val walletKeyStore = resolvedWalletScope.keyStore
    val walletStorage = resolvedWalletScope.walletStorage
    val walletManager = resolvedWalletScope.walletManager
    val agentFileManager = remember(context) { AgentFileManager.fromContext(context.applicationContext) }
    val secureStore = secureStoreOverride ?: remember(context) { SecureCredentialStore(context) }
    val oauthBridgeClient = remember { CodexOauthBridgeClient() }
    var embeddedBridge by remember { mutableStateOf<EmbeddedCodexOauthBridgeServer?>(null) }
    var showQuickSwitcher by remember { mutableStateOf(false) }
    var selectedFlavor by remember { mutableStateOf(readSelectedFlavor(context)) }
    val isDarkTheme = LocalCitrosIsDark.current
    val directiveSurfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    var codexOauthStatus by remember { mutableStateOf<String?>(null) }
    var codexOauthBusy by remember { mutableStateOf(false) }
    val walletStateFlow = remember { MutableStateFlow(walletManager.loadOrDefault()) }
    val walletMutationMutex = remember { Mutex() }
    val walletState by walletStateFlow.collectAsState()
    val applySensorContextPreferenceFromPrefs = {
        applySensorContextPreference(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
    }
    val stopEmbeddedBridge = {
        embeddedBridge?.stop()
        embeddedBridge = null
    }
    fun readSecureBackedValue(key: String): String? {
        val secureValue = secureStore.getString(key)
        if (!secureValue.isNullOrBlank()) {
            return secureValue
        }
        val legacyValue = prefs.getString(key, null)?.takeIf { it.isNotBlank() } ?: return null
        secureStore.putString(key, legacyValue)
        prefs.edit().remove(key).apply()
        return legacyValue
    }
    fun writeSecureBackedValue(key: String, value: String?) =
        writeSecureBacked(secureStore, prefs, key, value)
    suspend fun ensureEmbeddedBridgeRunning(
        config: EmbeddedCodexOauthBridgeServer.Config
    ): Result<String> {
        if (config.clientId.isBlank()) {
            return Result.failure(
                IllegalArgumentException("OAuth client ID is required for on-device bridge mode")
            )
        }
        val existing = embeddedBridge
        if (existing != null && existing.isRunning()) {
            return existing.start().map { it.baseUrl }
        }
        stopEmbeddedBridge()
        val candidate = EmbeddedCodexOauthBridgeServer(config)
        return candidate.start().mapCatching { running ->
            embeddedBridge = candidate
            running.baseUrl
        }.onFailure {
            candidate.stop()
        }
    }
    val clearPendingOauthState = {
        secureStore.remove(PREF_CODEX_OAUTH_LOGIN_ID)
        secureStore.remove(PREF_CODEX_OAUTH_CODE_VERIFIER)
        prefs.edit()
            .remove(PREF_CODEX_OAUTH_STATE)
            .remove(PREF_CODEX_OAUTH_STATE_TIMESTAMP)
            .remove(PREF_CODEX_OAUTH_BRIDGE)
            .remove(PREF_CODEX_OAUTH_LOGIN_ID)
            .remove(PREF_CODEX_OAUTH_CODE_VERIFIER)
            .apply()
    }
    val refreshWalletState = {
        walletStateFlow.value = walletManager.loadOrDefault()
    }
    suspend fun mutateWalletAndRefresh(
        reconfigure: Boolean = false,
        modelOnlyUpdate: Boolean = false,
        mutation: () -> Unit
    ) {
        walletMutationMutex.withLock {
            mutation()
            val updatedState = walletManager.loadOrDefault()
            walletStateFlow.value = updatedState
            if (reconfigure && updatedState.activeKeyId != null) {
                viewModel.setSystemPrompt(OnboardingPersistence.systemPromptForStartup(agentFileManager))
                if (modelOnlyUpdate) {
                    viewModel.updateModelsFromWallet(walletManager)
                } else {
                    viewModel.configureWithWallet(walletManager)
                }
            }
        }
    }
    val persistCloudCredential = { token: String, provider: Provider?, authKind: CloudAuthKind? ->
        // Secure storage
        secureStore.putString(PREF_CLOUD_TOKEN, token)
        // Legacy storage (keep for backward compat)
        prefs.edit()
            .apply {
                if (provider != null) {
                    putString(PREF_CLOUD_PROVIDER, provider.name)
                } else {
                    remove(PREF_CLOUD_PROVIDER)
                }
                if (authKind != null) {
                    putString(PREF_CLOUD_AUTH_KIND, authKind.name)
                } else {
                    remove(PREF_CLOUD_AUTH_KIND)
                }
            }
            .remove(PREF_CLOUD_TOKEN)
            .remove(PREF_LEGACY_ANTHROPIC_TOKEN)
            .remove(PREF_LOCAL_URL)
            .apply()
        // Also add to wallet (check for duplicates first)
        val detectedProvider = provider ?: ProviderConfig.detectProvider(token) ?: Provider.OPENAI
        val currentState = walletManager.loadOrDefault()
        // Find existing key with matching token
        val existingKey = currentState.keys.find { key ->
            key.provider == detectedProvider && walletKeyStore.get(key.id) == token
        }
        if (existingKey != null) {
            // Token already exists - just set it active
            walletManager.setActiveKey(existingKey.id)
        } else {
            // New token - add it to wallet
            val label = "${detectedProvider.name.lowercase().replaceFirstChar { it.uppercase() }} Key"
            walletManager.addKey(detectedProvider, label, token)
            walletManager.setActiveKey(walletManager.loadOrDefault().keys.last().id)
        }
        // Always update model configuration for the provider
        walletManager.setChatModel(ModelConfig.defaultChatModel(detectedProvider))
        walletManager.setActionModel(ModelConfig.defaultActionModel(detectedProvider))
        refreshWalletState()
    }
    val applyCodexOauthToken = { token: String ->
        clearPendingOauthState()
        stopEmbeddedBridge()
        codexOauthBusy = false
        codexOauthStatus = "OpenAI account connected via OAuth."
        persistCloudCredential(token, Provider.OPENAI, CloudAuthKind.OPENAI_CODEX_OAUTH)
        coroutineScope.launch {
            mutateWalletAndRefresh(reconfigure = true) { }
        }
    }
    fun buildCodexOauthStartRequestFromPrefs(): CodexOauthStartRequest {
        val mode = readCodexBridgeMode(prefs)
        val externalBridgeUrl = prefs.getString(
            PREF_CODEX_OAUTH_BRIDGE,
            CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL
        ) ?: CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL
        val embeddedConfig = readEmbeddedCodexConfig(
            prefs = prefs,
            clientSecret = readSecureBackedValue(PREF_CODEX_OAUTH_CLIENT_SECRET)
        )
        return when (mode) {
            CodexOauthBridgeMode.EXTERNAL -> CodexOauthStartRequest(
                mode = CodexOauthBridgeMode.EXTERNAL,
                externalBridgeUrl = externalBridgeUrl
            )
            CodexOauthBridgeMode.EMBEDDED -> CodexOauthStartRequest(
                mode = CodexOauthBridgeMode.EMBEDDED,
                embeddedConfig = embeddedConfig
            )
        }
    }
    suspend fun beginCodexOauthSignIn(
        request: CodexOauthStartRequest,
        startingStatus: String = "Starting OpenAI sign-in..."
    ) {
        codexOauthBusy = true
        codexOauthStatus = startingStatus
        clearPendingOauthState()
        val state = generateOauthState()
        val embeddedConfig = request.embeddedConfig ?: readEmbeddedCodexConfig(
            prefs = prefs,
            clientSecret = readSecureBackedValue(PREF_CODEX_OAUTH_CLIENT_SECRET)
        )
        val normalizedBridge = when (request.mode) {
            CodexOauthBridgeMode.EXTERNAL -> {
                request.externalBridgeUrl
                    ?.trim()
                    .orEmpty()
                    .ifBlank { CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL }
            }
            CodexOauthBridgeMode.EMBEDDED -> {
                ensureEmbeddedBridgeRunning(embeddedConfig)
                    .onFailure { error ->
                        codexOauthBusy = false
                        codexOauthStatus =
                            "Could not start on-device bridge: ${error.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}"
                        clearPendingOauthState()
                        stopEmbeddedBridge()
                    }
                    .getOrElse { return }
            }
        }
        // Validate bridge URL starts with http:// or https://
        if (!normalizedBridge.startsWith("http://") && !normalizedBridge.startsWith("https://")) {
            codexOauthBusy = false
            codexOauthStatus = "Invalid bridge URL. Must start with http:// or https://"
            return
        }
        oauthBridgeClient.startLogin(
            bridgeBaseUrl = normalizedBridge,
            redirectUri = ChatActivity.OAUTH_CALLBACK_URI,
            state = state
        ).onSuccess { start ->
            prefs.edit()
                .putString(PREF_CODEX_OAUTH_STATE, state)
                .putLong(PREF_CODEX_OAUTH_STATE_TIMESTAMP, System.currentTimeMillis())
                .putString(PREF_CODEX_OAUTH_BRIDGE, normalizedBridge)
                .putString(PREF_CODEX_OAUTH_BRIDGE_MODE, request.mode.name)
                .putString(PREF_CODEX_OAUTH_AUTH_URL, embeddedConfig.authorizeUrl)
                .putString(PREF_CODEX_OAUTH_TOKEN_URL, embeddedConfig.tokenUrl)
                .putString(PREF_CODEX_OAUTH_CLIENT_ID, embeddedConfig.clientId)
                .putString(PREF_CODEX_OAUTH_SCOPE, embeddedConfig.scope)
                .apply()
            writeSecureBackedValue(PREF_CODEX_OAUTH_CLIENT_SECRET, embeddedConfig.clientSecret)
            writeSecureBackedValue(PREF_CODEX_OAUTH_LOGIN_ID, start.loginId)
            writeSecureBackedValue(PREF_CODEX_OAUTH_CODE_VERIFIER, start.codeVerifier)
            val launchResult = runCatching {
                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(start.authUrl)))
            }
            if (launchResult.isSuccess) {
                codexOauthStatus = "Browser opened. Complete OpenAI sign-in, then return to Citros."
            } else {
                codexOauthBusy = false
                codexOauthStatus =
                    "Could not open browser: ${launchResult.exceptionOrNull()?.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}"
                clearPendingOauthState()
                if (request.mode == CodexOauthBridgeMode.EMBEDDED) {
                    stopEmbeddedBridge()
                }
            }
        }.onFailure { error ->
            codexOauthBusy = false
            codexOauthStatus =
                "Could not start OAuth: ${error.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}"
            clearPendingOauthState()
            if (request.mode == CodexOauthBridgeMode.EMBEDDED) {
                stopEmbeddedBridge()
            }
        }
    }
    // Check accessibility status on resume and refresh wallet state from the source of truth.
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                coroutineScope.launch {
                    mutateWalletAndRefresh(reconfigure = true) { }
                }
                viewModel.updateAccessibilityStatus(
                    CitrosAccessibilityService.isEnabled(context)
                )
                selectedFlavor = readSelectedFlavor(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    DisposableEffect(prefs) {
        val listener = createSensorContextPreferenceChangeListener(
            prefs = prefs,
            appContext = context.applicationContext,
            viewModel = viewModel
        )
        prefs.registerOnSharedPreferenceChangeListener(listener)
        onDispose {
            prefs.unregisterOnSharedPreferenceChangeListener(listener)
        }
    }
    // Load saved credentials on first launch
    LaunchedEffect(Unit) {
        // Migrate legacy plaintext sensitive OAuth/device-code fields into encrypted storage.
        listOf(
            PREF_CODEX_OAUTH_CLIENT_SECRET,
            PREF_CODEX_OAUTH_LOGIN_ID,
            PREF_CODEX_OAUTH_CODE_VERIFIER,
            PREF_DEVICE_CODE_REFRESH_TOKEN
        ).forEach { key ->
            readSecureBackedValue(key)
        }
        applySensorContextPreferenceFromPrefs()
        // Initialize on-device memory provider for remember/recall/list_memories tools.
        // Store DB reference on the hosting Activity for cleanup in onDestroy().
        val activity = context as? ChatActivity
        if (activity != null) {
            val db = android.database.sqlite.SQLiteDatabase.openOrCreateDatabase(
                context.getDatabasePath("citros_memories.db"), null
            )
            activity.memoryDb = db
            viewModel.setMemoryProvider(SqliteMemoryProvider(db))
        } else {
            android.util.Log.w("ChatActivity", "Context is not ChatActivity — memory provider not initialized")
        }
        viewModel.setAgentFileManager(agentFileManager)
        // Load tool category settings from SharedPreferences
        if (ai.citros.core.FeatureFlags.toolGroupingV1Enabled) {
            val toolCatPrefs = context.getSharedPreferences(CITROS_PREFS, android.content.Context.MODE_PRIVATE)
            viewModel.setToolCategorySettings(loadToolCategorySettings(toolCatPrefs))
        }
        viewModel.setSystemPrompt(OnboardingPersistence.systemPromptForStartup(agentFileManager))
        // Search config: user Settings override server-delivered keys
        val searchPrefs = context.getSharedPreferences(CITROS_PREFS, android.content.Context.MODE_PRIVATE)
        val userTinyFishKey = searchPrefs.getString(PREF_TINYFISH_API_KEY, null)
        viewModel.setSearchConfig(
            searxngUrl = searchPrefs.getString(PREF_SEARCH_BASE_URL, null),
            braveKey = searchPrefs.getString(PREF_BRAVE_API_KEY, null),
            tinyFishKey = userTinyFishKey
        )
        // App token is compiled into the APK at build time (scripts/release.sh).
        // Empty string = no token (dev build without -PcitrosAppToken).
        val compiledAppToken = BuildConfig.CITROS_APP_TOKEN.takeIf { it.isNotBlank() }
        if (compiledAppToken != null) {
            viewModel.updateCitrosKeys(appToken = compiledAppToken)
        }

        // Fetch server-delivered keys (TinyFish) asynchronously.
        // When compiled app token is present, sends Bearer auth.
        // When absent (dev builds), still attempts unauthenticated fetch —
        // server may return 401 (gracefully handled) or partial response.
        if (activity != null) {
            activity.lifecycleScope.launch {
                val delivered = ai.citros.core.KeyDeliveryClient(
                    appToken = compiledAppToken
                ).fetchKeys()
                if (delivered != null) {
                    viewModel.updateCitrosKeys(
                        appToken = if (compiledAppToken == null) delivered.appToken else null,
                        tinyFishKey = if (userTinyFishKey == null) delivered.tinyfish else null
                    )
                }
            }
        }
        // Try wallet first
        walletStateFlow.value = walletManager.loadOrDefault()
        if (walletState.keys.isNotEmpty() &&
            walletState.activeKeyId != null &&
            walletState.keys.any { it.id == walletState.activeKeyId }) {
            viewModel.configureWithWallet(walletManager)
        } else {
            // Fall back to secure store → plain prefs → legacy token
            val secureToken = secureStore.getString(PREF_CLOUD_TOKEN)
            val plainToken = prefs.getString(PREF_CLOUD_TOKEN, null)
            val legacyToken = prefs.getString(PREF_LEGACY_ANTHROPIC_TOKEN, null)
            val token = when {
                !secureToken.isNullOrBlank() -> secureToken
                !plainToken.isNullOrBlank() -> plainToken
                !legacyToken.isNullOrBlank() -> legacyToken
                else -> null
            }
            // Migrate plaintext/legacy tokens to secure store
            if (!token.isNullOrBlank() && secureToken.isNullOrBlank()) {
                secureStore.putString(PREF_CLOUD_TOKEN, token)
                prefs.edit()
                    .remove(PREF_CLOUD_TOKEN)
                    .remove(PREF_LEGACY_ANTHROPIC_TOKEN)
                    .apply()
            }
            val savedProvider = prefs.getString(PREF_CLOUD_PROVIDER, null)?.let { raw ->
                runCatching { Provider.valueOf(raw) }.getOrNull()
            }
            val savedAuthKind = prefs.getString(PREF_CLOUD_AUTH_KIND, null)?.let { raw ->
                runCatching { CloudAuthKind.valueOf(raw) }.getOrNull()
            }
            val localUrl = prefs.getString(PREF_LOCAL_URL, null)
            val localModel = prefs.getString(PREF_LOCAL_MODEL, "qwen2.5:3b")
            when {
                localUrl != null -> viewModel.configureWithLocalLLM(localUrl, localModel!!)
                token != null -> {
                    // Migrate to wallet and configure from it
                    walletManager.migrateFromLegacy(token, savedProvider, savedAuthKind?.name)
                    refreshWalletState()
                    viewModel.configureWithWallet(walletManager)
                }
            }
        }
        viewModel.updateAccessibilityStatus(
            CitrosAccessibilityService.isEnabled(context)
        )
    }
    LaunchedEffect(Unit) {
        ChatActivity.oauthCallbackFlow().collect { uri ->
            val callbackUri = uri ?: return@collect
            ChatActivity.clearOauthCallback()
            val pendingState = prefs.getString(PREF_CODEX_OAUTH_STATE, null)
            val stateTimestamp = prefs.getLong(PREF_CODEX_OAUTH_STATE_TIMESTAMP, 0)
            val stateAge = System.currentTimeMillis() - stateTimestamp
            if (pendingState.isNullOrBlank() || stateAge > OAUTH_STATE_EXPIRY_MS) {
                clearPendingOauthState()
                codexOauthBusy = false
                codexOauthStatus = if (pendingState.isNullOrBlank()) {
                    "No active OAuth login was found. Start sign-in again."
                } else {
                    "OAuth session expired. Please start sign-in again."
                }
                return@collect
            }
            val callbackState = callbackUri.getOauthParameter("state")
            if (!callbackState.isNullOrBlank() && callbackState != pendingState) {
                codexOauthBusy = false
                codexOauthStatus = "OAuth state mismatch. Please try again."
                clearPendingOauthState()
                return@collect
            }
            val oauthError = callbackUri.getOauthParameter("error")
            if (!oauthError.isNullOrBlank()) {
                codexOauthBusy = false
                val errorDescription = callbackUri.getOauthParameter("error_description")
                codexOauthStatus = buildString {
                    append("OpenAI OAuth failed: ")
                    append(oauthError)
                    if (!errorDescription.isNullOrBlank()) {
                        append(" (")
                        append(errorDescription.take(TOKEN_PREVIEW_LIMIT))
                        append(")")
                    }
                }
                clearPendingOauthState()
                return@collect
            }
            callbackUri.extractOauthTokenFromCallback()?.let { token ->
                applyCodexOauthToken(token)
                return@collect
            }
            val code = callbackUri.getOauthParameter("code")
            if (code.isNullOrBlank()) {
                codexOauthBusy = false
                codexOauthStatus = "OAuth callback is missing token and code."
                clearPendingOauthState()
                return@collect
            }
            codexOauthStatus = "Completing OpenAI sign-in..."
            val bridgeBaseUrl = prefs.getString(
                PREF_CODEX_OAUTH_BRIDGE,
                CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL
            ) ?: CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL
            val resolvedBridgeBaseUrl = when (readCodexBridgeMode(prefs)) {
                CodexOauthBridgeMode.EXTERNAL -> bridgeBaseUrl
                CodexOauthBridgeMode.EMBEDDED -> {
                    val embeddedConfig = readEmbeddedCodexConfig(
                        prefs = prefs,
                        clientSecret = readSecureBackedValue(PREF_CODEX_OAUTH_CLIENT_SECRET)
                    )
                    ensureEmbeddedBridgeRunning(embeddedConfig)
                        .onFailure { error ->
                            codexOauthBusy = false
                            codexOauthStatus = "Could not start on-device bridge: ${error.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}"
                            clearPendingOauthState()
                        }
                        .getOrElse { return@collect }
                }
            }
            oauthBridgeClient.exchangeCode(
                bridgeBaseUrl = resolvedBridgeBaseUrl,
                code = code,
                state = callbackState ?: pendingState,
                loginId = readSecureBackedValue(PREF_CODEX_OAUTH_LOGIN_ID),
                codeVerifier = readSecureBackedValue(PREF_CODEX_OAUTH_CODE_VERIFIER),
                redirectUri = ChatActivity.OAUTH_CALLBACK_URI
            ).onSuccess { exchange ->
                applyCodexOauthToken(exchange.accessToken)
            }.onFailure { error ->
                if (isRecoverableOauthSessionError(error.message)) {
                    codexOauthStatus = "OAuth session expired. Restarting sign-in..."
                    val restartRequest = buildCodexOauthStartRequestFromPrefs()
                    stopEmbeddedBridge()
                    beginCodexOauthSignIn(
                        request = restartRequest,
                        startingStatus = "OAuth session expired. Restarting sign-in..."
                    )
                } else {
                    codexOauthBusy = false
                    codexOauthStatus =
                        "OAuth exchange failed: ${error.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}"
                    clearPendingOauthState()
                    stopEmbeddedBridge()
                }
            }
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            stopEmbeddedBridge()
        }
    }
    // Auto-scroll when new messages arrive
    // #552: scroll to end-spacer item (size, not size-1) to ensure last message
    // is fully visible including bottom content padding
    LaunchedEffect(viewModel.messages.size) {
        if (viewModel.messages.isNotEmpty()) {
            listState.animateScrollToItem((listState.layoutInfo.totalItemsCount - 1).coerceAtLeast(0))
        }
    }
    // Auto-scroll during streaming responses (#618).
    // When the assistant message content updates in-place (size unchanged),
    // scroll to bottom only if the user is already near the bottom.
    // This prevents hijacking scroll when the user has scrolled up to read history.
    val streamingVersion = viewModel.streamingContentVersion.intValue
    LaunchedEffect(streamingVersion) {
        if (streamingVersion == 0) return@LaunchedEffect
        val layoutInfo = listState.layoutInfo
        val lastVisibleItem = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
        val totalItems = layoutInfo.totalItemsCount
        // "Near bottom" = the last visible item is among the final NEAR_BOTTOM_THRESHOLD items.
        // For totalItems=10 (indices 0-9), this triggers when lastVisibleItem >= 7 (items 7, 8, 9).
        // When totalItems == 0 (initial load / empty layout), treat as near-bottom — the
        // subsequent messages.isNotEmpty() check prevents scrolling to an invalid index.
        val isNearBottom = totalItems == 0 || lastVisibleItem >= totalItems - NEAR_BOTTOM_THRESHOLD
        if (isNearBottom && viewModel.messages.isNotEmpty()) {
            // Use scrollToItem (instant jump) instead of animateScrollToItem because
            // streaming deltas arrive rapidly — animated scrolls get cancelled mid-flight
            // by the next recomposition, causing jittery behavior.
            listState.scrollToItem((totalItems - 1).coerceAtLeast(0))
        }
    }
    // Auto-scroll when keyboard appears (#450, #552) — only on hidden→visible transition
    val imeBottom = WindowInsets.ime.getBottom(LocalDensity.current)
    val wasKeyboardHidden = remember { mutableStateOf(imeBottom == 0) }
    LaunchedEffect(imeBottom) {
        if (wasKeyboardHidden.value && imeBottom > 0 && viewModel.messages.isNotEmpty()) {
            listState.animateScrollToItem((listState.layoutInfo.totalItemsCount - 1).coerceAtLeast(0))
        }
        wasKeyboardHidden.value = imeBottom == 0
    }
    val isConfigured = viewModel.isConfigured.value
    val hasActiveWalletKey = walletState.activeKeyId != null &&
        walletState.keys.any { it.id == walletState.activeKeyId }
    var showApiKeyRequiredFlag by rememberSaveable { mutableStateOf(false) }
    val canInteractWithModel = hasActiveWalletKey && isConfigured
    val requiresProviderSetup = hasActiveWalletKey && !isConfigured
    LaunchedEffect(canInteractWithModel) {
        if (canInteractWithModel) {
            showApiKeyRequiredFlag = false
        }
    }
    val requireModelAccess = {
        if (canInteractWithModel) {
            true
        } else {
            showApiKeyRequiredFlag = true
            false
        }
    }
    val openModelSwitcher = {
        if (canInteractWithModel) {
            showQuickSwitcher = true
        } else if (requiresProviderSetup) {
            onOpenSettings()
        } else {
            onOpenApiKeys()
        }
    }
    Scaffold(
        containerColor = Color.Transparent,
        topBar = {
            val statusBarTopPadding = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
            val chatSubtitle = when {
                canInteractWithModel -> shortModelName(walletState.chatModelId)
                requiresProviderSetup -> "Provider setup required"
                else -> "No provider connected"
            }
            val chatSubtitleColor = if (canInteractWithModel) {
                directiveSurfaces.labelSecondary
            } else {
                CitrosColorScheme.error
            }
            val quickSwitcherArrowColor = if (canInteractWithModel) {
                directiveSurfaces.labelPrimary
            } else {
                CitrosColorScheme.error
            }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(directiveSurfaces.background)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(
                            top = statusBarTopPadding + 10.dp,
                            bottom = 10.dp,
                            start = 14.dp,
                            end = 14.dp
                        ),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    CitrosDirectiveOrb(
                        flavor = selectedFlavor,
                        size = 32.dp,
                        modifier = Modifier
                            .testTag(TEST_TAG_QUICK_SWITCHER_CHIP)
                            .clickable { openModelSwitcher() }
                    )
                    Spacer(Modifier.width(10.dp))
                    Column(
                        modifier = Modifier
                            .weight(1f)
                            .padding(end = 4.dp)
                            .testTag(TEST_TAG_QUICK_SWITCHER_HEADER)
                            .clickable { openModelSwitcher() }
                    ) {
                        Text(
                            text = "Citros",
                            style = CitrosTypography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                            color = directiveSurfaces.labelPrimary
                        )
                        Row(
                            modifier = Modifier
                                .padding(top = 1.dp)
                                .wrapContentWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(2.dp)
                        ) {
                            Text(
                                text = chatSubtitle,
                                style = CitrosTypography.bodySmall,
                                color = chatSubtitleColor,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.widthIn(max = 190.dp)
                            )
                            if (canInteractWithModel) {
                                Icon(
                                    imageVector = Icons.Default.ExpandMore,
                                    contentDescription = null,
                                    tint = quickSwitcherArrowColor,
                                    modifier = Modifier.size(18.dp)
                                )
                            }
                        }
                    }
                    CitrosLiquidGlassSurface(
                        modifier = Modifier
                            .size(36.dp)
                            .semantics { contentDescription = "Settings" },
                        shape = CircleShape,
                        onClick = onOpenSettings,
                        baseColor = directiveSurfaces.surface1,
                        borderColor = directiveSurfaces.separatorLight,
                        borderWidth = 1.dp
                    ) {
                        Box(
                            modifier = Modifier.fillMaxSize(),
                            contentAlignment = Alignment.Center
                        ) {
                            SettingsGlyph(
                                tint = directiveSurfaces.labelPrimary,
                                modifier = Modifier.size(20.dp)
                            )
                        }
                    }
                }
                HorizontalDivider(
                    color = directiveSurfaces.separator,
                    thickness = 0.5.dp
                )
                if (showApiKeyRequiredFlag) {
                    Surface(
                        modifier = Modifier
                            .fillMaxWidth()
                            .testTag(TEST_TAG_API_KEY_REQUIRED_MODAL),
                        color = directiveSurfaces.surface1,
                        border = BorderStroke(1.dp, directiveSurfaces.red.copy(alpha = 0.9f))
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 14.dp, vertical = 10.dp),
                            verticalArrangement = Arrangement.spacedBy(4.dp)
                        ) {
                            if (requiresProviderSetup) {
                                Text(
                                    text = "Provider setup is incomplete.",
                                    style = CitrosTypography.bodySmall,
                                    color = directiveSurfaces.labelSecondary
                                )
                            }
                            Text(
                                text = "Connect a provider to continue →",
                                style = CitrosTypography.bodySmall,
                                fontWeight = FontWeight.Medium,
                                color = directiveSurfaces.red,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable {
                                        showApiKeyRequiredFlag = false
                                        onOpenApiKeys()
                                    }
                            )
                        }
                    }
                }
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .background(directiveSurfaces.background)
        ) {
            // Accessibility banner if not enabled
            if (!viewModel.accessibilityEnabled.value) {
                AccessibilityBanner(
                    flavor = selectedFlavor,
                    onEnable = { CitrosAccessibilityService.openSettings(context) }
                )
            }
            val showCenteredEmptyState = viewModel.messages.isEmpty() && !viewModel.isLoading.value
            if (showCenteredEmptyState) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp),
                    contentAlignment = Alignment.Center
                ) {
                    ChatEmptyState(
                        flavor = selectedFlavor
                    )
                }
            } else {
                // Messages list
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(vertical = 8.dp)
                ) {
                    items(viewModel.messages) { message ->
                        MessageBubble(
                            message = message,
                            flavor = selectedFlavor,
                        )
                    }
                    if (viewModel.isLoading.value) {
                        item {
                            LoadingIndicator(
                                flavor = selectedFlavor,
                                label = when {
                                    viewModel.hasQueuedSteer.value -> "Redirecting..."
                                    viewModel.currentToolStatus.value != null -> viewModel.currentToolStatus.value!!
                                    else -> "Thinking"
                                }
                            )
                        }
                    }
                    // #552: End spacer — scroll target to ensure last message is fully visible.
                    // animateScrollToItem(messages.size) lands here, pushing content up.
                    item { Spacer(Modifier.height(4.dp)) }
                }
            }
            // Error snackbar
            viewModel.error.value?.let { error ->
                Snackbar(
                    modifier = Modifier.padding(16.dp),
                    action = {
                        TextButton(onClick = { viewModel.clearError() }) {
                            Text("Dismiss")
                        }
                    }
                ) {
                    Text(error)
                }
            }
            // Input field
            val voiceReadyState by viewModel.voiceReady.collectAsState()
            val voiceManagerState by viewModel.voiceManager.collectAsState()
            HorizontalDivider(
                color = directiveSurfaces.separator,
                thickness = 0.5.dp
            )
            MessageInput(
                onSend = {
                    if (requireModelAccess()) {
                        viewModel.sendMessage(it)
                        true
                    } else {
                        false
                    }
                },
                onSteer = {
                    if (requireModelAccess()) {
                        viewModel.steerMessage(it)
                        true
                    } else {
                        false
                    }
                },
                onQueue = {
                    if (requireModelAccess()) {
                        viewModel.setQueuedMessage(it)
                        true
                    } else {
                        false
                    }
                },
                queuedMessage = viewModel.queuedMessage.value,
                onSteerQueuedMessage = {
                    if (requireModelAccess()) {
                        viewModel.setQueuedMessage("")
                        viewModel.steerMessage(it)
                    }
                },
                onCancel = { viewModel.cancelToolExecution() },
                isLoading = viewModel.isLoading.value,
                flavor = selectedFlavor,
                modifier = Modifier.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 24.dp),
                placeholder = "Message",
                voiceReady = voiceReadyState,
                voiceManager = voiceManagerState,
                startListeningToken = startVoiceInputToken
            )
        }
    }
    if (showQuickSwitcher && canInteractWithModel) {
        QuickSwitcherSheet(
            walletState = walletState,
            activeConfig = walletState.activeConfig(walletKeyStore),
            flavor = selectedFlavor,
            onDismiss = { showQuickSwitcher = false },
            onSelectKey = { keyId ->
                coroutineScope.launch {
                    mutateWalletAndRefresh(reconfigure = true) {
                        walletManager.setActiveKey(keyId)
                    }
                }
            },
            onSelectChatModel = { modelId ->
                coroutineScope.launch {
                    mutateWalletAndRefresh(reconfigure = true, modelOnlyUpdate = true) {
                        walletManager.setChatModel(modelId)
                    }
                }
            },
            onSelectActionModel = { modelId ->
                coroutineScope.launch {
                    mutateWalletAndRefresh(reconfigure = true, modelOnlyUpdate = true) {
                        walletManager.setActionModel(modelId)
                    }
                }
            },
            onManageKeys = {
                showQuickSwitcher = false
                onOpenSettings()
            }
        )
    }
}

@Composable
private fun SettingsGlyph(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val center = Offset(size.width / 2f, size.height / 2f)
        val radius = size.minDimension / 2f
        val strokeWidth = radius * 0.22f
        val outerGearRadius = radius * 0.78f
        val spokeInnerRadius = radius * 0.58f

        drawCircle(
            color = tint,
            radius = radius * 0.48f,
            style = Stroke(width = strokeWidth)
        )
        drawCircle(
            color = tint,
            radius = radius * 0.13f
        )

        repeat(8) { index ->
            val angle = Math.toRadians((index * 45.0) - 90.0)
            val start = Offset(
                x = center.x + cos(angle).toFloat() * spokeInnerRadius,
                y = center.y + sin(angle).toFloat() * spokeInnerRadius
            )
            val end = Offset(
                x = center.x + cos(angle).toFloat() * outerGearRadius,
                y = center.y + sin(angle).toFloat() * outerGearRadius
            )
            drawLine(
                color = tint,
                start = start,
                end = end,
                strokeWidth = strokeWidth,
                cap = StrokeCap.Round
            )
        }
    }
}
@Composable
private fun FlavorToolbarIconButton(
    icon: ImageVector,
    contentDescription: String,
    flavor: CitrosFlavor,
    onClick: () -> Unit,
    enabled: Boolean = true
) {
    val iconColor = if (enabled) {
        lerp(flavor.primary, CitrosColorScheme.onSurface, 0.34f)
    } else {
        flavor.primary.copy(alpha = 0.34f)
    }
    CitrosLiquidGlassSurface(
        modifier = Modifier
            .padding(horizontal = 2.dp)
            .size(36.dp),
        shape = CircleShape,
        onClick = onClick,
        enabled = enabled,
        borderColor = if (enabled) {
            flavor.primary.copy(alpha = 0.42f)
        } else {
            flavor.primary.copy(alpha = 0.20f)
        },
        borderWidth = 1.dp,
        highlightColor = if (enabled) flavor.primary else null,
        warmth = if (enabled) 0.96f else 0.62f
    ) {
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            CitrosIcon(
                imageVector = icon,
                contentDescription = contentDescription,
                tint = iconColor
            )
        }
    }
}
@Composable
internal fun AccessibilityBanner(
    onEnable: () -> Unit,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE
) {
    CitrosLiquidGlassSurface(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp),
        shape = RoundedCornerShape(16.dp),
        borderColor = flavor.primary.copy(alpha = 0.34f),
        borderWidth = 1.dp,
        highlightColor = flavor.primary,
        warmth = 0.90f,
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    "Enable phone control",
                    style = CitrosTypography.titleSmall,
                    color = flavor.primary
                )
                Text(
                    "Let Citros see and control your screen",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onSurface.copy(alpha = 0.78f)
                )
            }
            TextButton(onClick = onEnable) {
                CitrosIcon(CitrosIcons.Settings, contentDescription = null, tint = flavor.primary)
                Spacer(Modifier.width(4.dp))
                Text("Enable", color = flavor.primary)
            }
        }
    }
}
@Composable
fun SignInPrompt(
    onToken: (String, Provider?, CloudAuthKind?) -> Unit,
    onLocalLLM: (String, String) -> Unit,
    onStartCodexOauth: (CodexOauthStartRequest) -> Unit,
    onOpenApiKeySetup: (Provider) -> Unit,
    codexOauthInProgress: Boolean,
    codexOauthStatus: String?,
    initialCodexBridgeUrl: String,
    initialCodexBridgeMode: CodexOauthBridgeMode,
    initialEmbeddedConfig: EmbeddedCodexOauthBridgeServer.Config
) {
    var mode by remember { mutableStateOf("main") } // main, token, codex_oauth, local
    var inputValue by remember { mutableStateOf("") }
    var modelName by remember { mutableStateOf("qwen2.5:3b") }
    var selectedProvider by remember { mutableStateOf<Provider?>(null) }
    var selectedAuthKind by remember { mutableStateOf<CloudAuthKind?>(null) }
    var codexBridgeMode by remember(initialCodexBridgeMode) {
        mutableStateOf(initialCodexBridgeMode)
    }
    var codexBridgeUrl by remember(initialCodexBridgeUrl) {
        mutableStateOf(initialCodexBridgeUrl)
    }
    var codexAuthUrl by remember(initialEmbeddedConfig.authorizeUrl) {
        mutableStateOf(initialEmbeddedConfig.authorizeUrl)
    }
    var codexTokenUrl by remember(initialEmbeddedConfig.tokenUrl) {
        mutableStateOf(initialEmbeddedConfig.tokenUrl)
    }
    var codexClientId by remember(initialEmbeddedConfig.clientId) {
        mutableStateOf(initialEmbeddedConfig.clientId)
    }
    var codexClientSecret by remember(initialEmbeddedConfig.clientSecret) {
        mutableStateOf(initialEmbeddedConfig.clientSecret.orEmpty())
    }
    var codexScope by remember(initialEmbeddedConfig.scope) {
        mutableStateOf(initialEmbeddedConfig.scope)
    }
    var apiKeyValidationStatus by remember { mutableStateOf(ApiKeyValidationStatus.UNKNOWN) }
    var validatingApiKey by remember { mutableStateOf(false) }
    val coroutineScope = rememberCoroutineScope()
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(32.dp)
            .verticalScroll(rememberScrollState())
            .testTag("signin_prompt_root"),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(
            "🍊",
            style = CitrosTypography.displayLarge
        )
        Spacer(modifier = Modifier.height(16.dp))
        Text(
            "Welcome to Citros",
            style = CitrosTypography.headlineMedium
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            "AI phone control with cloud and local models",
            style = CitrosTypography.bodyMedium,
            color = CitrosColorScheme.onBackground.copy(alpha = 0.7f)
        )
        Spacer(modifier = Modifier.height(32.dp))
        when (mode) {
            "main" -> {
                // PROMINENT: Device Code Flow - simplest for users
                Button(
                    onClick = {
                        selectedProvider = Provider.OPENAI
                        selectedAuthKind = CloudAuthKind.OPENAI_DEVICE_CODE
                        mode = "device_code"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("🔑 Sign in with OpenAI")
                }
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "No API key needed — sign in with your ChatGPT account",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.5f)
                )
                Spacer(modifier = Modifier.height(20.dp))
                OutlinedButton(
                    onClick = {
                        selectedProvider = Provider.ANTHROPIC
                        selectedAuthKind = CloudAuthKind.ANTHROPIC_CREDENTIAL
                        mode = "token"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("🔐 Anthropic Key / Setup Token")
                }
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "From console.anthropic.com",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.5f)
                )
                Spacer(modifier = Modifier.height(20.dp))
                OutlinedButton(
                    onClick = {
                        selectedProvider = Provider.OPENAI
                        selectedAuthKind = CloudAuthKind.OPENAI_API_KEY
                        mode = "token"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("🤖 OpenAI API Key")
                }
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "From platform.openai.com/api-keys",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.5f)
                )
                Spacer(modifier = Modifier.height(20.dp))
                OutlinedButton(
                    onClick = {
                        selectedProvider = Provider.OPENROUTER
                        selectedAuthKind = CloudAuthKind.OPENROUTER_API_KEY
                        mode = "token"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("🌐 OpenRouter API Key")
                }
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "From openrouter.ai/keys",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.5f)
                )
                Spacer(modifier = Modifier.height(20.dp))
                OutlinedButton(
                    onClick = {
                        selectedProvider = null
                        selectedAuthKind = null
                        mode = "local"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("⚡ Local LLM")
                }
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "llama.cpp or Ollama in Termux",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.5f)
                )
            }
            "token" -> {
                val resolvedProvider = if (inputValue.isNotBlank()) {
                    ProviderConfig.detectProvider(inputValue, selectedProvider)
                } else {
                    selectedProvider
                }
                val tokenType = if (inputValue.isNotBlank()) {
                    if (resolvedProvider == Provider.ANTHROPIC) {
                        ClaudeClient.identifyTokenType(inputValue)
                    } else {
                        null
                    }
                } else null
                val codexOauthSelected = selectedAuthKind == CloudAuthKind.OPENAI_CODEX_OAUTH
                val looksLikeOauth = ProviderConfig.isLikelyOpenAiOauthToken(inputValue)
                val trimmedToken = inputValue.trim()
                val hasToken = trimmedToken.isNotEmpty()
                val isTokenFormatValid = resolvedProvider?.let { provider ->
                    isValidKeyFormat(trimmedToken, provider)
                } ?: false
                val providerLabel = when (selectedAuthKind) {
                    CloudAuthKind.ANTHROPIC_CREDENTIAL -> "Anthropic credential selected"
                    CloudAuthKind.OPENAI_API_KEY -> "OpenAI API key selected"
                    CloudAuthKind.OPENAI_CODEX_OAUTH -> "Codex OAuth selected (experimental)"
                    CloudAuthKind.OPENAI_DEVICE_CODE -> "OpenAI Device Code selected"
                    CloudAuthKind.OPENROUTER_API_KEY -> "OpenRouter API key selected"
                    null -> when (selectedProvider) {
                        Provider.ANTHROPIC -> "Anthropic selected"
                        Provider.OPENAI -> "OpenAI selected"
                        Provider.OPENROUTER -> "OpenRouter selected"
                        null -> "Auto-detect mode"
                    }
                }
                val hint = when {
                    inputValue.isBlank() -> "$providerLabel. Paste credential to continue."
                    codexOauthSelected && !looksLikeOauth ->
                        "⚠️ Expected OAuth token format (sess-*, oauth_*, or oa-*)"
                    codexOauthSelected && looksLikeOauth ->
                        "✅ Codex OAuth token format detected (experimental)"
                    resolvedProvider == Provider.OPENAI &&
                        ProviderConfig.isLikelyOpenAiOauthToken(inputValue) &&
                        !inputValue.startsWith("sk-") -> "✅ OpenAI OAuth token detected"
                    resolvedProvider == Provider.OPENAI -> "✅ OpenAI credential detected (GPT-4o)"
                    resolvedProvider == Provider.OPENROUTER -> "✅ OpenRouter key detected"
                    tokenType == ClaudeClient.TokenType.SETUP_TOKEN -> "✅ Setup token detected (uses subscription)"
                    tokenType == ClaudeClient.TokenType.API_KEY -> "✅ Anthropic API key detected"
                    else -> "⚠️ Unrecognized format for selected provider"
                }
                Text(
                    hint,
                    style = CitrosTypography.bodySmall,
                    color = when {
                        codexOauthSelected && inputValue.isNotBlank() && !looksLikeOauth ->
                            CitrosColorScheme.error
                        resolvedProvider != null ->
                            CitrosColorScheme.secondary
                        inputValue.isNotBlank() -> CitrosColorScheme.error
                        else ->
                            CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                    }
                )
                Spacer(modifier = Modifier.height(8.dp))
                if (hasToken && resolvedProvider != null && !isTokenFormatValid) {
                    Text(
                        "Credential format does not match the selected provider.",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.error
                    )
                }
                Spacer(modifier = Modifier.height(12.dp))
                resolvedProvider?.let { provider ->
                    OutlinedButton(
                        onClick = { onOpenApiKeySetup(provider) },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Set up API Key")
                    }
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "This opens your provider dashboard in Chrome Custom Tabs. Create/copy the key, then return and paste.",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onBackground.copy(alpha = 0.65f)
                    )
                }
                Spacer(modifier = Modifier.height(16.dp))
                var tokenVisible by remember { mutableStateOf(false) }
                OutlinedTextField(
                    value = inputValue,
                    onValueChange = {
                        inputValue = it
                        apiKeyValidationStatus = ApiKeyValidationStatus.UNKNOWN
                    },
                    label = { Text("Token") },
                    placeholder = {
                        Text(
                            when (selectedAuthKind) {
                                CloudAuthKind.ANTHROPIC_CREDENTIAL -> "sk-ant-api... or sk-ant-oat..."
                                CloudAuthKind.OPENAI_API_KEY -> "sk-proj-..."
                                CloudAuthKind.OPENAI_CODEX_OAUTH -> "sess-... or oauth_..."
                                CloudAuthKind.OPENAI_DEVICE_CODE -> "Device code access token"
                                CloudAuthKind.OPENROUTER_API_KEY -> "sk-or-..."
                                null -> when (selectedProvider) {
                                    Provider.ANTHROPIC -> "sk-ant-api... or sk-ant-oat..."
                                    Provider.OPENAI -> "sk-proj-... or OAuth token"
                                    Provider.OPENROUTER -> "sk-or-..."
                                    null -> "Paste token"
                                }
                            }
                        )
                    },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    visualTransformation = if (tokenVisible) {
                        VisualTransformation.None
                    } else {
                        PasswordVisualTransformation()
                    },
                    trailingIcon = {
                        TextButton(onClick = { tokenVisible = !tokenVisible }) {
                            Text(if (tokenVisible) "Hide" else "Show")
                        }
                    }
                )
                Spacer(modifier = Modifier.height(16.dp))
                OutlinedButton(
                    onClick = {
                        val provider = resolvedProvider ?: return@OutlinedButton
                        if (!isTokenFormatValid) return@OutlinedButton
                        validatingApiKey = true
                        coroutineScope.launch {
                            apiKeyValidationStatus = validateApiCredential(trimmedToken, provider)
                            validatingApiKey = false
                        }
                    },
                    enabled = hasToken && resolvedProvider != null && isTokenFormatValid && !validatingApiKey,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text(if (validatingApiKey) "Testing..." else "Test Connection")
                }
                if (apiKeyValidationStatus != ApiKeyValidationStatus.UNKNOWN) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        when (apiKeyValidationStatus) {
                            ApiKeyValidationStatus.VALID -> stringResource(R.string.api_key_status_valid)
                            ApiKeyValidationStatus.INVALID -> stringResource(R.string.api_key_status_invalid)
                            ApiKeyValidationStatus.EXPIRED -> stringResource(R.string.api_key_status_expired)
                            ApiKeyValidationStatus.UNKNOWN -> stringResource(R.string.api_key_status_unknown)
                        },
                        style = CitrosTypography.bodySmall,
                        color = when (apiKeyValidationStatus) {
                            ApiKeyValidationStatus.VALID -> CitrosColorScheme.secondary
                            ApiKeyValidationStatus.INVALID -> CitrosColorScheme.error
                            ApiKeyValidationStatus.EXPIRED -> CitrosColorScheme.error
                            ApiKeyValidationStatus.UNKNOWN -> CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                        }
                    )
                }
                Spacer(modifier = Modifier.height(12.dp))
                Button(
                    onClick = {
                        val provider = resolvedProvider ?: return@Button
                        if (!isValidKeyFormat(trimmedToken, provider)) return@Button
                        onToken(trimmedToken, provider, selectedAuthKind)
                    },
                    enabled = hasToken && resolvedProvider != null && isTokenFormatValid,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Connect")
                }
                Spacer(modifier = Modifier.height(8.dp))
                TextButton(onClick = {
                    mode = "main"
                    inputValue = ""
                    selectedProvider = null
                    selectedAuthKind = null
                }) {
                    Text("Back")
                }
            }
            "codex_oauth" -> {
                Text(
                    "Connect your OpenAI subscription with browser sign-in.",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                )
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    "Bridge Mode",
                    style = CitrosTypography.labelMedium,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                )
                Spacer(modifier = Modifier.height(8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    FilterChip(
                        selected = codexBridgeMode == CodexOauthBridgeMode.EMBEDDED,
                        onClick = { codexBridgeMode = CodexOauthBridgeMode.EMBEDDED },
                        label = { Text("On-device") },
                        enabled = !codexOauthInProgress
                    )
                    FilterChip(
                        selected = codexBridgeMode == CodexOauthBridgeMode.EXTERNAL,
                        onClick = { codexBridgeMode = CodexOauthBridgeMode.EXTERNAL },
                        label = { Text("External URL") },
                        enabled = !codexOauthInProgress
                    )
                }
                Spacer(modifier = Modifier.height(12.dp))
                if (codexBridgeMode == CodexOauthBridgeMode.EMBEDDED) {
                    OutlinedTextField(
                        value = codexAuthUrl,
                        onValueChange = { codexAuthUrl = it },
                        label = { Text("Authorize URL") },
                        placeholder = { Text(EmbeddedCodexOauthBridgeServer.DEFAULT_AUTHORIZE_URL) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = codexTokenUrl,
                        onValueChange = { codexTokenUrl = it },
                        label = { Text("Token URL") },
                        placeholder = { Text(EmbeddedCodexOauthBridgeServer.DEFAULT_TOKEN_URL) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = codexClientId,
                        onValueChange = { codexClientId = it },
                        label = { Text("Client ID") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = codexClientSecret,
                        onValueChange = { codexClientSecret = it },
                        label = { Text("Client Secret (optional)") },
                        visualTransformation = PasswordVisualTransformation(),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = codexScope,
                        onValueChange = { codexScope = it },
                        label = { Text("Scope") },
                        placeholder = { Text(EmbeddedCodexOauthBridgeServer.DEFAULT_SCOPE) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "Starts a local bridge inside the app and exchanges callback code for token.",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onBackground.copy(alpha = 0.6f)
                    )
                } else {
                    OutlinedTextField(
                        value = codexBridgeUrl,
                        onValueChange = { codexBridgeUrl = it },
                        label = { Text("OAuth Bridge URL") },
                        placeholder = { Text(CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "External bridge starts login and exchanges callback code for token.",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onBackground.copy(alpha = 0.6f)
                    )
                }
                if (!codexOauthStatus.isNullOrBlank()) {
                    Spacer(modifier = Modifier.height(12.dp))
                    Text(
                        codexOauthStatus,
                        style = CitrosTypography.bodySmall,
                        color = when {
                            codexOauthStatus.contains("failed", ignoreCase = true) ||
                                codexOauthStatus.contains("could not", ignoreCase = true) ||
                                codexOauthStatus.contains("missing", ignoreCase = true) ||
                                codexOauthStatus.contains("mismatch", ignoreCase = true) ||
                                codexOauthStatus.contains("no active", ignoreCase = true) ->
                                    CitrosColorScheme.error
                            else -> CitrosColorScheme.secondary
                        }
                    )
                }
                Spacer(modifier = Modifier.height(16.dp))
                val codexStartRequest = when (codexBridgeMode) {
                    CodexOauthBridgeMode.EXTERNAL -> {
                        CodexOauthStartRequest(
                            mode = CodexOauthBridgeMode.EXTERNAL,
                            externalBridgeUrl = codexBridgeUrl
                                .trim()
                                .ifBlank { CodexOauthBridgeClient.DEFAULT_BRIDGE_BASE_URL }
                        )
                    }
                    CodexOauthBridgeMode.EMBEDDED -> {
                        CodexOauthStartRequest(
                            mode = CodexOauthBridgeMode.EMBEDDED,
                            embeddedConfig = EmbeddedCodexOauthBridgeServer.Config(
                                authorizeUrl = codexAuthUrl
                                    .trim()
                                    .ifBlank { EmbeddedCodexOauthBridgeServer.DEFAULT_AUTHORIZE_URL },
                                tokenUrl = codexTokenUrl
                                    .trim()
                                    .ifBlank { EmbeddedCodexOauthBridgeServer.DEFAULT_TOKEN_URL },
                                clientId = codexClientId.trim(),
                                clientSecret = codexClientSecret.trim().ifBlank { null },
                                scope = codexScope
                                    .trim()
                                    .ifBlank { EmbeddedCodexOauthBridgeServer.DEFAULT_SCOPE }
                            )
                        )
                    }
                }
                Button(
                    onClick = { onStartCodexOauth(codexStartRequest) },
                    enabled = !codexOauthInProgress,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (codexOauthInProgress) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(16.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        Text("Waiting for browser callback...")
                    } else {
                        Text("Sign in with OpenAI")
                    }
                }
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedButton(
                    onClick = {
                        selectedProvider = Provider.OPENAI
                        selectedAuthKind = CloudAuthKind.OPENAI_CODEX_OAUTH
                        inputValue = ""
                        mode = "token"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Paste Token Manually")
                }
                Spacer(modifier = Modifier.height(8.dp))
                TextButton(onClick = {
                    mode = "main"
                    selectedProvider = null
                    selectedAuthKind = null
                }) {
                    Text("Back")
                }
            }
            "device_code" -> {
                var deviceCodeResponse by remember { mutableStateOf<ai.citros.core.DeviceCodeAuthClient.DeviceCodeResponse?>(null) }
                var deviceCodeStatus by remember { mutableStateOf<String?>(null) }
                var deviceCodeError by remember { mutableStateOf<String?>(null) }
                var deviceCodeDiagnostics by remember { mutableStateOf<String?>(null) }
                var isPolling by remember { mutableStateOf(false) }
                var hasInitialized by remember { mutableStateOf(false) }
                var retryTrigger by remember { mutableStateOf(0) }
                var pollingJob by remember { mutableStateOf<kotlinx.coroutines.Job?>(null) }
                val context = LocalContext.current
                val prefs = remember(context) {
                    context.getSharedPreferences(CITROS_PREFS, android.content.Context.MODE_PRIVATE)
                }
                val secureStore = remember(context) { SecureCredentialStore(context) }
                fun writeSecureBackedValue(key: String, value: String?) =
                    writeSecureBacked(secureStore, prefs, key, value)
                val coroutineScope = rememberCoroutineScope()
                val deviceCodeClient = remember { ai.citros.core.DeviceCodeAuthClient() }
                // Cleanup on disposal - cancel any running polling
                DisposableEffect(Unit) {
                    onDispose {
                        pollingJob?.cancel()
                    }
                }
                LaunchedEffect(retryTrigger) {
                    // Prevent duplicate requests on recomposition
                    if (hasInitialized) return@LaunchedEffect
                    hasInitialized = true
                    deviceCodeStatus = "Requesting authorization code..."
                    deviceCodeError = null
                    deviceCodeDiagnostics = null
                    prefs.edit()
                        .putString(PREF_DEVICE_CODE_LAST_STATUS, deviceCodeStatus)
                        .remove(PREF_DEVICE_CODE_LAST_ERROR)
                        .remove(PREF_DEVICE_CODE_LAST_DIAGNOSTICS)
                        .apply()
                    deviceCodeClient.requestDeviceCode(
                        ai.citros.core.DeviceCodeAuthClient.DEFAULT_CLIENT_ID
                    ).onSuccess { response ->
                        deviceCodeResponse = response
                        deviceCodeStatus = "Waiting for authorization..."
                        deviceCodeError = null
                        deviceCodeDiagnostics = formatDeviceCodeSessionInfo(response)
                        prefs.edit()
                            .putString(PREF_DEVICE_CODE_LAST_STATUS, deviceCodeStatus)
                            .putString(PREF_DEVICE_CODE_LAST_DIAGNOSTICS, deviceCodeDiagnostics)
                            .remove(PREF_DEVICE_CODE_LAST_ERROR)
                            .apply()
                        // Auto-start polling in background, storing Job for cleanup
                        isPolling = true
                        pollingJob = coroutineScope.launch {
                            when (val result = deviceCodeClient.pollForToken(
                                response.deviceAuthId,
                                response.userCode,
                                response.interval
                            )) {
                                is ai.citros.core.DeviceCodeAuthClient.PollResult.Success -> {
                                    // Step 2: Exchange auth code for tokens
                                    deviceCodeStatus = "Completing sign-in..."
                                    deviceCodeDiagnostics = formatDeviceCodeDiagnostics(result.diagnostics)
                                    prefs.edit()
                                        .putString(PREF_DEVICE_CODE_LAST_STATUS, deviceCodeStatus)
                                        .putString(PREF_DEVICE_CODE_LAST_DIAGNOSTICS, deviceCodeDiagnostics)
                                        .remove(PREF_DEVICE_CODE_LAST_ERROR)
                                        .apply()
                                    deviceCodeClient.exchangeCode(
                                        clientId = ai.citros.core.DeviceCodeAuthClient.DEFAULT_CLIENT_ID,
                                        authCode = result.authCode,
                                        codeVerifier = result.codeVerifier,
                                        codeChallenge = result.codeChallenge
                                    ).onSuccess { tokens ->
                                        // Store refresh token separately if present
                                        if (tokens.refreshToken != null) {
                                            writeSecureBackedValue(PREF_DEVICE_CODE_REFRESH_TOKEN, tokens.refreshToken)
                                        }
                                        // onToken callback handles main token persistence
                                        onToken(tokens.accessToken, Provider.OPENAI, CloudAuthKind.OPENAI_DEVICE_CODE)
                                        prefs.edit()
                                            .putString(PREF_DEVICE_CODE_LAST_STATUS, "Authorized")
                                            .remove(PREF_DEVICE_CODE_LAST_ERROR)
                                            .putString(PREF_DEVICE_CODE_LAST_DIAGNOSTICS, deviceCodeDiagnostics)
                                            .apply()
                                        isPolling = false
                                    }.onFailure { error ->
                                        deviceCodeError = "Token exchange failed: ${error.message?.take(TOKEN_PREVIEW_LIMIT) ?: "unknown error"}. Try again or use API key instead."
                                        deviceCodeStatus = null
                                        prefs.edit()
                                            .remove(PREF_DEVICE_CODE_LAST_STATUS)
                                            .putString(PREF_DEVICE_CODE_LAST_ERROR, deviceCodeError)
                                            .putString(PREF_DEVICE_CODE_LAST_DIAGNOSTICS, deviceCodeDiagnostics)
                                            .apply()
                                        isPolling = false
                                    }
                                }
                                is ai.citros.core.DeviceCodeAuthClient.PollResult.Error -> {
                                    deviceCodeError = mapDeviceCodePollError(result.error, result.description)
                                    deviceCodeDiagnostics = formatDeviceCodeDiagnostics(result.diagnostics)
                                    deviceCodeStatus = null
                                    prefs.edit()
                                        .remove(PREF_DEVICE_CODE_LAST_STATUS)
                                        .putString(PREF_DEVICE_CODE_LAST_ERROR, deviceCodeError)
                                        .putString(PREF_DEVICE_CODE_LAST_DIAGNOSTICS, deviceCodeDiagnostics)
                                        .apply()
                                    isPolling = false
                                }
                            }
                        }
                    }.onFailure { error ->
                        deviceCodeError = "Failed to request device code: ${error.message?.take(TOKEN_PREVIEW_LIMIT)}. Check your internet connection and try again."
                        deviceCodeStatus = null
                        deviceCodeDiagnostics = null
                        prefs.edit()
                            .remove(PREF_DEVICE_CODE_LAST_STATUS)
                            .putString(PREF_DEVICE_CODE_LAST_ERROR, deviceCodeError)
                            .remove(PREF_DEVICE_CODE_LAST_DIAGNOSTICS)
                            .apply()
                    }
                }
                Text(
                    "Sign in with your OpenAI account",
                    style = CitrosTypography.bodyMedium,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                )
                Spacer(modifier = Modifier.height(24.dp))
                if (deviceCodeResponse != null) {
                    // Step 1: Open the link
                    Text(
                        "1. Open the link below",
                        style = CitrosTypography.labelLarge,
                        color = CitrosColorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    Button(
                        onClick = {
                            val uri = Uri.parse(
                                deviceCodeResponse?.verificationUri
                            )
                            context.startActivity(Intent(Intent.ACTION_VIEW, uri))
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        CitrosIcon(CitrosIcons.ExitToApp, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text("Open Browser")
                    }
                    Spacer(modifier = Modifier.height(24.dp))
                    // Step 2: Enter the code
                    Text(
                        "2. Enter this code",
                        style = CitrosTypography.labelLarge,
                        color = CitrosColorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = CitrosColorScheme.primaryContainer,
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            deviceCodeResponse?.userCode ?: "",
                            style = CitrosTypography.displayMedium,
                            color = CitrosColorScheme.onPrimaryContainer,
                            modifier = Modifier.padding(24.dp)
                        )
                    }
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = {
                            val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                            val clip = android.content.ClipData.newPlainText("User Code", deviceCodeResponse?.userCode)
                            clipboard.setPrimaryClip(clip)
                            android.widget.Toast.makeText(context, "Code copied", android.widget.Toast.LENGTH_SHORT).show()
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("📋 Copy Code")
                    }
                    Spacer(modifier = Modifier.height(24.dp))
                    // Step 3: Approve access
                    Text(
                        "3. Approve access",
                        style = CitrosTypography.labelLarge,
                        color = CitrosColorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "Then return to Citros",
                        style = CitrosTypography.bodySmall,
                        color = CitrosColorScheme.onBackground.copy(alpha = 0.6f)
                    )
                    Spacer(modifier = Modifier.height(24.dp))
                    // Polling status
                    if (isPolling && deviceCodeStatus != null) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.Center
                        ) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                strokeWidth = 2.dp
                            )
                            Spacer(modifier = Modifier.width(12.dp))
                            Text(
                                deviceCodeStatus ?: "",
                                style = CitrosTypography.bodyMedium,
                                color = CitrosColorScheme.secondary
                            )
                        }
                    }
                } else if (deviceCodeStatus != null) {
                    // Loading state (before device code arrives)
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.Center
                    ) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp
                        )
                        Spacer(modifier = Modifier.width(12.dp))
                        Text(
                            deviceCodeStatus ?: "",
                            style = CitrosTypography.bodyMedium
                        )
                    }
                }
                if (deviceCodeDiagnostics != null) {
                    Spacer(modifier = Modifier.height(12.dp))
                    Surface(
                        shape = RoundedCornerShape(8.dp),
                        color = CitrosColorScheme.surfaceVariant,
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            "Diagnostics: ${deviceCodeDiagnostics ?: ""}",
                            style = CitrosTypography.bodySmall,
                            color = CitrosColorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(12.dp)
                        )
                    }
                }
                // Error display
                if (deviceCodeError != null) {
                    Spacer(modifier = Modifier.height(16.dp))
                    Surface(
                        shape = RoundedCornerShape(8.dp),
                        color = CitrosColorScheme.errorContainer,
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Column(modifier = Modifier.padding(16.dp)) {
                            Text(
                                deviceCodeError ?: "",
                                style = CitrosTypography.bodyMedium,
                                color = CitrosColorScheme.onErrorContainer
                            )
                            Spacer(modifier = Modifier.height(12.dp))
                            Button(
                                onClick = {
                                    // Retry by canceling current poll and resetting state
                                    pollingJob?.cancel()
                                    pollingJob = null
                                    hasInitialized = false
                                    retryTrigger++
                                    deviceCodeResponse = null
                                    deviceCodeStatus = null
                                    deviceCodeError = null
                                    deviceCodeDiagnostics = null
                                    prefs.edit()
                                        .remove(PREF_DEVICE_CODE_LAST_STATUS)
                                        .remove(PREF_DEVICE_CODE_LAST_ERROR)
                                        .remove(PREF_DEVICE_CODE_LAST_DIAGNOSTICS)
                                        .apply()
                                    isPolling = false
                                },
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Text("Try Again")
                            }
                        }
                    }
                }
                Spacer(modifier = Modifier.height(16.dp))
                TextButton(
                    onClick = {
                        // Cancel any running polling before navigating away
                        pollingJob?.cancel()
                        pollingJob = null
                        mode = "main"
                        hasInitialized = false
                        deviceCodeResponse = null
                        deviceCodeStatus = null
                        deviceCodeError = null
                        deviceCodeDiagnostics = null
                        prefs.edit()
                            .remove(PREF_DEVICE_CODE_LAST_STATUS)
                            .remove(PREF_DEVICE_CODE_LAST_ERROR)
                            .remove(PREF_DEVICE_CODE_LAST_DIAGNOSTICS)
                            .apply()
                        isPolling = false
                    }
                ) {
                    Text("Back")
                }
            }
            "local" -> {
                Text(
                    "Connect to llama.cpp or Ollama in Termux",
                    style = CitrosTypography.bodySmall,
                    color = CitrosColorScheme.onBackground.copy(alpha = 0.7f)
                )
                Spacer(modifier = Modifier.height(16.dp))
                OutlinedTextField(
                    value = inputValue.ifBlank { "http://localhost:8080" },
                    onValueChange = { inputValue = it },
                    label = { Text("Server URL") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedTextField(
                    value = modelName,
                    onValueChange = { modelName = it },
                    label = { Text("Model") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(16.dp))
                Button(
                    onClick = {
                        val url = inputValue.ifBlank { "http://localhost:8080" }
                        onLocalLLM(url, modelName)
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("⚡ Connect")
                }
                Spacer(modifier = Modifier.height(8.dp))
                TextButton(onClick = {
                    mode = "main"
                    inputValue = ""
                    selectedProvider = null
                    selectedAuthKind = null
                }) {
                    Text("Back")
                }
            }
        }
    }
}
@Composable
internal fun MessageBubble(
    message: Message,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE
) {
    PortedMessageBubble(message = message, flavor = flavor)
}
@Composable
internal fun LoadingIndicator(flavor: CitrosFlavor = CitrosFlavor.TANGERINE, label: String = "Thinking") {
    PortedLoadingIndicator(flavor = flavor, label = label)
}

internal data class MessageInputSubmissionState(
    val text: String,
    val pendingStopVisual: Boolean
)

internal data class VoiceStartRetryState(
    val lastConsumedToken: Int = 0,
    val pendingToken: Int = 0,
    val permissionRequestedToken: Int = 0
)

internal enum class VoiceStartRetryAction {
    NONE,
    REQUEST_PERMISSION,
    START_LISTENING
}

internal data class VoiceStartRetryResolution(
    val state: VoiceStartRetryState,
    val action: VoiceStartRetryAction
)

internal fun resolveVoiceStartRetry(
    state: VoiceStartRetryState,
    incomingToken: Int,
    isLoading: Boolean,
    isListening: Boolean,
    hasVoiceManager: Boolean,
    hasActiveStt: Boolean,
    hasMicPermission: Boolean
): VoiceStartRetryResolution {
    var next = state
    if (incomingToken > next.lastConsumedToken && incomingToken > next.pendingToken) {
        next = next.copy(pendingToken = incomingToken)
    }
    if (next.pendingToken <= next.lastConsumedToken) {
        return VoiceStartRetryResolution(next, VoiceStartRetryAction.NONE)
    }
    if (isLoading || isListening || !hasVoiceManager || !hasActiveStt) {
        return VoiceStartRetryResolution(next, VoiceStartRetryAction.NONE)
    }
    if (hasMicPermission) {
        return VoiceStartRetryResolution(
            next.copy(
                lastConsumedToken = next.pendingToken,
                pendingToken = 0,
                permissionRequestedToken = 0
            ),
            VoiceStartRetryAction.START_LISTENING
        )
    }
    if (next.permissionRequestedToken < next.pendingToken) {
        return VoiceStartRetryResolution(
            next.copy(permissionRequestedToken = next.pendingToken),
            VoiceStartRetryAction.REQUEST_PERMISSION
        )
    }
    return VoiceStartRetryResolution(next, VoiceStartRetryAction.NONE)
}

internal fun applyVoiceStartPermissionResult(
    state: VoiceStartRetryState,
    granted: Boolean
): VoiceStartRetryState = if (granted) {
    state.copy(permissionRequestedToken = 0)
} else {
    state
}

internal data class VoiceStartPermissionHandlingResult(
    val state: VoiceStartRetryState,
    val shouldBeginListeningNow: Boolean
)

internal fun handleVoiceStartPermissionResult(
    state: VoiceStartRetryState,
    granted: Boolean,
    manualPermissionRequestInFlight: Boolean,
    hasActiveStt: Boolean
): VoiceStartPermissionHandlingResult {
    var next = applyVoiceStartPermissionResult(state, granted)
    if (!granted) {
        return VoiceStartPermissionHandlingResult(
            state = next,
            shouldBeginListeningNow = false
        )
    }
    if (manualPermissionRequestInFlight && hasActiveStt) {
        if (next.pendingToken > next.lastConsumedToken) {
            next = next.copy(
                lastConsumedToken = next.pendingToken,
                pendingToken = 0
            )
        }
        return VoiceStartPermissionHandlingResult(
            state = next,
            shouldBeginListeningNow = true
        )
    }
    return VoiceStartPermissionHandlingResult(
        state = next,
        shouldBeginListeningNow = false
    )
}

internal data class VoiceStartActionDispatchResult(
    val state: VoiceStartRetryState,
    val shouldBeginListening: Boolean,
    val shouldRequestPermission: Boolean
)

internal fun dispatchVoiceStartRetryAction(
    state: VoiceStartRetryState,
    action: VoiceStartRetryAction,
    hasActiveSttAtLaunch: Boolean
): VoiceStartActionDispatchResult = when (action) {
    VoiceStartRetryAction.START_LISTENING -> {
        if (hasActiveSttAtLaunch) {
            VoiceStartActionDispatchResult(
                state = state,
                shouldBeginListening = true,
                shouldRequestPermission = false
            )
        } else {
            val tokenToRetry = maxOf(state.pendingToken, state.lastConsumedToken)
            VoiceStartActionDispatchResult(
                state = state.copy(
                    pendingToken = tokenToRetry,
                    // Keep retry arithmetic bounded for the zero-state case (0 -> 0) while
                    // still reopening the gate for non-zero tokens (n -> n-1).
                    lastConsumedToken = (tokenToRetry - 1).coerceAtLeast(0)
                ),
                shouldBeginListening = false,
                shouldRequestPermission = false
            )
        }
    }
    VoiceStartRetryAction.REQUEST_PERMISSION -> VoiceStartActionDispatchResult(
        state = state,
        shouldBeginListening = false,
        shouldRequestPermission = true
    )
    VoiceStartRetryAction.NONE -> VoiceStartActionDispatchResult(
        state = state,
        shouldBeginListening = false,
        shouldRequestPermission = false
    )
}

internal fun submitMessageDraft(
    attemptedText: String,
    isLoading: Boolean,
    onSend: (String) -> Boolean,
    onQueue: (String) -> Boolean
): MessageInputSubmissionState {
    val submitted = if (isLoading) onQueue(attemptedText) else onSend(attemptedText)
    return if (submitted) {
        MessageInputSubmissionState(text = "", pendingStopVisual = true)
    } else {
        MessageInputSubmissionState(text = attemptedText, pendingStopVisual = false)
    }
}

@Composable
internal fun MessageInput(
    onSend: (String) -> Boolean,
    onSteer: (String) -> Boolean = onSend,
    onQueue: (String) -> Boolean = onSteer,
    queuedMessage: String? = null,
    onSteerQueuedMessage: (String) -> Unit = { onSteer(it) },
    onCancel: () -> Unit = {},
    isLoading: Boolean = false,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    modifier: Modifier = Modifier,
    placeholder: String = "Message Citros...",
    voiceReady: Boolean = false,
    voiceManager: VoiceManager? = null,
    startListeningToken: Int = 0
) {
    var text by rememberSaveable { mutableStateOf("") }
    var pendingStopVisual by remember { mutableStateOf(false) }
    var isListening by remember { mutableStateOf(false) }
    var listeningJob by remember { mutableStateOf<Job?>(null) }
    var lastConsumedListeningToken by rememberSaveable { mutableIntStateOf(0) }
    var pendingListeningToken by rememberSaveable { mutableIntStateOf(0) }
    var permissionRequestedToken by rememberSaveable { mutableIntStateOf(0) }
    var manualPermissionRequestInFlight by remember { mutableStateOf(false) }
    var voiceStartRetryTrigger by remember { mutableIntStateOf(0) }
    val coroutineScope = rememberCoroutineScope()
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val activeStt = voiceManager?.activeStt?.collectAsState()?.value
    LaunchedEffect(pendingStopVisual, isLoading) {
        if (pendingStopVisual && isLoading) {
            pendingStopVisual = false
            return@LaunchedEffect
        }
        if (pendingStopVisual && !isLoading) {
            delay(180)
            if (!isLoading) {
                pendingStopVisual = false
            }
        }
    }
    fun beginListening(stt: SpeechToTextProvider) {
        // Cancel any previous listening session to avoid two AudioRecord
        // instances fighting over the microphone (#637).
        val previousJob = listeningJob
        stt.stopListening()
        val prefix = text  // Preserve existing text in the input field (#637)
        isListening = true
        listeningJob = coroutineScope.launch {
            // Wait for old AudioRecord cleanup to complete before creating
            // a new one. cancel() is async — the old job's finally block
            // (which calls audioRecord.stop()/release()) may not have run yet.
            previousJob?.cancelAndJoin()
            val accumulator = VoiceAccumulator(prefix)
            stt.startListening().collect { event ->
                when (event) {
                    is SpeechEvent.Error -> {
                        isListening = false
                        val err = event.error
                        val errorMsg = when (err) {
                            is SpeechError.PermissionDenied -> err.message
                            is SpeechError.Unavailable -> err.message
                            is SpeechError.Timeout -> err.message
                            is SpeechError.EngineError -> err.message
                            is SpeechError.NetworkError -> err.message
                        }
                        Toast.makeText(context, errorMsg, Toast.LENGTH_SHORT).show()
                    }
                    else -> {
                        accumulator.onEvent(event)?.let { display -> text = display }
                    }
                }
            }
            // Flow completed naturally (timeout or provider stopped).
            // Auto-send the complete accumulated transcription if enabled.
            isListening = false
            val result = accumulator.finish(
                autoSend = voiceManager?.autoSendAfterVoice?.value == true
            )
            val sendText = result.autoSendText
            if (sendText != null) {
                val submitState = submitMessageDraft(
                    attemptedText = sendText,
                    isLoading = isLoading,
                    onSend = onSend,
                    onQueue = onQueue
                )
                text = submitState.text
                pendingStopVisual = submitState.pendingStopVisual
            } else {
                text = result.displayText
            }
        }
    }
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                voiceStartRetryTrigger += 1
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        val permissionHandling = handleVoiceStartPermissionResult(
            state = VoiceStartRetryState(
                lastConsumedToken = lastConsumedListeningToken,
                pendingToken = pendingListeningToken,
                permissionRequestedToken = permissionRequestedToken
            ),
            granted = granted,
            manualPermissionRequestInFlight = manualPermissionRequestInFlight,
            hasActiveStt = activeStt != null
        )
        lastConsumedListeningToken = permissionHandling.state.lastConsumedToken
        pendingListeningToken = permissionHandling.state.pendingToken
        permissionRequestedToken = permissionHandling.state.permissionRequestedToken
        if (permissionHandling.shouldBeginListeningNow) {
            activeStt?.let(::beginListening)
        } else if (!granted) {
            Toast.makeText(context, "Microphone permission is required for voice input", Toast.LENGTH_SHORT).show()
        }
        manualPermissionRequestInFlight = false
        voiceStartRetryTrigger += 1
    }
    val startListening = {
        val stt = activeStt
        if (stt != null) {
            val hasPermission = ContextCompat.checkSelfPermission(
                context, Manifest.permission.RECORD_AUDIO
            ) == PackageManager.PERMISSION_GRANTED
            if (hasPermission) {
                beginListening(stt)
            } else {
                manualPermissionRequestInFlight = true
                permissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            }
            true
        } else {
            false
        }
    }
    LaunchedEffect(
        startListeningToken,
        voiceManager,
        activeStt,
        isLoading,
        isListening,
        voiceStartRetryTrigger
    ) {
        val hasMicPermission = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.RECORD_AUDIO
        ) == PackageManager.PERMISSION_GRANTED
        val resolution = resolveVoiceStartRetry(
            state = VoiceStartRetryState(
                lastConsumedToken = lastConsumedListeningToken,
                pendingToken = pendingListeningToken,
                permissionRequestedToken = permissionRequestedToken
            ),
            incomingToken = startListeningToken,
            isLoading = isLoading,
            isListening = isListening,
            hasVoiceManager = voiceManager != null,
            hasActiveStt = activeStt != null,
            hasMicPermission = hasMicPermission
        )
        lastConsumedListeningToken = resolution.state.lastConsumedToken
        pendingListeningToken = resolution.state.pendingToken
        permissionRequestedToken = resolution.state.permissionRequestedToken
        val dispatch = dispatchVoiceStartRetryAction(
            state = VoiceStartRetryState(
                lastConsumedToken = lastConsumedListeningToken,
                pendingToken = pendingListeningToken,
                permissionRequestedToken = permissionRequestedToken
            ),
            action = resolution.action,
            hasActiveSttAtLaunch = activeStt != null
        )
        lastConsumedListeningToken = dispatch.state.lastConsumedToken
        pendingListeningToken = dispatch.state.pendingToken
        permissionRequestedToken = dispatch.state.permissionRequestedToken
        if (dispatch.shouldBeginListening) {
            activeStt?.let(::beginListening)
        }
        if (dispatch.shouldRequestPermission) {
            permissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val queuedText = queuedMessage?.trim()?.takeIf { it.isNotBlank() }
    val hasInputText = text.isNotBlank()
    val activeSendButtonColor = if (flavor == CitrosFlavor.NONE) {
        if (isDarkTheme) Color.White else Color.Black
    } else {
        flavor.primary
    }
    val activeSendIconTint = contrastOn(activeSendButtonColor)
    val inactiveSendButtonColor = if (isDarkTheme) surfaces.surface3 else surfaces.surface2
    val inactiveSendIconTint = surfaces.labelQuaternary
    val textFieldColors = OutlinedTextFieldDefaults.colors(
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
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        if (queuedText != null) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                color = surfaces.surface2,
                border = BorderStroke(1.dp, surfaces.separatorLight)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Text(
                        text = queuedText,
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                    Text(
                        text = "Steer",
                        style = CitrosTypography.labelSmall,
                        color = flavor.primary,
                        modifier = Modifier
                            .testTag(TEST_TAG_MESSAGE_STEER_QUEUED_BUTTON)
                            .clickable {
                            onSteerQueuedMessage(queuedText)
                        }
                    )
                }
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Surface(
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(22.dp),
                color = surfaces.surface2
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(start = 2.dp, end = 6.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    OutlinedTextField(
                        value = text,
                        onValueChange = { text = it },
                        modifier = Modifier
                            .weight(1f)
                            .testTag(TEST_TAG_MESSAGE_INPUT_FIELD)
                            .heightIn(max = 132.dp),
                        placeholder = {
                            Text(
                                text = placeholder,
                                style = CitrosTypography.bodyLarge
                            )
                        },
                        enabled = true,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                        keyboardActions = KeyboardActions(
                            onSend = {
                                if (text.isNotBlank()) {
                                    val submitState = submitMessageDraft(
                                        attemptedText = text,
                                        isLoading = isLoading,
                                        onSend = onSend,
                                        onQueue = onQueue
                                    )
                                    text = submitState.text
                                    pendingStopVisual = submitState.pendingStopVisual
                                }
                            }
                        ),
                        singleLine = false,
                        maxLines = 6,
                        centerSingleLineContentWhenMultiline = true,
                        textStyle = CitrosTypography.bodyLarge,
                        shape = RoundedCornerShape(18.dp),
                        colors = textFieldColors
                    )
                    Box(
                        modifier = Modifier
                            .size(32.dp)
                            .clip(CircleShape)
                            .background(
                                when {
                                    hasInputText -> surfaces.surface3
                                    isListening -> surfaces.red.copy(alpha = 0.18f)
                                    else -> Color.Transparent
                                }
                            )
                            .clickable(
                                enabled = hasInputText || isListening || (!isLoading && voiceReady),
                                onClick = {
                                    when {
                                        hasInputText -> {
                                            text = ""
                                        }
                                        isListening -> {
                                            listeningJob?.cancel()
                                            listeningJob = null
                                            activeStt?.stopListening()
                                            isListening = false
                                        }
                                        else -> {
                                            startListening()
                                        }
                                    }
                                }
                            ),
                        contentAlignment = Alignment.Center
                    ) {
                        val micTint = when {
                            isListening -> surfaces.red
                            !hasInputText && !isLoading && voiceReady -> surfaces.labelSecondary.copy(alpha = 0.92f)
                            else -> surfaces.labelSecondary.copy(alpha = 0.90f)
                        }
                        if (hasInputText) {
                            MessageInputClearGlyph(
                                tint = surfaces.labelSecondary.copy(alpha = 0.92f),
                                modifier = Modifier.size(13.dp)
                            )
                        } else if (isListening) {
                            Box(
                                modifier = Modifier
                                    .size(10.dp)
                                    .clip(RoundedCornerShape(3.dp))
                                    .background(micTint)
                            )
                        } else {
                            MessageInputMicGlyph(
                                tint = micTint,
                                modifier = Modifier.size(16.dp)
                            )
                        }
                    }
                }
            }
            val showStopButton = (isLoading || pendingStopVisual) && !hasInputText && !isListening
            val sendEnabled = hasInputText || showStopButton
            MessageInputGlassIconButton(
                modifier = Modifier.testTag(TEST_TAG_MESSAGE_SEND_BUTTON),
                onClick = {
                    when {
                        showStopButton -> {
                            pendingStopVisual = false
                            onCancel()
                        }
                        text.isNotBlank() -> {
                            val submitState = submitMessageDraft(
                                attemptedText = text,
                                isLoading = isLoading,
                                onSend = onSend,
                                onQueue = onQueue
                            )
                            text = submitState.text
                            pendingStopVisual = submitState.pendingStopVisual
                        }
                    }
                },
                enabled = sendEnabled,
                backgroundColor = if (hasInputText) activeSendButtonColor else inactiveSendButtonColor,
                iconTint = when {
                    showStopButton -> surfaces.labelPrimary
                    hasInputText -> activeSendIconTint
                    else -> inactiveSendIconTint
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
@Composable
internal fun MessageInputMicGlyph(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val w = size.width
        val h = size.height
        val stroke = size.minDimension * 0.11f
        val bodyWidth = w * 0.36f
        val bodyHeight = h * 0.48f
        val bodyLeft = (w - bodyWidth) / 2f
        val bodyTop = h * 0.15f
        val bodyRadius = bodyWidth * 0.38f

        drawRoundRect(
            color = tint,
            topLeft = Offset(bodyLeft, bodyTop),
            size = androidx.compose.ui.geometry.Size(bodyWidth, bodyHeight),
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(bodyRadius, bodyRadius),
            style = Stroke(width = stroke)
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.26f, h * 0.58f),
            end = Offset(w * 0.74f, h * 0.58f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.50f, h * 0.63f),
            end = Offset(w * 0.50f, h * 0.84f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.34f, h * 0.84f),
            end = Offset(w * 0.66f, h * 0.84f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
    }
}
@Composable
internal fun MessageInputArrowGlyph(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val w = size.width
        val h = size.height
        val stroke = size.minDimension * 0.12f
        val tip = Offset(w * 0.50f, h * 0.18f)
        val left = Offset(w * 0.30f, h * 0.40f)
        val right = Offset(w * 0.70f, h * 0.40f)
        drawLine(
            color = tint,
            start = Offset(w * 0.50f, h * 0.82f),
            end = tip,
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = tip,
            end = left,
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = tip,
            end = right,
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
    }
}

@Composable
internal fun MessageInputClearGlyph(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val stroke = size.minDimension * 0.20f
        drawLine(
            color = tint,
            start = Offset(size.width * 0.24f, size.height * 0.24f),
            end = Offset(size.width * 0.76f, size.height * 0.76f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.76f, size.height * 0.24f),
            end = Offset(size.width * 0.24f, size.height * 0.76f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
    }
}
@Composable
internal fun MessageInputGlassIconButton(
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
    enabled: Boolean,
    backgroundColor: Color,
    iconTint: Color,
    contentDescription: String? = null,
    content: @Composable (Color) -> Unit
) {
    val resolvedIconTint = if (enabled) iconTint else iconTint.copy(alpha = 0.55f)
    Box(
        modifier = modifier
            .size(40.dp)
            .clip(CircleShape)
            .background(backgroundColor)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics {
                contentDescription?.let { this.contentDescription = it }
            },
        contentAlignment = Alignment.Center
    ) {
        content(resolvedIconTint)
    }
}
