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
    if (flavor == CitrosFlavor.NONE) {
        return CitrosSplashVisualTokens(
            hero = CitrosHeroShaderTokens(
                deep = if (isDark) Color(0xFFCACDD6) else Color(0xFF09090B),
                primary = if (isDark) Color(0xFFFFFFFF) else Color(0xFF000000),
                warm = if (isDark) Color(0xFFE6E8EE) else Color(0xFF262830),
                wire = if (isDark) Color(0xFFAAAEBB) else Color(0xFF41444E),
                ring = if (isDark) Color(0xFFD6D9E3) else Color(0xFF373A44),
                particle = if (isDark) Color(0xFFBEC2CE) else Color(0xFF525664)
            ),
            glassButton = CitrosLiquidGlassButtonTokens(
                deep = if (isDark) Color(0xFF22242B) else Color(0xFFE7E8ED),
                amber = if (isDark) Color(0xFF323540) else Color(0xFFD6D8DF),
                warm = if (isDark) Color(0xFF4A4E5D) else Color(0xFFBFC3CE),
                textEnabled = if (isDark) Color.White else Color.Black,
                textDisabled = if (isDark) Color(0x99FFFFFF) else Color(0x99000000)
            ),
            brandTitleColor = if (isDark) Color.White else Color.Black
        )
    }

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
internal val LocalCitrosFlavor = staticCompositionLocalOf { CitrosFlavor.TANGERINE }

@Composable
internal fun ProvideCitrosSplashVisualTokens(
    flavor: CitrosFlavor,
    isDark: Boolean,
    content: @Composable () -> Unit
) {
    val tokens = remember(flavor, isDark) { citrosSplashVisualTokens(flavor, isDark) }
    CompositionLocalProvider(
        LocalCitrosFlavor provides flavor,
        LocalCitrosIsDark provides isDark,
        LocalCitrosSplashVisualTokens provides tokens
    ) {
        content()
    }
}
