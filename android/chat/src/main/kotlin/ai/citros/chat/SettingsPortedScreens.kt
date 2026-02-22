package ai.citros.chat
import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.role
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.compose.ui.platform.LocalLifecycleOwner
import ai.citros.core.WalletManager
import ai.citros.core.Provider
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.accessibility.AccessibilityManager
import androidx.core.content.ContextCompat
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
/**
 * Helper function to check if accessibility service is enabled for the app
 */
private fun isAccessibilityServiceEnabled(context: Context): Boolean {
    val accessibilityManager = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
    val enabledServices = accessibilityManager.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
    val packageName = context.packageName
    return enabledServices.any { it.resolveInfo.serviceInfo.packageName == packageName }
}

private fun isLocationPermissionGranted(context: Context): Boolean {
    val coarse = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION)
    val fine = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION)
    return coarse == PackageManager.PERMISSION_GRANTED || fine == PackageManager.PERMISSION_GRANTED
}
@Composable
private fun SettingsSubPageScaffold(
    flavor: CitrosFlavor,
    title: String,
    onBack: () -> Unit,
    accentColor: Color? = null,
    scrollable: Boolean = true,
    content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val resolvedAccent = accentColor ?: flavor.primary.copy(alpha = 0.92f)
    val statusBarTopPadding = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    Scaffold(containerColor = surfaces.background) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = statusBarTopPadding + 4.dp, start = 12.dp, end = 12.dp, bottom = 8.dp)
            ) {
                Row(
                    modifier = Modifier
                        .align(Alignment.CenterStart)
                        .height(44.dp)
                        .clickable(onClick = onBack)
                        .semantics {
                            contentDescription = "Back"
                            role = Role.Button
                        },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    SettingsBackChevron(
                        tint = resolvedAccent,
                        modifier = Modifier.size(width = 10.dp, height = 16.dp)
                    )
                    Text(
                        text = "Settings",
                        style = CitrosTypography.bodyLarge,
                        color = resolvedAccent
                    )
                }
                Text(
                    text = title,
                    style = CitrosTypography.titleMedium,
                    color = surfaces.labelPrimary,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.align(Alignment.Center)
                )
            }
            HorizontalDivider(
                color = surfaces.separator,
                thickness = 0.5.dp
            )
            val bodyModifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 16.dp, vertical = 14.dp)
            if (scrollable) {
                Column(
                    modifier = bodyModifier.verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                    content = content
                )
            } else {
                Column(
                    modifier = bodyModifier,
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                    content = content
                )
            }
        }
    }
}

@Composable
private fun SettingsBackChevron(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val stroke = size.minDimension * 0.22f
        drawLine(
            color = tint,
            start = Offset(size.width * 0.88f, size.height * 0.10f),
            end = Offset(size.width * 0.12f, size.height * 0.50f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.12f, size.height * 0.50f),
            end = Offset(size.width * 0.88f, size.height * 0.90f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
    }
}

@Composable
private fun SettingsTrashIcon(
    tint: Color,
    modifier: Modifier = Modifier
) {
    Canvas(modifier = modifier) {
        val stroke = size.minDimension * 0.12f
        drawLine(
            color = tint,
            start = Offset(size.width * 0.30f, size.height * 0.28f),
            end = Offset(size.width * 0.70f, size.height * 0.28f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.40f, size.height * 0.20f),
            end = Offset(size.width * 0.60f, size.height * 0.20f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.35f, size.height * 0.28f),
            end = Offset(size.width * 0.38f, size.height * 0.78f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.65f, size.height * 0.28f),
            end = Offset(size.width * 0.62f, size.height * 0.78f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.38f, size.height * 0.78f),
            end = Offset(size.width * 0.62f, size.height * 0.78f),
            strokeWidth = stroke,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.46f, size.height * 0.38f),
            end = Offset(size.width * 0.46f, size.height * 0.68f),
            strokeWidth = stroke * 0.9f,
            cap = StrokeCap.Round
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.54f, size.height * 0.38f),
            end = Offset(size.width * 0.54f, size.height * 0.68f),
            strokeWidth = stroke * 0.9f,
            cap = StrokeCap.Round
        )
    }
}

@Composable
private fun SettingsGlassPillButton(
    text: String,
    tint: Color,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(999.dp),
        color = tint.copy(alpha = 0.14f),
        border = androidx.compose.foundation.BorderStroke(1.dp, tint.copy(alpha = 0.42f))
    ) {
        Box(
            modifier = Modifier
                .clickable(onClick = onClick)
                .padding(horizontal = 14.dp, vertical = 8.dp)
        ) {
            Text(
                text,
                style = CitrosTypography.labelLarge,
                fontWeight = FontWeight.SemiBold,
                color = tint.copy(alpha = 0.95f)
            )
        }
    }
}

@Composable
private fun SettingsSelectedBadge(
    accent: Color,
    text: String = "Selected"
) {
    Surface(
        shape = RoundedCornerShape(999.dp),
        color = accent.copy(alpha = 0.16f)
    ) {
        Text(
            text = text,
            style = CitrosTypography.labelSmall,
            color = accent.copy(alpha = 0.96f),
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
        )
    }
}

