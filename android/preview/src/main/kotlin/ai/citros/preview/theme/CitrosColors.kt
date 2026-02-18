package ai.citros.preview.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocal
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import ai.citros.preview.theme.extendedColors

/**
 * Design tokens for all raw color values extracted from citros.ai.
 * Single source of truth for the color palette.
 */
object CitrosDesignTokens {
    // Base colors
    val ColorBackground = Color(0xFF050505)
    val ColorSurface = Color(0xFF0A0A0A)
    val ColorSurfaceVariant = Color(0xFF111111)
    val ColorText = Color(0xFFFFFFFF)

    // Accent colors
    val ColorPrimary = Color(0xFFF59E0B)        // Amber/gold
    val ColorSecondary = Color(0xFFFF6B2B)      // Warm orange
    val ColorTertiary = Color(0xFFFFD600)       // Golden yellow
    val ColorError = Color(0xFFEF4444)

    // Dark brown for text on primary
    val ColorOnPrimary = Color(0xFF1A0A00)

    // Text variations
    val ColorTextDim = Color(0x80FFFFFF)        // 50% opacity
    val ColorTextFaint = Color(0x40FFFFFF)      // 25% opacity

    // Accent glows
    val ColorAccentGlow = Color(0x26F59E0B)     // 15% opacity amber
    val ColorAccentGlowStrong = Color(0x4DF59E0B) // 30% opacity amber

    // Borders
    val ColorBorder = Color(0x1FFFFFFF)         // 12% white
    val ColorBorderSubtle = Color(0x0FFFFFFF)   // 6% white

    // Card surfaces
    val ColorCardSurface = Color(0x0AFFFFFF)    // 4% white over background
    val ColorCardSurfaceElevated = Color(0x14FFFFFF) // 8% white
    val ColorShimmer = Color(0x0DFFFFFF)        // ~5% white
}

/**
 * Extended colors not available in Material3 ColorScheme.
 * Provides access to semantic colors like textDim, textFaint, borders, etc.
 */
data class CitrosExtendedColors(
    val textDim: Color = CitrosDesignTokens.ColorTextDim,
    val textFaint: Color = CitrosDesignTokens.ColorTextFaint,
    val accentGlow: Color = CitrosDesignTokens.ColorAccentGlow,
    val accentGlowStrong: Color = CitrosDesignTokens.ColorAccentGlowStrong,
    val border: Color = CitrosDesignTokens.ColorBorder,
    val borderSubtle: Color = CitrosDesignTokens.ColorBorderSubtle,
    val cardSurface: Color = CitrosDesignTokens.ColorCardSurface,
    val cardSurfaceElevated: Color = CitrosDesignTokens.ColorCardSurfaceElevated,
    val shimmer: Color = CitrosDesignTokens.ColorShimmer,
)

/**
 * CompositionLocal for extended colors.
 * Accessed via [androidx.compose.material3.MaterialTheme.extendedColors]
 */
val LocalCitrosExtendedColors = staticCompositionLocalOf {
    CitrosExtendedColors()
}

/**
 * Overlay colors for UI chrome, cards, inputs, and status bars.
 * Maintains consistent semantic naming with the rest of the color system.
 */
object OverlayColors {
    val AppChrome = Color(0xFF080808)
    val PreviewBackground = Color(0xFF050505)
    val CardBackground = Color(0xFF0C0C0C)
    val CardBorder = Color(0x1FFFFFFF)          // 12% white
    val StatusBar = Color(0xFF080808)
    val InputBackground = Color(0xFF0A0A0A)
    val InputBorder = Color(0x1FFFFFFF)         // 12% white
}

/**
 * Creates the Citros dark color scheme for Material3.
 * Maps semantic color slots to the citros.ai palette.
 *
 * @return A Material3 [ColorScheme] configured for the Citros dark theme
 */
fun citrosDarkColorScheme(): ColorScheme {
    return darkColorScheme(
        // Fundamental colors
        background = Color(0xFF050505),
        onBackground = Color(0xFFFFFFFF),

        // Surface hierarchy (slight white overlays create depth)
        surface = Color(0xFF0A0A0A),
        onSurface = Color(0xFFFFFFFF),
        surfaceVariant = Color(0xFF111111),
        onSurfaceVariant = Color(0xB3FFFFFF),   // 70% white

        // Container surfaces for elevated components
        surfaceContainerLowest = Color(0xFF050505),
        surfaceContainerLow = Color(0xFF0A0A0A),
        surfaceContainer = Color(0xFF0F0F0F),
        surfaceContainerHigh = Color(0xFF141414),
        surfaceContainerHighest = Color(0xFF1A1A1A),

        // Primary accent (amber/gold)
        primary = Color(0xFFF59E0B),
        onPrimary = Color(0xFF1A0A00),
        primaryContainer = Color(0xFF261600),
        onPrimaryContainer = Color(0xFFFDD835),

        // Secondary accent (warm orange)
        secondary = Color(0xFFFF6B2B),
        onSecondary = Color(0xFF1A0A00),
        secondaryContainer = Color(0xFF1A1000),
        onSecondaryContainer = Color(0xFFFFAB91),

        // Tertiary (golden yellow)
        tertiary = Color(0xFFFFD600),
        onTertiary = Color(0xFF332B00),

        // Error state
        error = Color(0xFFEF4444),
        onError = Color(0xFF1A0000),
        errorContainer = Color(0xFF5F0000),
        onErrorContainer = Color(0xFFFFDADA),

        // Outlines and borders
        outline = Color(0x1FFFFFFF),             // 12% white
        outlineVariant = Color(0x0FFFFFFF),      // 6% white

        // Inverse colors (for snackbars, etc.)
        inverseSurface = Color(0xFFE0E0E0),
        inverseOnSurface = Color(0xFF050505),
        inversePrimary = Color(0xFF8B6B00),

        // Scrim (for modals/overlays)
        scrim = Color(0xFF000000),
    )
}
