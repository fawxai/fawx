package ai.citros.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

internal data class CitrosDirectiveSurfaces(
    val background: Color,
    val surface1: Color,
    val surface2: Color,
    val surface3: Color,
    val surface4: Color,
    val labelPrimary: Color,
    val labelSecondary: Color,
    val labelTertiary: Color,
    val labelQuaternary: Color,
    val separator: Color,
    val separatorLight: Color,
    val green: Color,
    val red: Color,
    val orange: Color,
    val blue: Color
)

internal data class CitrosDirectiveFlavorTokens(
    val orbColor: Color,
    val orbInner: Color,
    val orbGlow: Color,
    val washColor: Color,
    val userBubbleBackground: Color,
    val userBubbleText: Color,
    val sendButtonBackground: Color,
    val sendIconTint: Color,
    val caretColor: Color,
    val actionIconTint: Color
)

private data class FlavorMetadata(
    val primary: Color,
    val onPrimary: Color,
    val glow: Color,
    val wash: Color
)

private val LemonFlavor = FlavorMetadata(
    primary = Color(0xFFFFD600),
    onPrimary = Color(0xFF1C1A00),
    glow = Color(0x26FFD600),
    wash = Color(0x08FFD600)
)

private val TangerineFlavor = FlavorMetadata(
    primary = Color(0xFFFF8C00),
    onPrimary = Color.White,
    glow = Color(0x26FF8C00),
    wash = Color(0x08FF8C00)
)

private val LimeFlavor = FlavorMetadata(
    primary = Color(0xFF7CB342),
    onPrimary = Color.White,
    glow = Color(0x267CB342),
    wash = Color(0x087CB342)
)

private val BloodOrangeFlavor = FlavorMetadata(
    primary = Color(0xFFD84315),
    onPrimary = Color.White,
    glow = Color(0x26D84315),
    wash = Color(0x08D84315)
)

private val GrapefruitFlavor = FlavorMetadata(
    primary = Color(0xFFE91E63),
    onPrimary = Color.White,
    glow = Color(0x26E91E63),
    wash = Color(0x08E91E63)
)

internal fun citrosDirectiveSurfaces(isDark: Boolean): CitrosDirectiveSurfaces {
    return if (isDark) {
        CitrosDirectiveSurfaces(
            background = Color(0xFF000000),
            surface1 = Color(0xFF1C1C1E),
            surface2 = Color(0xFF2C2C2E),
            surface3 = Color(0xFF3A3A3C),
            surface4 = Color(0xFF48484A),
            labelPrimary = Color(0xFFFFFFFF),
            labelSecondary = Color(0x99EBEBF5),
            labelTertiary = Color(0x4DEBEBF5),
            labelQuaternary = Color(0x2EEBEBF5),
            separator = Color(0x5C545458),
            separatorLight = Color(0x33545458),
            green = Color(0xFF30D158),
            red = Color(0xFFFF453A),
            orange = Color(0xFFFF9F0A),
            blue = Color(0xFF0A84FF)
        )
    } else {
        CitrosDirectiveSurfaces(
            background = Color(0xFFFFFFFF),
            surface1 = Color(0xFFF2F2F7),
            surface2 = Color(0xFFE5E5EA),
            surface3 = Color(0xFFD1D1D6),
            surface4 = Color(0xFFC7C7CC),
            labelPrimary = Color(0xFF000000),
            labelSecondary = Color(0x993C3C43),
            labelTertiary = Color(0x4D3C3C43),
            labelQuaternary = Color(0x2E3C3C43),
            separator = Color(0x1F3C3C43),
            separatorLight = Color(0x0F3C3C43),
            green = Color(0xFF34C759),
            red = Color(0xFFFF3B30),
            orange = Color(0xFFFF9500),
            blue = Color(0xFF007AFF)
        )
    }
}

