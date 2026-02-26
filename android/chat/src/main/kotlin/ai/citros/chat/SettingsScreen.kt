package ai.citros.chat

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.annotation.StringRes
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import ai.citros.core.KeyHealth
import ai.citros.core.ModelConfig
import ai.citros.core.Provider
import ai.citros.core.ProviderConfig
import ai.citros.core.WalletKey
import ai.citros.core.WalletManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.SocketTimeoutException
import java.net.URL
import java.net.UnknownHostException

private val SpacingXs = 8.dp
private val SpacingSm = 12.dp
private val SpacingMd = 16.dp
private val CardShape = RoundedCornerShape(16.dp)
private const val CONNECT_TIMEOUT_MS = 8_000
private const val READ_TIMEOUT_MS = 8_000
private const val EXPIRY_WARNING_THRESHOLD_MS = 7 * 24 * 60 * 60 * 1000L

// KeyHealth is now in ai.citros.core.KeyHealth

internal fun maskApiKey(raw: String?): String {
    if (raw.isNullOrBlank()) return "••••••"
    if (raw.length <= 8) return "••••"
    return "${raw.take(6)}...${raw.takeLast(4)}"
}

internal fun defaultLabelFor(provider: Provider): String = "${ProviderUi.displayName(provider)} Key"

private fun providerAccent(provider: Provider): Color = ProviderUi.brandColor(provider)

private fun providerGlyph(provider: Provider): String = ProviderUi.icon(provider)

private data class ConnectionTestResult(
    val health: KeyHealth,
    @StringRes val messageRes: Int
)

