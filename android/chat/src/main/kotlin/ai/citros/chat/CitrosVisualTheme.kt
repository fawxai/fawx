package ai.citros.chat

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp

internal data class CitrosHeroShaderTokens(
    val deep: Color,
    val primary: Color,
    val warm: Color,
    val wire: Color,
    val ring: Color,
    val particle: Color
)

internal data class CitrosLiquidGlassButtonTokens(
    val deep: Color,
    val amber: Color,
    val warm: Color,
    val textEnabled: Color,
    val textDisabled: Color
)

internal data class CitrosSplashVisualTokens(
    val hero: CitrosHeroShaderTokens,
    val glassButton: CitrosLiquidGlassButtonTokens,
    val brandTitleColor: Color
)

internal fun citrosSplashVisualTokens(
    flavor: CitrosFlavor,
    isDark: Boolean
): CitrosSplashVisualTokens {
    val heroDeep = if (isDark) {
        lerp(Color(0xFF1A0A00), flavor.tint, 0.20f)
    } else {
        lerp(Color(0xFFF7EEE3), flavor.glow, 0.22f)
    }
    val heroPrimary = if (isDark) {
        lerp(Color(0xFFF59E0B), flavor.primary, 0.15f)
    } else {
        lerp(Color(0xFFFFC463), flavor.primary, 0.20f)
    }
    val heroWarm = if (isDark) {
        lerp(Color(0xFFFF6B2B), flavor.primary, 0.18f)
    } else {
        lerp(Color(0xFFFFAA57), flavor.primary, 0.22f)
    }
    val heroWire = if (isDark) {
        lerp(Color(0xFF4A2D13), heroDeep, 0.28f)
    } else {
        lerp(Color(0xFFD6BCA2), heroDeep, 0.26f)
    }
    val heroRing = if (isDark) {
        lerp(heroPrimary, heroWarm, 0.36f)
    } else {
        lerp(heroPrimary, heroWarm, 0.28f)
    }
    val heroParticle = if (isDark) {
        lerp(Color(0xFFFFCC53), heroPrimary, 0.22f)
    } else {
        lerp(Color(0xFFFFE4A3), heroPrimary, 0.24f)
    }

    val buttonDeep = if (isDark) {
        lerp(Color(0xFF1A0A00), flavor.tint, 0.22f)
    } else {
        lerp(Color(0xFFFFF4E7), flavor.glow, 0.22f)
    }
    val buttonAmber = if (isDark) {
        lerp(Color(0xFFF59E0B), flavor.primary, 0.20f)
    } else {
        lerp(Color(0xFFFFC463), flavor.primary, 0.24f)
    }
    val buttonWarm = if (isDark) {
        lerp(Color(0xFFFF6B2B), flavor.primary, 0.18f)
    } else {
        lerp(Color(0xFFFF9D4D), flavor.primary, 0.24f)
    }
    val brandTitle = if (isDark) {
        lerp(Color(0xFFFFA625), flavor.primary, 0.10f)
    } else {
        lerp(Color(0xFF9A4A08), flavor.primary, 0.26f)
    }

    return CitrosSplashVisualTokens(
        hero = CitrosHeroShaderTokens(
            deep = heroDeep,
            primary = heroPrimary,
            warm = heroWarm,
            wire = heroWire,
            ring = heroRing,
            particle = heroParticle
        ),
        glassButton = CitrosLiquidGlassButtonTokens(
            deep = buttonDeep,
            amber = buttonAmber,
            warm = buttonWarm,
            textEnabled = if (isDark) Color(0xFFFFCF7A) else Color(0xFF2B1A08),
            textDisabled = if (isDark) Color(0x99FFCF7A) else Color(0x992B1A08)
        ),
        brandTitleColor = brandTitle
    )
}

internal val LocalCitrosSplashVisualTokens = staticCompositionLocalOf {
    citrosSplashVisualTokens(CitrosFlavor.TANGERINE, isDark = false)
}
internal val LocalCitrosIsDark = staticCompositionLocalOf { false }

@Composable
internal fun ProvideCitrosSplashVisualTokens(
    flavor: CitrosFlavor,
    isDark: Boolean,
    content: @Composable () -> Unit
) {
    val tokens = remember(flavor, isDark) { citrosSplashVisualTokens(flavor, isDark) }
    CompositionLocalProvider(
        LocalCitrosIsDark provides isDark,
        LocalCitrosSplashVisualTokens provides tokens
    ) {
        content()
    }
}
