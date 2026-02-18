package ai.citros.preview.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.unit.dp
import ai.citros.preview.theme.CitrosFlavor

/**
 * Simple radial gradient badge with drop shadow glow.
 * Displays a circular gradient badge with the flavor's glow and primary colors.
 */
@Composable
fun CitrusHeroBadge(
    flavor: CitrosFlavor,
    size: Int = 68
) {
    val sizeDp = size.dp

    Box(
        modifier = Modifier.size(sizeDp),
        contentAlignment = Alignment.Center
    ) {
        Canvas(modifier = Modifier.size(sizeDp)) {
            val radius = size / 2f

            // Drop shadow glow behind badge (20% alpha)
            drawCircle(
                color = flavor.primary.copy(alpha = 0.2f),
                radius = radius * 1.2f,
                center = Offset(size / 2f, size / 2f)
            )

            // Main badge: radial gradient
            drawCircle(
                brush = Brush.radialGradient(
                    colors = listOf(
                        flavor.glow,
                        flavor.primary,
                        Color.Transparent
                    ),
                    center = Offset(size / 2f, size / 2f),
                    radius = radius
                ),
                radius = radius,
                center = Offset(size / 2f, size / 2f)
            )
        }
    }
}