private class PrefixMaskVisualTransformation(
    private val visiblePrefix: Int = 8,
    private val maskChar: Char = '•'
) : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val raw = text.text
        if (raw.isEmpty() || raw.length <= visiblePrefix) {
            return TransformedText(AnnotatedString(raw), OffsetMapping.Identity)
        }
        val masked = raw.take(visiblePrefix) + maskChar.toString().repeat(raw.length - visiblePrefix)
        return TransformedText(AnnotatedString(masked), OffsetMapping.Identity)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    walletManager: WalletManager,
    keyStore: ai.citros.core.KeyStore,
    onBack: () -> Unit,
    viewModel: ChatViewModel = viewModel()
) {
    var walletState by remember { mutableStateOf(walletManager.loadOrDefault()) }
    var showAddSheet by remember { mutableStateOf(false) }
    var keyToDelete by remember { mutableStateOf<WalletKey?>(null) }
    val health = remember { mutableStateMapOf<String, KeyHealth>() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val snackbarHostState = remember { SnackbarHostState() }
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val visuals = remember(flavor, isDarkTheme) {
        citrosSplashVisualTokens(flavor, isDark = isDarkTheme)
    }
    val backdropScrim = if (isDarkTheme) {
        Color.Black.copy(alpha = 0.44f)
    } else {
        MaterialTheme.colorScheme.surface.copy(alpha = 0.58f)
    }

    fun refreshAndReconfigure() {
        walletState = walletManager.loadOrDefault()
        viewModel.configureWithWallet(walletManager)
    }

    fun testAndStoreHealth(key: WalletKey) {
        val rawKey = keyStore.get(key.id) ?: return
        scope.launch {
            val result = testConnection(key.provider, rawKey)
            health[key.id] = result.health
        }
    }

    Scaffold(
        containerColor = Color.Transparent,
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
        floatingActionButton = {
            CitrosLiquidGlassSurface(
                modifier = Modifier.size(56.dp),
                shape = CircleShape,
                onClick = { showAddSheet = true },
                borderColor = flavor.primary.copy(alpha = 0.44f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 1.08f
            ) {
                Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Icon(
                        Icons.Default.Add,
                        contentDescription = stringResource(R.string.wallet_add_key),
                        tint = flavor.primary.copy(alpha = 0.98f)
                    )
                }
            }
        }
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            CitrosHeroShaderSphere(
                flavor = flavor,
                modifier = Modifier.fillMaxSize()
            )
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(backdropScrim)
            )

            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = SpacingMd),
                verticalArrangement = Arrangement.spacedBy(SpacingSm)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    CitrosLiquidGlassSurface(
                        modifier = Modifier
                            .size(40.dp),
                        shape = RoundedCornerShape(999.dp),
                        onClick = onBack,
                        borderColor = flavor.primary.copy(alpha = 0.44f),
                        borderWidth = 1.dp,
                        highlightColor = flavor.primary,
                        warmth = 1.02f
                    ) {
                        Box(
                            modifier = Modifier.fillMaxSize(),
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                imageVector = Icons.Default.ArrowBack,
                                contentDescription = stringResource(R.string.common_back),
                                tint = flavor.primary.copy(alpha = 0.96f)
                            )
                        }
                    }
                    Text(
                        text = stringResource(R.string.wallet_api_keys),
                        style = MaterialTheme.typography.headlineSmall.copy(
                            shadow = Shadow(
                                color = visuals.hero.deep.copy(alpha = 0.70f),
                                offset = Offset(0f, 2f),
                                blurRadius = 14f
                            )
                        ),
                        color = flavor.primary,
                        fontWeight = FontWeight.SemiBold
                    )
                    Spacer(modifier = Modifier.size(40.dp))
                }

                Text(
                    text = "Manage provider keys and model defaults",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
                )

                if (walletState.keys.isEmpty()) {
                    CitrosLiquidGlassSurface(
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f),
                        shape = RoundedCornerShape(20.dp),
                        borderColor = flavor.primary.copy(alpha = 0.34f),
                        borderWidth = 1.dp,
                        highlightColor = flavor.primary,
                        warmth = 0.98f,
                        contentPadding = PaddingValues(SpacingMd)
                    ) {
                        Column(
                            modifier = Modifier.fillMaxSize(),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center
                        ) {
                            Text(
                                stringResource(R.string.wallet_add_first_key),
                                color = flavor.primary.copy(alpha = 0.92f),
                                style = MaterialTheme.typography.titleMedium
                            )
                            Spacer(Modifier.height(SpacingXs))
                            Text(
                                stringResource(R.string.wallet_api_keys_provider_connect),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.74f)
                            )
                        }
                    }
                } else {
                    LazyColumn(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(SpacingXs),
                        contentPadding = PaddingValues(bottom = 4.dp)
                    ) {
                        items(walletState.keys, key = { it.id }) { key ->
                            val isActive = walletState.activeKeyId == key.id
                            val dismissState = rememberSwipeToDismissBoxState(
                                confirmValueChange = {
                                    if (it == SwipeToDismissBoxValue.EndToStart) keyToDelete = key
                                    false
                                }
                            )

                            SwipeToDismissBox(
                                state = dismissState,
                                enableDismissFromStartToEnd = false,
                                backgroundContent = {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxSize()
                                            .background(Color(0xAA4A0B0B), CardShape)
                                            .padding(start = SpacingMd, end = SpacingXs),
                                        horizontalArrangement = Arrangement.End,
                                        verticalAlignment = Alignment.CenterVertically
                                    ) {
                                        Icon(
                                            Icons.Default.Delete,
                                            contentDescription = stringResource(R.string.common_delete),
                                            tint = Color(0xFFFF8A8A)
                                        )
                                    }
                                }
                            ) {
                                WalletKeyCard(
                                    key = key,
                                    maskedKey = maskApiKey(keyStore.get(key.id)),
                                    isActive = isActive,
                                    health = health[key.id] ?: if (isActive) KeyHealth.VALID else KeyHealth.UNKNOWN,
                                    flavor = flavor,
                                    onTap = {
                                        walletManager.setActiveKey(key.id)
                                        refreshAndReconfigure()
                                        testAndStoreHealth(key)
                                    }
                                )
                            }
                        }
                    }
                }

                val activeProvider = walletState.keys.find { it.id == walletState.activeKeyId }?.provider
                val activeConfig = remember(walletState, keyStore) {
                    walletState.activeConfig(keyStore)
                }
                val modelCatalogRefreshTick = rememberModelCatalogRefreshTick(
                    activeConfig = activeConfig,
                    extraKey = walletState.activeKeyId,
                    logTag = "SettingsScreen"
                )

                if (activeProvider != null) {
                    val chatModels = remember(activeProvider, modelCatalogRefreshTick) {
                        ModelConfig.runtimeChatModels(activeProvider)
                    }
                    val actionModels = remember(activeProvider, modelCatalogRefreshTick) {
                        ModelConfig.runtimeActionModels(activeProvider)
                    }

                    LaunchedEffect(
                        activeProvider,
                        chatModels,
                        actionModels,
                        walletState.chatModelId,
                        walletState.actionModelId
                    ) {
                        val correction = computeRuntimeModelSelectionCorrection(
                            provider = activeProvider,
                            selectedChatModelId = walletState.chatModelId,
                            selectedActionModelId = walletState.actionModelId,
                            chatModels = chatModels,
                            actionModels = actionModels
                        )

                        var changed = false
                        if (correction.chatModelId != walletState.chatModelId) {
                            walletManager.setChatModel(correction.chatModelId)
                            changed = true
                        }
                        if (correction.actionModelId != walletState.actionModelId) {
                            walletManager.setActionModel(correction.actionModelId)
                            changed = true
                        }

                        if (changed) {
                            refreshAndReconfigure()
                            correction.notices.forEach { notice ->
                                snackbarHostState.showSnackbar(notice)
                            }
                        }
                    }

                    ModelSelectionSection(
                        activeProvider = activeProvider,
                        chatModelId = walletState.chatModelId,
                        actionModelId = walletState.actionModelId,
                        chatModels = chatModels,
                        actionModels = actionModels,
                        flavor = flavor,
                        onChatChange = {
                            walletManager.setChatModel(it)
                            refreshAndReconfigure()
                        },
                        onActionChange = {
                            walletManager.setActionModel(it)
                            refreshAndReconfigure()
                        }
                    )
                }

                Text(
                    stringResource(R.string.wallet_base_super_coming_soon),
                    style = MaterialTheme.typography.bodySmall,
                    color = flavor.primary.copy(alpha = 0.72f)
                )
                Spacer(Modifier.height(68.dp))
            }
        }
    }

    if (showAddSheet) {
        AddKeyBottomSheet(
            flavor = flavor,
            onDismiss = { showAddSheet = false },
            onTested = { keyHealth, provider, rawKey ->
                if (walletState.activeKeyId != null) {
                    val active = walletState.keys.find { it.id == walletState.activeKeyId }
                    if (active != null && active.provider == provider && keyStore.get(active.id) == rawKey) {
                        health[active.id] = keyHealth
                    }
                }
            },
            onSave = { provider, label, apiKey ->
                val created = walletManager.addKey(provider, label, apiKey)
                walletManager.setActiveKey(created.id)
                refreshAndReconfigure()
                scope.launch {
                    val result = testConnection(provider, apiKey)
                    health[created.id] = result.health
                }
                showAddSheet = false
            }
        )
    }

    keyToDelete?.let { doomed ->
        AlertDialog(
            onDismissRequest = { keyToDelete = null },
            containerColor = if (isDarkTheme) {
                Color(0xE6070709)
            } else {
                MaterialTheme.colorScheme.surface.copy(alpha = 0.96f)
            },
            titleContentColor = flavor.primary.copy(alpha = 0.95f),
            textContentColor = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f),
            confirmButton = {
                TextButton(onClick = {
                    walletManager.removeKey(doomed.id)
                    health.remove(doomed.id)
                    keyToDelete = null
                    refreshAndReconfigure()
                }) { Text(stringResource(R.string.common_delete), color = Color(0xFFFF7B7B)) }
            },
            dismissButton = {
                TextButton(onClick = { keyToDelete = null }) {
                    Text(stringResource(R.string.common_cancel), color = flavor.primary.copy(alpha = 0.84f))
                }
            },
            title = { Text(stringResource(R.string.wallet_delete_key_title)) },
            text = { Text(stringResource(R.string.wallet_delete_key_message, doomed.label)) }
        )
    }
}

