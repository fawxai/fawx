package ai.citros.preview.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.unit.dp
import ai.citros.preview.theme.extendedColors

/**
 * Design system dimensions and spacing constants for Citros.
 * Used throughout the app for consistent layout and component sizing.
 */
object CitrosDimensions {
    // Spacing scale (4dp base unit)
    val spacingXs = 4.dp
    val spacingSm = 8.dp
    val spacingMd = 16.dp
    val spacingLg = 24.dp
    val spacingXl = 32.dp
    val spacingXxl = 48.dp

    // Corner radius
    val cardRadius = 16.dp
    val buttonRadius = 999.dp     // Pill shape
    val chipRadius = 999.dp        // Pill shape
    val inputRadius = 12.dp
    val sheetRadius = 24.dp

    // Elevation (we use borders/shadows instead)
    val cardElevation = 0.dp

    // Icon sizing
    val iconBoxSize = 40.dp
    val iconBoxRadius = 12.dp
}

/**
 * Citros theme for Jetpack Compose Material3.
 * Provides:
 * - Material3 color scheme (dark theme only)
 * - Typography scale (Inter font)
 * - Extended colors via CompositionLocal
 * - Global composition locals for design tokens
 *
 * @param content The composable content to theme
 */
@Composable
fun CitrosTheme(
    content: @Composable () -> Unit,
) {
    val colorScheme = citrosDarkColorScheme()
    val typography = citrosTypography()
    val extendedColors = CitrosExtendedColors()

    CompositionLocalProvider(
        LocalCitrosExtendedColors provides extendedColors,
    ) {
        MaterialTheme(
            colorScheme = colorScheme,
            typography = typography,
            content = content,
        )
    }
}

/**
 * Extension property to access extended colors from MaterialTheme.
 * Provides convenient access to semantic colors like textDim, border, accentGlow, etc.
 *
 * Usage:
 * ```
 * val textDim = MaterialTheme.extendedColors.textDim
 * ```
 */
val MaterialTheme.extendedColors: CitrosExtendedColors
    @Composable
    get() = LocalCitrosExtendedColors.current