internal fun citrosDirectiveFlavorTokens(
    flavor: CitrosFlavor,
    surfaces: CitrosDirectiveSurfaces
): CitrosDirectiveFlavorTokens {
    if (flavor == CitrosFlavor.NONE) {
        val isDarkPalette = surfaces.background == Color(0xFF000000)
        val nonePrimary = Color(0xFF8E8E93)
        val orbColor = if (isDarkPalette) Color(0xFFFFFFFF) else Color(0xFF000000)
        val orbInner = if (isDarkPalette) Color(0xFFD7D9E0) else Color(0xFF2B2C31)
        val orbGlow = if (isDarkPalette) Color(0x0FFFFFFF) else Color(0x0A000000)
        return CitrosDirectiveFlavorTokens(
            orbColor = orbColor,
            orbInner = orbInner,
            orbGlow = orbGlow,
            washColor = Color.Transparent,
            userBubbleBackground = nonePrimary,
            userBubbleText = Color.White,
            sendButtonBackground = nonePrimary,
            sendIconTint = Color.White,
            caretColor = nonePrimary,
            actionIconTint = surfaces.labelTertiary
        )
    }

    val flavorMetadata = when (flavor) {
        CitrosFlavor.NONE -> TangerineFlavor
        CitrosFlavor.LEMON -> LemonFlavor
        CitrosFlavor.TANGERINE -> TangerineFlavor
        CitrosFlavor.LIME -> LimeFlavor
        CitrosFlavor.BLOOD_ORANGE -> BloodOrangeFlavor
        CitrosFlavor.GRAPEFRUIT -> GrapefruitFlavor
    }

    return CitrosDirectiveFlavorTokens(
        orbColor = flavorMetadata.primary,
        orbInner = Color(0x1F000000),
        orbGlow = flavorMetadata.glow,
        washColor = flavorMetadata.wash,
        userBubbleBackground = flavorMetadata.primary,
        userBubbleText = flavorMetadata.onPrimary,
        sendButtonBackground = flavorMetadata.primary,
        sendIconTint = flavorMetadata.onPrimary,
        caretColor = flavorMetadata.primary,
        actionIconTint = surfaces.labelTertiary
    )
}

internal val CitrosGrid: Dp = 4.dp

internal fun cg(multiplier: Int): Dp = CitrosGrid * multiplier

internal fun cg(multiplier: Float): Dp = CitrosGrid * multiplier

internal fun Modifier.citrosFlavorWash(
    washColor: Color?,
    centerXFraction: Float = 0.5f,
    centerYFraction: Float = 0.4f,
    radiusFraction: Float = 0.84f
): Modifier {
    if (washColor == null || washColor.alpha <= 0f) return this
    return drawBehind {
        val center = Offset(
            x = size.width * centerXFraction.coerceIn(0f, 1f),
            y = size.height * centerYFraction.coerceIn(0f, 1f)
        )
        val radius = size.minDimension * radiusFraction.coerceAtLeast(0.1f)
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(washColor, Color.Transparent),
                center = center,
                radius = radius
            ),
            center = center,
            radius = radius
        )
    }
}

@Composable
internal fun CitrosDirectiveOrb(
    flavor: CitrosFlavor,
    size: Dp,
    modifier: Modifier = Modifier,
    colorOverride: Color? = null,
    innerOverride: Color? = null,
    glowOverride: Color? = null
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    val orbColor = colorOverride ?: flavorTokens.orbColor
    val innerColor = innerOverride ?: flavorTokens.orbInner
    val glowColor = glowOverride ?: flavorTokens.orbGlow

    Box(
        modifier = modifier
            .size(size)
            .shadow(
                elevation = size * 0.45f,
                shape = CircleShape,
                clip = false,
                ambientColor = glowColor.copy(alpha = 0.85f),
                spotColor = glowColor
            )
            .background(orbColor, CircleShape),
        contentAlignment = Alignment.Center
    ) {
        Box(
            modifier = Modifier
                .size(size * 0.38f)
                .background(innerColor, CircleShape)
        )
    }
}

@Composable
internal fun CitrosDirectiveWashBox(
    modifier: Modifier = Modifier,
    washColor: Color?,
    centerXFraction: Float = 0.5f,
    centerYFraction: Float = 0.4f,
    radiusFraction: Float = 0.84f,
    contentAlignment: Alignment = Alignment.Center,
    content: @Composable BoxScope.() -> Unit
) {
    Box(
        modifier = modifier.citrosFlavorWash(
            washColor = washColor,
            centerXFraction = centerXFraction,
            centerYFraction = centerYFraction,
            radiusFraction = radiusFraction
        ),
        contentAlignment = contentAlignment,
        content = content
    )
}
