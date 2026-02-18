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

internal fun citrosSplashVisualTokens(flavor: CitrosFlavor): CitrosSplashVisualTokens {
    val heroDeep = lerp(Color(0xFF1A0A00), flavor.tint, 0.20f)
    val heroPrimary = lerp(Color(0xFFF59E0B), flavor.primary, 0.15f)
    val heroWarm = lerp(Color(0xFFFF6B2B), flavor.primary, 0.18f)
    val heroWire = lerp(Color(0xFF4A2D13), heroDeep, 0.28f)
    val heroRing = lerp(heroPrimary, heroWarm, 0.36f)
    val heroParticle = lerp(Color(0xFFFFCC53), heroPrimary, 0.22f)

    val buttonDeep = lerp(Color(0xFF1A0A00), flavor.tint, 0.22f)
    val buttonAmber = lerp(Color(0xFFF59E0B), flavor.primary, 0.20f)
    val buttonWarm = lerp(Color(0xFFFF6B2B), flavor.primary, 0.18f)
    val brandTitle = lerp(Color(0xFFFFA625), flavor.primary, 0.10f)

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
            textEnabled = Color(0xFFFFCF7A),
            textDisabled = Color(0x99FFCF7A)
        ),
        brandTitleColor = brandTitle
    )
}

internal val LocalCitrosSplashVisualTokens = staticCompositionLocalOf {
    citrosSplashVisualTokens(CitrosFlavor.TANGERINE)
}

@Composable
internal fun ProvideCitrosSplashVisualTokens(
    flavor: CitrosFlavor,
    content: @Composable () -> Unit
) {
    val tokens = remember(flavor) { citrosSplashVisualTokens(flavor) }
    CompositionLocalProvider(LocalCitrosSplashVisualTokens provides tokens) {
        content()
    }
}