@Composable
private fun SettingsSelectionCheck(
    modifier: Modifier = Modifier
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Box(
        modifier = modifier
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
}

@Composable
private fun SettingsSectionHeader(
    text: String
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Text(
        text = text.uppercase(),
        style = CitrosTypography.labelSmall,
        color = surfaces.labelSecondary
    )
}
@Composable
private fun SettingsGroupedSurface(
    modifier: Modifier = Modifier,
    content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        color = surfaces.surface1,
        border = BorderStroke(1.dp, surfaces.separatorLight)
    ) {
        Column(modifier = Modifier.fillMaxWidth(), content = content)
    }
}
@Composable
private fun SettingsListRow(
    title: String,
    subtitle: String? = null,
    onClick: (() -> Unit)? = null,
    showDivider: Boolean = true,
    trailing: @Composable (() -> Unit)? = null
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) {}
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 14.dp, vertical = 12.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = CitrosTypography.bodyLarge,
                    color = surfaces.labelPrimary
                )
                if (!subtitle.isNullOrBlank()) {
                    Text(
                        text = subtitle,
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelSecondary
                    )
                }
            }
            trailing?.invoke()
        }
        if (showDivider) {
            Spacer(Modifier.height(12.dp))
            HorizontalDivider(
                color = surfaces.separatorLight,
                thickness = 0.5.dp
            )
        }
    }
}
@Composable
internal fun SettingsHubScreen(
    context: Context,
    walletManager: WalletManager,
    onBack: () -> Unit,
    onOpenWallet: () -> Unit,
    onOpenModels: () -> Unit,
    onOpenTrust: () -> Unit,
    onOpenPhoneControl: () -> Unit,
    onOpenSound: () -> Unit,
    onOpenAppearance: () -> Unit,
    onOpenAbout: () -> Unit
) {
    val walletState = remember { walletManager.loadOrDefault() }
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId }
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    val statusBarTopPadding = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    Scaffold(
        containerColor = surfaces.background
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = statusBarTopPadding + 4.dp, start = 12.dp, end = 12.dp, bottom = 8.dp)
                    .height(44.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Settings",
                    style = CitrosTypography.headlineLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = surfaces.labelPrimary
                )
                Spacer(Modifier.weight(1f))
                Text(
                    text = "Back",
                    modifier = Modifier.clickable(onClick = onBack),
                    style = CitrosTypography.bodyLarge,
                    color = flavor.primary.copy(alpha = 0.92f)
                )
            }
            HorizontalDivider(
                color = surfaces.separator,
                thickness = 0.5.dp
            )
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp)
            ) {
                CitrosDirectiveWashBox(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 2.dp),
                    washColor = flavorTokens.washColor,
                    centerXFraction = 0.18f,
                    centerYFraction = 0.5f,
                    radiusFraction = 0.66f,
                    contentAlignment = Alignment.CenterStart
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 4.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        CitrosDirectiveOrb(
                            flavor = flavor,
                            size = 56.dp
                        )
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                "Citros",
                                style = CitrosTypography.headlineSmall,
                                fontWeight = FontWeight.SemiBold,
                                color = surfaces.labelPrimary
                            )
                            Text(
                                activeKey?.let { "${it.label} · ${shortModelName(walletState.chatModelId)}" }
                                    ?: "No active API key",
                                style = CitrosTypography.bodyMedium,
                                color = surfaces.labelSecondary
                            )
                        }
                    }
                }
                Text(
                    text = "General",
                    style = CitrosTypography.labelMedium,
                    color = surfaces.labelSecondary
                )
                SettingsNavCard(
                    icon = CitrosIcons.Brush,
                    title = "Appearance",
                    subtitle = "Theme & flavor settings",
                    flavor = flavor,
                    onClick = onOpenAppearance
                )
                SettingsNavCard(
                    icon = CitrosIcons.Tune,
                    title = "Models",
                    subtitle = "Chat & action model selection",
                    flavor = flavor,
                    onClick = onOpenModels
                )
                SettingsNavCard(
                    icon = CitrosIcons.Volume,
                    title = "Sound & Haptics",
                    subtitle = "Voice, sounds, haptic feedback",
                    flavor = flavor,
                    onClick = onOpenSound
                )
                Text(
                    text = "Account",
                    style = CitrosTypography.labelMedium,
                    color = surfaces.labelSecondary
                )
                SettingsNavCard(
                    icon = CitrosIcons.Key,
                    title = "API Keys",
                    subtitle = "Manage your provider keys",
                    flavor = flavor,
                    onClick = onOpenWallet
                )
                SettingsNavCard(
                    icon = CitrosIcons.Info,
                    title = "About",
                    subtitle = "Version, licenses",
                    flavor = flavor,
                    onClick = onOpenAbout
                )
                Text(
                    text = "Privacy & Control",
                    style = CitrosTypography.labelMedium,
                    color = surfaces.labelSecondary
                )
                SettingsNavCard(
                    icon = CitrosIcons.Security,
                    title = "Trust Level",
                    subtitle = "Permission tier settings",
                    flavor = flavor,
                    onClick = onOpenTrust
                )
                SettingsNavCard(
                    icon = CitrosIcons.Phone,
                    title = "Phone Control",
                    subtitle = "Accessibility & overlay",
                    flavor = flavor,
                    onClick = onOpenPhoneControl
                )
                Spacer(Modifier.height(16.dp))
            }
        }
    }
}
@Composable
internal fun TrustSettingsScreen(
    context: Context,
    onBack: () -> Unit,
    locationPermissionChecker: (Context) -> Boolean = ::isLocationPermissionGranted
) {
    val prefs = remember(context) { context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE) }
    val chatPrefs = remember(context) { context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE) }
    val flavor = remember { readSelectedFlavor(context) }
    var selected by rememberSaveable {
        mutableStateOf(prefs.getString(PREF_PERSONALITY_TRUST, "Ask for risky stuff") ?: "Ask for risky stuff")
    }
    var sensorContextEnabled by rememberSaveable {
        mutableStateOf(chatPrefs.getBoolean(PREF_SENSOR_CONTEXT_ENABLED, PREF_SENSOR_CONTEXT_ENABLED_DEFAULT))
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    var locationPermissionGranted by rememberSaveable { mutableStateOf(locationPermissionChecker(context)) }
    var locationPermissionDenied by rememberSaveable { mutableStateOf(false) }
    DisposableEffect(lifecycleOwner, context) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                val granted = locationPermissionChecker(context)
                locationPermissionGranted = granted
                if (granted) locationPermissionDenied = false
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    val locationPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission()
    ) { granted ->
        locationPermissionGranted = granted || locationPermissionChecker(context)
        locationPermissionDenied = !locationPermissionGranted
    }
    val options = listOf(
        "Ask before everything",
        "Ask for risky stuff",
        "Full autonomy"
    )
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Trust Level",
        onBack = onBack
    ) {
        val isDarkTheme = LocalCitrosIsDark.current
        val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
        SettingsSectionHeader("Autonomy level")
        SettingsGroupedSurface {
            options.forEachIndexed { index, option ->
                val isSelected = selected == option
                SettingsListRow(
                    title = option,
                    subtitle = when (option) {
                        "Ask before everything" -> "Confirm every action before Citros acts."
                        "Ask for risky stuff" -> "Auto-run safe actions, confirm sensitive actions."
                        else -> "Citros acts independently."
                    },
                    onClick = {
                        selected = option
                        prefs.edit().putString(PREF_PERSONALITY_TRUST, option).apply()
                    },
                    showDivider = index < options.lastIndex,
                    trailing = {
                        if (isSelected) {
                            SettingsSelectionCheck()
                        }
                    }
                )
            }
        }
        Text(
            "Trust level controls how much confirmation Citros requires before taking actions on your phone.",
            style = CitrosTypography.bodySmall,
            color = surfaces.labelTertiary
        )
        SettingsSectionHeader("Prompt privacy")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Send device context to cloud models",
                subtitle = "Includes battery, network, local time, and location when permission is granted.",
                showDivider = false,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = sensorContextEnabled,
                            onCheckedChange = {
                                sensorContextEnabled = it
                                if (!it) {
                                    // Hide stale denial warning while sensor sharing is disabled.
                                    locationPermissionDenied = false
                                }
                                chatPrefs.edit().putBoolean(PREF_SENSOR_CONTEXT_ENABLED, it).apply()
                            },
                            modifier = Modifier.testTag("trust_sensor_context_toggle"),
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
        }
        Text(
            "Off by default. Citros only sends this metadata to cloud prompts when enabled and data is available.",
            style = CitrosTypography.bodySmall,
            color = surfaces.labelTertiary
        )
        if (sensorContextEnabled || locationPermissionGranted) {
            SettingsGlassPillButton(
                text = if (locationPermissionGranted) "Location permission granted" else "Request location permission",
                tint = surfaces.green,
                modifier = Modifier.padding(top = 6.dp),
                onClick = {
                    if (locationPermissionGranted || !sensorContextEnabled) return@SettingsGlassPillButton
                    locationPermissionDenied = false
                    locationPermissionLauncher.launch(Manifest.permission.ACCESS_COARSE_LOCATION)
                }
            )
        } else {
            Text(
                "Location permission is optional and only used when device context sharing is enabled.",
                style = CitrosTypography.bodySmall,
                color = surfaces.labelTertiary
            )
        }
        if (sensorContextEnabled && locationPermissionDenied) {
            Text(
                "Location permission denied. You can grant it anytime in App Info.",
                style = CitrosTypography.bodySmall,
                color = surfaces.labelTertiary
            )
        }
        SettingsGlassPillButton(
            text = "Open app permissions",
            tint = surfaces.labelSecondary,
            modifier = Modifier.padding(top = 6.dp),
            onClick = {
                val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                    data = Uri.fromParts("package", context.packageName, null)
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                context.startActivity(intent)
            }
        )
        Spacer(Modifier.height(6.dp))
    }
}
@Composable
internal fun AppearanceSettingsScreen(
    context: Context,
    onBack: () -> Unit
) {
    val prefs = remember(context) { context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE) }
    val chatPrefs = remember(context) { context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE) }
    var selectedFlavor by rememberSaveable {
        mutableStateOf(readSelectedFlavor(context))
    }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    var themeMode by rememberSaveable {
        mutableStateOf(prefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT)
    }
    val autoClearOptions = remember {
        listOf(
            "Never" to ConversationLifecycle.TIMEOUT_NEVER,
            "After 1 hour" to 60L * 60 * 1000,
            "After 1 day" to 24L * 60 * 60 * 1000,
            "After 1 week" to 7L * 24 * 60 * 60 * 1000
        )
    }
    var selectedTimeout by rememberSaveable {
        mutableStateOf(
            chatPrefs
                .getLong("idle_timeout_ms", ConversationLifecycle.DEFAULT_TIMEOUT_MS)
                .let { stored ->
                    if (autoClearOptions.any { it.second == stored }) stored
                    else ConversationLifecycle.TIMEOUT_NEVER
                }
        )
    }
    SettingsSubPageScaffold(
        flavor = selectedFlavor,
        title = "Appearance",
        onBack = onBack
    ) {
        SettingsSectionHeader("Flavor")
        SettingsGroupedSurface {
            val flavorsPerRow = 3
            val flavorRows = CitrosFlavor.entries.chunked(flavorsPerRow)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp)
            ) {
                flavorRows.forEach { flavorRow ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.Top
                    ) {
                        flavorRow.forEach { flavor ->
                            val selected = selectedFlavor == flavor
                            val flavorTokens = citrosDirectiveFlavorTokens(flavor, surfaces)
                            Column(
                                modifier = Modifier
                                    .weight(1f)
                                    .clickable {
                                        selectedFlavor = flavor
                                        prefs.edit()
                                            .putString(PREF_SELECTED_FLAVOR, flavor.storageValue)
                                            .putString(PREF_SELECTED_FLAVOR_OPTION, flavor.storageValue)
                                            .apply()
                                        OverlayService.instance?.refreshAppearanceFromPrefs()
                                        if (!OverlayController.isOverlayActive.value) {
                                            runCatching {
                                                syncLauncherIconWithPreferences(context)
                                            }.onFailure { error ->
                                                android.util.Log.w("AppearanceSettings", "Failed to sync launcher icon", error)
                                            }
                                        }
                                    },
                                horizontalAlignment = Alignment.CenterHorizontally
                            ) {
                                Box(
                                    modifier = Modifier
                                        .size(44.dp)
                                        .background(
                                            color = if (selected) surfaces.labelPrimary else surfaces.separator,
                                            shape = CircleShape
                                        )
                                        .padding(if (selected) 2.dp else 1.dp),
                                    contentAlignment = Alignment.Center
                                ) {
                                    Box(
                                        modifier = Modifier
                                            .fillMaxSize()
                                            .background(flavorTokens.orbColor, CircleShape),
                                        contentAlignment = Alignment.Center
                                    ) {
                                        Box(
                                            modifier = Modifier
                                                .size(14.dp)
                                                .background(flavorTokens.orbInner, CircleShape)
                                        )
                                    }
                                }
                                Spacer(Modifier.height(6.dp))
                                Text(
                                    text = flavor.displayName,
                                    style = CitrosTypography.bodySmall,
                                    color = if (selected) surfaces.labelPrimary else surfaces.labelSecondary,
                                    textAlign = TextAlign.Center,
                                    maxLines = 2
                                )
                            }
                        }
                        repeat(flavorsPerRow - flavorRow.size) {
                            Spacer(modifier = Modifier.weight(1f))
                        }
                    }
                }
            }
        }

        SettingsSectionHeader("Theme")
        SettingsGroupedSurface {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                listOf("dark", "light", "system").forEach { mode ->
                    val selected = themeMode == mode
                    Column(
                        modifier = Modifier
                            .weight(1f)
                            .clickable {
                                themeMode = mode
                                prefs.edit().putString(PREF_THEME_MODE, mode).apply()
                                OverlayService.instance?.refreshAppearanceFromPrefs()
                            },
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Surface(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(64.dp),
                            shape = RoundedCornerShape(10.dp),
                            color = surfaces.surface2,
                            border = BorderStroke(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) surfaces.labelPrimary else surfaces.separator
                            )
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .padding(4.dp),
                                contentAlignment = Alignment.Center
                            ) {
                                when (mode) {
                                    "dark" -> Box(
                                        modifier = Modifier
                                            .fillMaxSize()
                                            .background(Color(0xFF1C1C1E), RoundedCornerShape(8.dp))
                                    )
                                    "light" -> Box(
                                        modifier = Modifier
                                            .fillMaxSize()
                                            .background(Color(0xFFF2F2F7), RoundedCornerShape(8.dp))
                                    )
                                    else -> Canvas(modifier = Modifier.fillMaxSize()) {
                                        drawRoundRect(
                                            color = Color(0xFF1C1C1E),
                                            cornerRadius = androidx.compose.ui.geometry.CornerRadius(16f, 16f)
                                        )
                                        val path = Path().apply {
                                            moveTo(size.width, 0f)
                                            lineTo(size.width, size.height)
                                            lineTo(0f, size.height)
                                            close()
                                        }
                                        drawPath(path = path, color = Color(0xFFF2F2F7))
                                    }
                                }
                            }
                        }
                        Spacer(Modifier.height(6.dp))
                        Text(
                            text = mode.replaceFirstChar { it.uppercase() },
                            style = CitrosTypography.bodyMedium,
                            color = if (selected) surfaces.labelPrimary else surfaces.labelSecondary
                        )
                    }
                }
            }
        }

        SettingsSectionHeader("Auto-clear chat")
        SettingsGroupedSurface {
            val resolvedSelectedTimeout = autoClearOptions
                .firstOrNull { it.second == selectedTimeout }
                ?.second
                ?: ConversationLifecycle.TIMEOUT_NEVER
            autoClearOptions.forEachIndexed { index, (label, timeoutMs) ->
                val selected = resolvedSelectedTimeout == timeoutMs
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable {
                            selectedTimeout = timeoutMs
                            chatPrefs.edit().putLong("idle_timeout_ms", timeoutMs).apply()
                        }
                        .padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = label,
                        style = CitrosTypography.bodyLarge,
                        color = surfaces.labelPrimary,
                        modifier = Modifier.weight(1f)
                    )
                    if (selected) {
                        SettingsSelectionCheck()
                    }
                }
                if (index < autoClearOptions.lastIndex) {
                    HorizontalDivider(
                        color = surfaces.separatorLight,
                        thickness = 0.5.dp
                    )
                }
            }
        }
    }
}
@Composable
internal fun AboutSettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val packageInfo = remember(context) {
        runCatching {
            @Suppress("DEPRECATION")
            context.packageManager.getPackageInfo(context.packageName, 0)
        }.getOrNull()
    }
    val versionName = packageInfo?.versionName ?: "0.1.0"
    @Suppress("DEPRECATION")
    val buildNumber = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        packageInfo?.longVersionCode?.toString() ?: "1"
    } else {
        packageInfo?.versionCode?.toString() ?: "1"
    }
    fun openUrl(url: String) {
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        }
    }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "About",
        onBack = onBack
    ) {
        SettingsSectionHeader("Citros")
        Text(
            "Citros",
            style = CitrosTypography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
            color = surfaces.labelPrimary
        )
        Text(
            "AI phone agent for Android",
            style = CitrosTypography.bodyMedium,
            color = surfaces.labelSecondary
        )
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Version",
                showDivider = true,
                trailing = {
                    Text(
                        text = versionName,
                        style = CitrosTypography.bodyMedium,
                        color = surfaces.labelTertiary
                    )
                }
            )
            SettingsListRow(
                title = "Build",
                showDivider = true
                ,
                trailing = {
                    Text(
                        text = buildNumber,
                        style = CitrosTypography.bodyMedium,
                        color = surfaces.labelTertiary
                    )
                }
            )
            SettingsListRow(
                title = "Device",
                showDivider = false,
                trailing = {
                    Text(
                        text = Build.MODEL ?: "Android device",
                        style = CitrosTypography.bodyMedium,
                        color = surfaces.labelTertiary
                    )
                }
            )
        }
        SettingsSectionHeader("Links")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Licenses",
                onClick = { },
                showDivider = true,
                trailing = {
                    Text("›", color = surfaces.labelTertiary)
                }
            )
            SettingsListRow(
                title = "Privacy Policy",
                onClick = { openUrl("https://citros.ai/privacy") },
                showDivider = true,
                trailing = {
                    Text("›", color = surfaces.labelTertiary)
                }
            )
            SettingsListRow(
                title = "Source Code",
                onClick = { openUrl("https://github.com/citros-ai/citros") },
                showDivider = false,
                trailing = {
                    Text("›", color = surfaces.labelTertiary)
                }
            )
        }
        Text(
            "Made with citrus intent.",
            style = CitrosTypography.bodySmall,
            color = surfaces.labelTertiary
        )
        Spacer(Modifier.height(6.dp))
    }
}
@Composable
internal fun SoundSettingsScreen(
    voiceManager: ai.citros.core.VoiceManager?,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val autoSpeak = voiceManager?.autoSpeakResponses?.collectAsState()?.value ?: false
    val autoSend = voiceManager?.autoSendAfterVoice?.collectAsState()?.value ?: false
    val chatPrefs = remember(context) {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
    }
    var hapticsEnabled by rememberSaveable {
        mutableStateOf(chatPrefs.getBoolean("feedback_haptics_enabled", true))
    }
    var soundEffectsEnabled by rememberSaveable {
        mutableStateOf(chatPrefs.getBoolean("feedback_sound_enabled", false))
    }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Sound & Haptics",
        onBack = onBack,
        scrollable = true
    ) {
        SettingsSectionHeader("Voice")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Read responses aloud",
                subtitle = "Speak AI responses using on-device TTS",
                showDivider = true,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = autoSpeak,
                            onCheckedChange = { voiceManager?.setAutoSpeakResponses(it) },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
            SettingsListRow(
                title = "Auto-send voice input",
                subtitle = "Send message immediately after voice recognition",
                showDivider = false,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = autoSend,
                            onCheckedChange = { voiceManager?.setAutoSendAfterVoice(it) },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
        }
        SettingsSectionHeader("Feedback")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Haptic feedback",
                subtitle = "Vibrate for key actions",
                showDivider = true,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = hapticsEnabled,
                            onCheckedChange = {
                                hapticsEnabled = it
                                chatPrefs.edit().putBoolean("feedback_haptics_enabled", it).apply()
                            },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
            SettingsListRow(
                title = "Sound effects",
                subtitle = "Play subtle interface sounds",
                showDivider = false,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = soundEffectsEnabled,
                            onCheckedChange = {
                                soundEffectsEnabled = it
                                chatPrefs.edit().putBoolean("feedback_sound_enabled", it).apply()
                            },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
        }
    }
}
@Composable
internal fun PhoneControlSettingsScreen(
    context: Context,
    onBack: () -> Unit
) {
    val chatPrefs = remember(context) {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    var overlayPermissionGranted by remember { mutableStateOf(Settings.canDrawOverlays(context)) }
    var accessibilityEnabled by remember { mutableStateOf(isAccessibilityServiceEnabled(context)) }
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    var useIslandWhenIdle by rememberSaveable {
        mutableStateOf(
            chatPrefs.getBoolean(
                PREF_OVERLAY_USE_ISLAND_WHEN_IDLE,
                PREF_OVERLAY_USE_ISLAND_WHEN_IDLE_DEFAULT
            )
        )
    }
    var showSearchBarWhenIdle by rememberSaveable {
        mutableStateOf(
            chatPrefs.getBoolean(
                PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE,
                PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE_DEFAULT
            )
        )
    }
    val okColor = Color(0xFF88F5B4)
    val warningColor = Color(0xFFFF8A8A)
    fun refreshPermissionStatus() {
        overlayPermissionGranted = Settings.canDrawOverlays(context)
        accessibilityEnabled = isAccessibilityServiceEnabled(context)
    }
    LaunchedEffect(Unit) {
        refreshPermissionStatus()
    }
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                refreshPermissionStatus()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Phone Control",
        onBack = onBack
    ) {
        SettingsSectionHeader("Permissions")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Accessibility Service",
                subtitle = "Read and interact with screen content",
                onClick = { context.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) },
                showDivider = true,
                trailing = {
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = (if (accessibilityEnabled) okColor else warningColor).copy(alpha = 0.16f)
                    ) {
                        Text(
                            text = if (accessibilityEnabled) "Granted" else "Not granted",
                            style = CitrosTypography.labelSmall,
                            color = if (accessibilityEnabled) okColor else warningColor,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                        )
                    }
                }
            )
            SettingsListRow(
                title = "Overlay Permission",
                subtitle = "Display Citros over other apps",
                onClick = {
                    val intent = Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION)
                    intent.data = Uri.parse("package:${context.packageName}")
                    context.startActivity(intent)
                },
                showDivider = false,
                trailing = {
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = (if (overlayPermissionGranted) okColor else warningColor).copy(alpha = 0.16f)
                    ) {
                        Text(
                            text = if (overlayPermissionGranted) "Granted" else "Not granted",
                            style = CitrosTypography.labelSmall,
                            color = if (overlayPermissionGranted) okColor else warningColor,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                        )
                    }
                }
            )
        }
        SettingsSectionHeader("Overlay Behavior")
        SettingsGroupedSurface {
            SettingsListRow(
                title = "Automatic mode switching",
                subtitle = "Citros switches between surfaces based on what it's doing.",
                showDivider = true
            )
            SettingsListRow(
                title = "Use island instead of search bar when idle",
                subtitle = "Default behavior for idle overlay mode outside the app.",
                showDivider = true,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = useIslandWhenIdle,
                            onCheckedChange = {
                                useIslandWhenIdle = it
                                chatPrefs.edit().putBoolean(PREF_OVERLAY_USE_ISLAND_WHEN_IDLE, it).apply()
                                OverlayController.updateIdleSurfacePreference(it)
                            },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
            SettingsListRow(
                title = "Show search bar when idle",
                subtitle = "Turn off to hide idle overlay when island idle mode is disabled.",
                showDivider = false,
                trailing = {
                    Box(modifier = Modifier.padding(start = 12.dp)) {
                        Switch(
                            checked = showSearchBarWhenIdle,
                            onCheckedChange = {
                                showSearchBarWhenIdle = it
                                chatPrefs.edit().putBoolean(PREF_OVERLAY_SHOW_SEARCH_BAR_WHEN_IDLE, it).apply()
                                OverlayController.updateSearchBarIdlePreference(it)
                            },
                            colors = SwitchDefaults.colors(
                                checkedThumbColor = Color.White,
                                checkedTrackColor = surfaces.green,
                                uncheckedThumbColor = surfaces.surface4,
                                uncheckedTrackColor = surfaces.surface3
                            )
                        )
                    }
                }
            )
        }
    }
}
@Composable
internal fun ModelsSettingsScreen(
    walletManager: WalletManager,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    var walletState by remember { mutableStateOf(walletManager.loadOrDefault()) }
    val activeProvider = walletState.keys.find { it.id == walletState.activeKeyId }?.provider
    val chatPrefs = remember(context) {
        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
    }
    var useLocalFallback by rememberSaveable {
        mutableStateOf(chatPrefs.getBoolean("models_use_local_offline", true))
    }
    fun modelSubtitle(modelId: String): String = when {
        modelId.contains("llama", ignoreCase = true) -> "On-device fallback model"
        modelId.contains("sonnet", ignoreCase = true) -> "Balanced speed and capability"
        modelId.contains("opus", ignoreCase = true) -> "Most capable cloud model"
        modelId.contains("haiku", ignoreCase = true) -> "Fastest cloud response"
        else -> modelId
    }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Models",
        onBack = onBack
    ) {
        if (activeProvider != null) {
            val chatModels = ai.citros.core.ModelConfig.chatModelsForProvider(activeProvider)
            val actionModels = ai.citros.core.ModelConfig.actionModelsForProvider(activeProvider)
            SettingsSectionHeader("Chat Model")
            SettingsGroupedSurface {
                chatModels.forEachIndexed { index, modelId ->
                    val selected = modelId == walletState.chatModelId
                    SettingsListRow(
                        title = shortModelName(modelId),
                        subtitle = modelSubtitle(modelId),
                        onClick = {
                            walletManager.setChatModel(modelId)
                            walletState = walletManager.loadOrDefault()
                        },
                        showDivider = index < chatModels.lastIndex,
                        trailing = {
                            if (selected) {
                                SettingsSelectionCheck()
                            }
                        }
                    )
                }
            }
            SettingsSectionHeader("Action Model")
            SettingsGroupedSurface {
                actionModels.forEachIndexed { index, modelId ->
                    val selected = modelId == walletState.actionModelId
                    SettingsListRow(
                        title = shortModelName(modelId),
                        subtitle = modelSubtitle(modelId),
                        onClick = {
                            walletManager.setActionModel(modelId)
                            walletState = walletManager.loadOrDefault()
                        },
                        showDivider = index < actionModels.lastIndex,
                        trailing = {
                            if (selected) {
                                SettingsSelectionCheck()
                            }
                        }
                    )
                }
            }
            SettingsSectionHeader("Fallback")
            SettingsGroupedSurface {
                SettingsListRow(
                    title = "Use local model when offline",
                    subtitle = "Automatically switch to an on-device fallback when provider calls fail",
                    showDivider = false,
                    trailing = {
                        Box(modifier = Modifier.padding(start = 12.dp)) {
                            Switch(
                                checked = useLocalFallback,
                                onCheckedChange = {
                                    useLocalFallback = it
                                    chatPrefs.edit().putBoolean("models_use_local_offline", it).apply()
                                },
                                colors = SwitchDefaults.colors(
                                    checkedThumbColor = Color.White,
                                    checkedTrackColor = surfaces.green,
                                    uncheckedThumbColor = surfaces.surface4,
                                    uncheckedTrackColor = surfaces.surface3
                                )
                            )
                        }
                    }
                )
            }
        } else {
            SettingsSectionHeader("Models")
            SettingsGroupedSurface {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 14.dp, vertical = 16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    CitrosIcon(
                        imageVector = CitrosIcons.Key,
                        contentDescription = "No API Key",
                        tint = surfaces.labelTertiary,
                        modifier = Modifier.padding(12.dp)
                    )
                    Text(
                        "No API Key Active",
                        style = CitrosTypography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = surfaces.labelPrimary
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Add an API key in Settings → API Keys to configure model preferences",
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelSecondary
                    )
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}
@Composable
internal fun ApiKeysSettingsScreen(
    walletManager: WalletManager,
    keyStore: ai.citros.core.KeyStore,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    var walletState by remember { mutableStateOf(walletManager.loadOrDefault()) }
    var showAddSheet by remember { mutableStateOf(false) }
    fun refreshWalletState() {
        walletState = walletManager.loadOrDefault()
    }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "API Keys",
        onBack = onBack
    ) {
        Text(
            "CONNECTED PROVIDERS",
            style = CitrosTypography.labelSmall,
            color = surfaces.labelSecondary
        )
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            color = surfaces.surface1,
            border = androidx.compose.foundation.BorderStroke(1.dp, surfaces.separatorLight)
        ) {
            if (walletState.keys.isEmpty()) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 14.dp, vertical = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Text(
                        "No API keys connected",
                        style = CitrosTypography.titleSmall,
                        color = surfaces.labelPrimary
                    )
                    Text(
                        "Add a provider key to enable cloud models.",
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelSecondary
                    )
                }
            } else {
                Column(modifier = Modifier.fillMaxWidth()) {
                    walletState.keys.forEachIndexed { index, key ->
                        val isActive = walletState.activeKeyId == key.id
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    walletManager.setActiveKey(key.id)
                                    refreshWalletState()
                                }
                                .padding(horizontal = 14.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = ProviderUi.icon(key.provider),
                                style = CitrosTypography.titleMedium
                            )
                            Spacer(Modifier.width(10.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    key.label,
                                    style = CitrosTypography.bodyLarge,
                                    color = surfaces.labelPrimary
                                )
                                Text(
                                    maskApiKey(keyStore.get(key.id)),
                                    style = CitrosTypography.bodySmall,
                                    color = surfaces.labelTertiary
                                )
                            }
                            Row(
                                modifier = Modifier.fillMaxHeight(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                if (isActive) {
                                    Surface(
                                        modifier = Modifier.height(26.dp),
                                        shape = RoundedCornerShape(999.dp),
                                        color = surfaces.green.copy(alpha = 0.18f)
                                    ) {
                                        Box(
                                            modifier = Modifier
                                                .fillMaxHeight()
                                                .padding(horizontal = 8.dp),
                                            contentAlignment = Alignment.Center
                                        ) {
                                            Text(
                                                "Active",
                                                style = CitrosTypography.labelSmall,
                                                color = surfaces.green
                                            )
                                        }
                                    }
                                }
                                Surface(
                                    modifier = Modifier
                                        .size(26.dp)
                                        .clickable {
                                            walletManager.removeKey(key.id)
                                            refreshWalletState()
                                        },
                                    shape = CircleShape,
                                    color = Color.Transparent,
                                    border = BorderStroke(1.dp, surfaces.separatorLight)
                                ) {
                                    Box(
                                        modifier = Modifier.fillMaxSize(),
                                        contentAlignment = Alignment.Center
                                    ) {
                                        SettingsTrashIcon(
                                            tint = surfaces.labelPrimary,
                                            modifier = Modifier.size(15.dp)
                                        )
                                    }
                                }
                            }
                        }
                        if (index < walletState.keys.lastIndex) {
                            HorizontalDivider(
                                color = surfaces.separatorLight,
                                thickness = 0.5.dp
                            )
                        }
                    }
                }
            }
        }
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { showAddSheet = true },
            shape = RoundedCornerShape(14.dp),
            color = surfaces.surface1,
            border = androidx.compose.foundation.BorderStroke(1.dp, surfaces.separatorLight)
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 14.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Add provider",
                    style = CitrosTypography.bodyLarge,
                    color = surfaces.labelPrimary,
                    modifier = Modifier.weight(1f)
                )
                Text(
                    text = "+",
                    style = CitrosTypography.headlineSmall,
                    color = surfaces.labelTertiary
                )
            }
        }
        Text(
            "Active key defaults are used for chat and action models.",
            style = CitrosTypography.bodySmall,
            color = surfaces.labelTertiary
        )
        Spacer(Modifier.height(6.dp))
    }
    if (showAddSheet) {
        AddKeyBottomSheet(
            flavor = flavor,
            onDismiss = { showAddSheet = false },
            onSave = { provider, label, apiKey ->
                val created = walletManager.addKey(provider, label, apiKey)
                walletManager.setActiveKey(created.id)
                refreshWalletState()
                showAddSheet = false
            },
            onTested = { _, _, _ -> }
        )
    }
}
@Composable
private fun SettingsNavCard(
    icon: ImageVector,
    title: String,
    subtitle: String,
    flavor: CitrosFlavor,
    onClick: () -> Unit
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) {
                contentDescription = "Settings card: $title"
            }
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(14.dp),
        color = surfaces.surface1,
        border = androidx.compose.foundation.BorderStroke(1.dp, surfaces.separatorLight)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Surface(
                shape = RoundedCornerShape(10.dp),
                color = surfaces.surface2
            ) {
                CitrosIcon(
                    imageVector = icon,
                    contentDescription = null,
                    tint = flavor.primary.copy(alpha = 0.88f),
                    modifier = Modifier.padding(8.dp)
                )
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    title,
                    style = CitrosTypography.titleMedium,
                    color = surfaces.labelPrimary,
                    modifier = Modifier.clickable(onClick = onClick)
                )
                Text(
                    subtitle,
                    style = CitrosTypography.bodySmall,
                    color = surfaces.labelSecondary
                )
            }
            Text("›", color = surfaces.labelTertiary)
        }
    }
}
