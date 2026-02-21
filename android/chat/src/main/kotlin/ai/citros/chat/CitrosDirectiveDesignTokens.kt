package ai.citros.chat

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

private val CitrosFont = FontFamily.Default

@Stable
internal object CitrosTypography {
    val largeTitle = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 34.sp,
        lineHeight = 40.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.8).sp
    )
    val title1 = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 28.sp,
        lineHeight = 34.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.8).sp
    )
    val title2 = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 24.sp,
        lineHeight = 30.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.6).sp
    )
    val title3 = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 20.sp,
        lineHeight = 26.sp,
        fontWeight = FontWeight.SemiBold,
        letterSpacing = (-0.4).sp
    )
    val headline = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 17.sp,
        lineHeight = 22.sp,
        fontWeight = FontWeight.SemiBold,
        letterSpacing = (-0.4).sp
    )
    val body = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 16.sp,
        lineHeight = 22.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = (-0.2).sp
    )
    val callout = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 15.sp,
        lineHeight = 22.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = (-0.2).sp
    )
    val subheadline = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = (-0.15).sp
    )
    val footnote = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 13.sp,
        lineHeight = 18.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = (-0.1).sp
    )
    val caption1 = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = (-0.1).sp
    )
    val caption2 = TextStyle(
        fontFamily = CitrosFont,
        fontSize = 11.sp,
        lineHeight = 16.sp,
        fontWeight = FontWeight.Normal,
        letterSpacing = 0.sp
    )

    val displayLarge: TextStyle = largeTitle
    val displayMedium: TextStyle = title1
    val displaySmall: TextStyle = title1
    val headlineLarge: TextStyle = title1
    val headlineMedium: TextStyle = title2
    val headlineSmall: TextStyle = title3
    val titleLarge: TextStyle = title2
    val titleMedium: TextStyle = title3
    val titleSmall: TextStyle = headline
    val bodyLarge: TextStyle = body
    val bodyMedium: TextStyle = callout
    val bodySmall: TextStyle = subheadline
    val labelLarge: TextStyle = headline
    val labelMedium: TextStyle = subheadline
    val labelSmall: TextStyle = footnote
}

@Stable
internal object CitrosColorScheme {
    @Composable
    private fun surfaces(): CitrosDirectiveSurfaces {
        val isDark = LocalCitrosIsDark.current
        return remember(isDark) { citrosDirectiveSurfaces(isDark) }
    }

    val background: Color
        @Composable get() = surfaces().background

    val surface: Color
        @Composable get() = surfaces().surface1

    val surfaceVariant: Color
        @Composable get() = surfaces().surface2

    val surfaceContainer: Color
        @Composable get() = surfaces().surface3

    val onBackground: Color
        @Composable get() = surfaces().labelPrimary

    val onSurface: Color
        @Composable get() = surfaces().labelPrimary

    val onSurfaceVariant: Color
        @Composable get() = surfaces().labelSecondary

    val outline: Color
        @Composable get() = surfaces().separator

    val outlineVariant: Color
        @Composable get() = surfaces().separatorLight

    val error: Color
        @Composable get() = surfaces().red

    val primary: Color
        @Composable get() = LocalCitrosFlavor.current.primary

    val secondary: Color
        @Composable get() = primary

    val tertiary: Color
        @Composable get() = primary

    val primaryContainer: Color
        @Composable get() = surfaces().surface2

    val onPrimaryContainer: Color
        @Composable get() = surfaces().labelPrimary

    val errorContainer: Color
        @Composable get() = if (LocalCitrosIsDark.current) {
            surfaces().red.copy(alpha = 0.25f)
        } else {
            surfaces().red.copy(alpha = 0.18f)
        }

    val onErrorContainer: Color
        @Composable get() = surfaces().red
}