@Composable
internal fun WalletKeyCard(
    key: WalletKey,
    maskedKey: String,
    isActive: Boolean,
    health: KeyHealth,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    onTap: () -> Unit
) {
    val accent = lerp(providerAccent(key.provider), flavor.primary, 0.42f)
    val healthColor = when (health) {
        KeyHealth.VALID -> CitrosFlavor.LIME.primary
        KeyHealth.INVALID, KeyHealth.EXPIRED -> CitrosFlavor.BLOOD_ORANGE.primary
        KeyHealth.UNKNOWN, KeyHealth.UNCHECKED -> Color(0xFFEAB308)
    }

    CitrosLiquidGlassSurface(
        modifier = Modifier.fillMaxWidth(),
        shape = CardShape,
        onClick = onTap,
        borderColor = if (isActive) accent.copy(alpha = 0.66f) else flavor.primary.copy(alpha = 0.28f),
        borderWidth = if (isActive) 1.8.dp else 1.dp,
        highlightColor = if (isActive) accent else flavor.primary,
        warmth = if (isActive) 1.10f else 0.82f,
        contentPadding = PaddingValues(SpacingMd)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                providerGlyph(key.provider),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.semantics { contentDescription = key.provider.name }
            )
            Spacer(Modifier.width(SpacingXs))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    key.label,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (isActive) accent else MaterialTheme.colorScheme.onSurface
                )
                Text(
                    maskedKey,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
                )
                key.expiresAt?.let { expiry ->
                    val now = System.currentTimeMillis()
                    when {
                        expiry < now -> Text(
                            "⚠\uFE0F Expired",
                            style = MaterialTheme.typography.bodySmall,
                            color = CitrosFlavor.BLOOD_ORANGE.primary,
                            modifier = Modifier.semantics { contentDescription = "API key expired" }
                        )
                        expiry - now < EXPIRY_WARNING_THRESHOLD_MS -> Text(
                            "⚠\uFE0F Expires soon",
                            style = MaterialTheme.typography.bodySmall,
                            color = Color(0xFFEAB308),
                            modifier = Modifier.semantics { contentDescription = "API key expires soon" }
                        )
                    }
                }
            }
            Box(
                modifier = Modifier
                    .size(10.dp)
                    .background(healthColor, CircleShape)
            )
            Spacer(Modifier.width(20.dp))
        }
    }
}

