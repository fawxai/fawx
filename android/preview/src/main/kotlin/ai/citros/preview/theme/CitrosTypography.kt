package ai.citros.preview.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * Inter font family with system fallback chain.
 * Font: Inter (designed for optimal readability at all sizes)
 */
val InterFontFamily = FontFamily.Default

/**
 * Creates the Citros typography scale for Material3.
 * Based on Inter font with custom spacing and optical sizing.
 * All text defaults to white (#FFFFFF) unless otherwise specified.
 *
 * @return A Material3 [Typography] configured for the Citros design system
 */
fun citrosTypography(): Typography {
    return Typography(
        // Display styles: largest headings, bold, tight tracking
        displayLarge = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Bold,
            fontSize = 48.sp,
            lineHeight = 56.sp,
            letterSpacing = (-0.5).sp,
            color = CitrosDesignTokens.ColorText,
        ),
        displayMedium = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Bold,
            fontSize = 36.sp,
            lineHeight = 44.sp,
            letterSpacing = (-0.25).sp,
            color = CitrosDesignTokens.ColorText,
        ),
        displaySmall = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 28.sp,
            lineHeight = 36.sp,
            letterSpacing = 0.sp,
            color = CitrosDesignTokens.ColorText,
        ),

        // Headline styles: section titles, bold to semibold
        headlineLarge = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Bold,
            fontSize = 28.sp,
            lineHeight = 36.sp,
            letterSpacing = (-0.25).sp,
            color = CitrosDesignTokens.ColorText,
        ),
        headlineMedium = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 24.sp,
            lineHeight = 32.sp,
            letterSpacing = 0.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        headlineSmall = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 20.sp,
            lineHeight = 28.sp,
            letterSpacing = 0.sp,
            color = CitrosDesignTokens.ColorText,
        ),

        // Title styles: secondary headings, semibold
        titleLarge = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 20.sp,
            lineHeight = 28.sp,
            letterSpacing = 0.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        titleMedium = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 16.sp,
            lineHeight = 24.sp,
            letterSpacing = 0.15.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        titleSmall = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            letterSpacing = 0.1.sp,
            color = CitrosDesignTokens.ColorText,
        ),

        // Body styles: main content text
        bodyLarge = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 16.sp,
            lineHeight = 24.sp,
            letterSpacing = 0.15.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        bodyMedium = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            letterSpacing = 0.25.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        bodySmall = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 12.sp,
            lineHeight = 16.sp,
            letterSpacing = 0.4.sp,
            color = CitrosDesignTokens.ColorTextDim, // Dimmer text for secondary content
        ),

        // Label styles: buttons, badges, chips (semibold for emphasis)
        labelLarge = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            letterSpacing = 0.1.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        labelMedium = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 12.sp,
            lineHeight = 16.sp,
            letterSpacing = 0.5.sp,
            color = CitrosDesignTokens.ColorText,
        ),
        labelSmall = TextStyle(
            fontFamily = InterFontFamily,
            fontWeight = FontWeight.Medium,
            fontSize = 10.sp,
            lineHeight = 14.sp,
            letterSpacing = 0.5.sp,
            color = CitrosDesignTokens.ColorTextDim, // Dimmer text for secondary labels
        ),
    )
}