/**
 * Shared component for model selection UI, used in both SettingsScreen and ModelsSettingsScreen.
 * Displays dropdown menus for chat and action model selection based on the active provider.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ModelSelectionSection(
    activeProvider: Provider,
    chatModelId: String,
    actionModelId: String,
    chatModels: List<String>,
    actionModels: List<String>,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    onChatChange: (String) -> Unit,
    onActionChange: (String) -> Unit
) {
    var chatExpanded by remember { mutableStateOf(false) }
    var actionExpanded by remember { mutableStateOf(false) }
    val accent = lerp(providerAccent(activeProvider), flavor.primary, 0.44f)
    val fieldColors = OutlinedTextFieldDefaults.colors(
        focusedBorderColor = accent.copy(alpha = 0.92f),
        unfocusedBorderColor = accent.copy(alpha = 0.46f),
        focusedLabelColor = accent.copy(alpha = 0.92f),
        unfocusedLabelColor = accent.copy(alpha = 0.74f),
        cursorColor = accent
    )

    CitrosLiquidGlassSurface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        borderColor = accent.copy(alpha = 0.42f),
        borderWidth = 1.dp,
        highlightColor = accent,
        warmth = 0.96f,
        contentPadding = PaddingValues(SpacingMd)
    ) {
        Column {
            Text(
                stringResource(R.string.wallet_model_selection),
                style = MaterialTheme.typography.titleMedium,
                color = accent
            )
            Spacer(Modifier.height(SpacingXs))

            ExposedDropdownMenuBox(expanded = chatExpanded, onExpandedChange = { chatExpanded = it }) {
                OutlinedTextField(
                    value = chatModelId,
                    onValueChange = {},
                    readOnly = true,
                    label = { Text(stringResource(R.string.wallet_chat_model)) },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = chatExpanded) },
                    colors = fieldColors,
                    modifier = Modifier.menuAnchor().fillMaxWidth()
                )
                DropdownMenu(expanded = chatExpanded, onDismissRequest = { chatExpanded = false }) {
                    chatModels.forEach { model ->
                        DropdownMenuItem(text = { Text(model) }, onClick = {
                            onChatChange(model)
                            chatExpanded = false
                        })
                    }
                }
            }

            Spacer(Modifier.height(SpacingXs))

            ExposedDropdownMenuBox(expanded = actionExpanded, onExpandedChange = { actionExpanded = it }) {
                OutlinedTextField(
                    value = actionModelId,
                    onValueChange = {},
                    readOnly = true,
                    label = { Text(stringResource(R.string.wallet_action_model)) },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = actionExpanded) },
                    colors = fieldColors,
                    modifier = Modifier.menuAnchor().fillMaxWidth()
                )
                DropdownMenu(expanded = actionExpanded, onDismissRequest = { actionExpanded = false }) {
                    actionModels.forEach { model ->
                        DropdownMenuItem(text = { Text(model) }, onClick = {
                            onActionChange(model)
                            actionExpanded = false
                        })
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AddKeyBottomSheet(
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    onDismiss: () -> Unit,
    onSave: (Provider, String, String) -> Unit,
    onTested: (KeyHealth, Provider, String) -> Unit
) {
    var selectedProvider by remember { mutableStateOf(Provider.ANTHROPIC) }
    var apiKey by remember { mutableStateOf("") }
    var label by remember { mutableStateOf(defaultLabelFor(selectedProvider)) }
    var showSecret by remember { mutableStateOf(false) }
    var testStatus by remember { mutableStateOf<ConnectionTestResult?>(null) }
    var testing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val providerTabs = remember {
        listOf(Provider.ANTHROPIC, Provider.OPENROUTER, Provider.OPENAI)
    }

    LaunchedEffect(apiKey) {
        ProviderConfig.detectProvider(apiKey)?.let {
            selectedProvider = it
            if (label.isBlank() || label.endsWith(" Key")) label = defaultLabelFor(it)
        }
    }

    val selectedAccent = lerp(providerAccent(selectedProvider), flavor.primary, 0.46f)
    val context = LocalContext.current
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val providerUrl = providerDashboardUrl(selectedProvider)
    val providerUrlLabel = providerUrl.removePrefix("https://")
    val hasApiKey = apiKey.trim().isNotBlank()
    val health = testStatus?.health
    val showInvalidState = health == KeyHealth.INVALID || health == KeyHealth.EXPIRED
    val invalidMessage = when (health) {
        KeyHealth.INVALID, KeyHealth.EXPIRED -> "Invalid API key. Check it and try again."
        else -> null
    }
    val fieldColors = OutlinedTextFieldDefaults.colors(
        focusedBorderColor = Color.Transparent,
        unfocusedBorderColor = Color.Transparent,
        focusedLabelColor = surfaces.labelSecondary,
        unfocusedLabelColor = surfaces.labelSecondary,
        cursorColor = selectedAccent,
        focusedTextColor = surfaces.labelPrimary,
        unfocusedTextColor = surfaces.labelPrimary,
        focusedContainerColor = surfaces.surface1,
        unfocusedContainerColor = surfaces.surface1,
        disabledContainerColor = surfaces.surface1
    )
    val disabledButtonColor = if (isDarkTheme) surfaces.surface2 else surfaces.surface3.copy(alpha = 0.92f)
    val disabledButtonTextColor = surfaces.labelSecondary
    val isValidated = health == KeyHealth.VALID
    val canValidate = hasApiKey && !testing && !isValidated
    val canSave = isValidated && !testing
    val validateButtonBg = if (isDarkTheme) Color.White else Color.Black
    val saveButtonBg = if (isDarkTheme) Color.White else Color.Black
    val buttonBg = when {
        canSave -> saveButtonBg
        canValidate -> validateButtonBg
        else -> disabledButtonColor
    }
    val buttonTextColor = if (canValidate || canSave) {
        if (buttonBg.luminance() >= 0.52f) Color.Black else Color.White
    } else {
        disabledButtonTextColor
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = if (isDarkTheme) {
            Color(0xF51C1C1E)
        } else {
            Color(0xF5F2F2F7)
        },
        contentColor = surfaces.labelPrimary,
        scrimColor = Color.Black.copy(alpha = if (isDarkTheme) 0.50f else 0.26f),
        dragHandle = {
            Box(
                modifier = Modifier
                    .padding(top = 12.dp, bottom = 6.dp)
                    .size(width = 44.dp, height = 5.dp)
                    .background(surfaces.labelTertiary.copy(alpha = 0.42f), RoundedCornerShape(999.dp))
            )
        }
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Add Provider",
                    style = CitrosTypography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = surfaces.labelPrimary,
                    modifier = Modifier.weight(1f)
                )
                Surface(
                    modifier = Modifier
                        .size(28.dp)
                        .clickable(onClick = onDismiss),
                    shape = CircleShape,
                    color = surfaces.surface3
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            text = "×",
                            style = CitrosTypography.titleMedium,
                            color = surfaces.labelSecondary
                        )
                    }
                }
            }
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                color = if (isDarkTheme) surfaces.surface1 else surfaces.surface2,
                border = BorderStroke(1.dp, surfaces.separatorLight)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(4.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    providerTabs.forEach { provider ->
                        val selected = selectedProvider == provider
                        Box(
                            modifier = Modifier
                                .weight(1f)
                                .background(
                                    if (selected) {
                                        if (isDarkTheme) surfaces.surface2 else surfaces.background
                                    } else {
                                        Color.Transparent
                                    },
                                    RoundedCornerShape(10.dp)
                                )
                                .border(
                                    width = if (selected) 1.dp else 0.dp,
                                    color = if (selected) surfaces.separator else Color.Transparent,
                                    shape = RoundedCornerShape(10.dp)
                                )
                                .clickable {
                                    selectedProvider = provider
                                    testStatus = null
                                    if (label.isBlank() || label.endsWith(" Key")) {
                                        label = defaultLabelFor(provider)
                                    }
                                }
                                .padding(horizontal = 8.dp, vertical = 10.dp),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                text = ProviderUi.displayName(provider),
                                style = CitrosTypography.bodyMedium,
                                color = if (selected) surfaces.labelPrimary else surfaces.labelTertiary,
                                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal
                            )
                        }
                    }
                }
            }

            Text(
                text = "Manage API keys at $providerUrlLabel",
                style = CitrosTypography.bodyMedium,
                color = selectedAccent.copy(alpha = 0.92f),
                modifier = Modifier
                    .clickable {
                        context.startActivity(
                            android.content.Intent(
                                android.content.Intent.ACTION_VIEW,
                                android.net.Uri.parse(providerUrl)
                            )
                        )
                    }
            )

            Text(
                text = "API KEY",
                style = CitrosTypography.labelMedium,
                color = surfaces.labelSecondary.copy(alpha = 0.90f)
            )
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(surfaces.surface1, RoundedCornerShape(12.dp))
                    .border(
                        width = if (showInvalidState) 1.5.dp else 0.dp,
                        color = if (showInvalidState) surfaces.red else Color.Transparent,
                        shape = RoundedCornerShape(12.dp)
                    )
            ) {
                OutlinedTextField(
                    value = apiKey,
                    onValueChange = {
                        apiKey = it
                        testStatus = null
                    },
                    placeholder = {
                        Text(
                            ProviderUi.keyPlaceholder(selectedProvider),
                            color = surfaces.labelTertiary,
                            fontFamily = FontFamily.Monospace
                        )
                    },
                    visualTransformation = if (showSecret) {
                        VisualTransformation.None
                    } else {
                        PrefixMaskVisualTransformation(visiblePrefix = 8)
                    },
                    textStyle = CitrosTypography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                    trailingIcon = {
                        CitrosIconButton(onClick = { showSecret = !showSecret }) {
                            CitrosIcon(
                                imageVector = if (showSecret) CitrosIcons.VisibilityOff else CitrosIcons.Visibility,
                                contentDescription = if (showSecret) "Hide key" else "Show key",
                                tint = surfaces.labelTertiary
                            )
                        }
                    },
                    colors = fieldColors,
                    singleLine = true,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.fillMaxWidth()
                )
            }

            invalidMessage?.let {
                Text(
                    text = it,
                    style = CitrosTypography.bodyMedium,
                    color = surfaces.red
                )
            }

            Text(
                text = "LABEL (optional)",
                style = CitrosTypography.labelMedium,
                color = surfaces.labelSecondary.copy(alpha = 0.90f)
            )
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(surfaces.surface1, RoundedCornerShape(12.dp))
            ) {
                OutlinedTextField(
                    value = label,
                    onValueChange = { label = it },
                    placeholder = {
                        Text(
                            "e.g. Personal key",
                            color = surfaces.labelTertiary
                        )
                    },
                    colors = fieldColors,
                    singleLine = true,
                    shape = RoundedCornerShape(12.dp),
                    modifier = Modifier.fillMaxWidth()
                )
            }

            if (isValidated) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center
                ) {
                    Box(
                        modifier = Modifier
                            .size(20.dp)
                            .background(surfaces.surface3, CircleShape),
                        contentAlignment = Alignment.Center
                    ) {
                        Canvas(modifier = Modifier.size(11.dp)) {
                            drawLine(
                                color = surfaces.labelPrimary,
                                start = Offset(x = size.width * 0.18f, y = size.height * 0.56f),
                                end = Offset(x = size.width * 0.42f, y = size.height * 0.80f),
                                strokeWidth = size.minDimension * 0.16f,
                                cap = StrokeCap.Round
                            )
                            drawLine(
                                color = surfaces.labelPrimary,
                                start = Offset(x = size.width * 0.40f, y = size.height * 0.80f),
                                end = Offset(x = size.width * 0.84f, y = size.height * 0.24f),
                                strokeWidth = size.minDimension * 0.16f,
                                cap = StrokeCap.Round
                            )
                        }
                    }
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = "Key verified",
                        style = CitrosTypography.bodyMedium,
                        color = surfaces.green
                    )
                }
            }

            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp)
                    .background(buttonBg, RoundedCornerShape(12.dp))
                    .clickable(enabled = (canValidate || canSave)) {
                        if (canSave) {
                            onSave(
                                selectedProvider,
                                label.ifBlank { defaultLabelFor(selectedProvider) },
                                apiKey.trim()
                            )
                        } else if (canValidate) {
                            testing = true
                            scope.launch {
                                val result = testConnection(selectedProvider, apiKey.trim())
                                testStatus = result
                                onTested(result.health, selectedProvider, apiKey.trim())
                                testing = false
                            }
                        }
                    },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = when {
                        canSave -> "Save Provider"
                        testing -> "Validating..."
                        else -> "Validate Key"
                    },
                    style = CitrosTypography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = buttonTextColor
                )
            }
            Spacer(Modifier.height(10.dp))
        }
    }
}

private suspend fun testConnection(provider: Provider, apiKey: String): ConnectionTestResult {
    if (apiKey.isBlank()) return ConnectionTestResult(KeyHealth.INVALID, R.string.wallet_key_required)
    if (provider == Provider.OPENROUTER) {
        return validateOpenRouterKey(apiKey.trim())
    }
    return when (validateApiCredential(apiKey.trim(), provider)) {
        ApiKeyValidationStatus.VALID -> ConnectionTestResult(KeyHealth.VALID, R.string.wallet_connection_connected)
        ApiKeyValidationStatus.INVALID -> ConnectionTestResult(KeyHealth.INVALID, R.string.wallet_connection_invalid_key)
        ApiKeyValidationStatus.EXPIRED -> ConnectionTestResult(KeyHealth.INVALID, R.string.wallet_connection_invalid_key)
        ApiKeyValidationStatus.UNKNOWN -> ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_could_not_verify)
    }
}

private suspend fun validateOpenRouterKey(apiKey: String): ConnectionTestResult = withContext(Dispatchers.IO) {
    try {
        val conn = (URL("https://openrouter.ai/api/v1/auth/key").openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            connectTimeout = CONNECT_TIMEOUT_MS
            readTimeout = READ_TIMEOUT_MS
            setRequestProperty("Authorization", "Bearer $apiKey")
            setRequestProperty("Accept", "application/json")
            setRequestProperty("HTTP-Referer", "https://citros.ai")
            setRequestProperty("X-Title", "Citros")
        }
        conn.connect()
        when (conn.responseCode) {
            in 200..299 -> ConnectionTestResult(KeyHealth.VALID, R.string.wallet_connection_connected)
            401, 403 -> ConnectionTestResult(KeyHealth.INVALID, R.string.wallet_connection_invalid_key)
            in 500..599 -> ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_provider_unavailable)
            else -> ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_could_not_verify)
        }
    } catch (_: SocketTimeoutException) {
        ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_timed_out)
    } catch (_: UnknownHostException) {
        ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_no_network)
    } catch (_: Throwable) {
        ConnectionTestResult(KeyHealth.UNKNOWN, R.string.wallet_connection_error)
    }
}
