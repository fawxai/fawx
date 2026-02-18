package ai.citros.preview.components

import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import ai.citros.preview.theme.CitrosFlavor
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.PI
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.animation.core.animateFloat

/**
 * Animated hero sphere with pulsing glow and orbiting dots.
 * Renders a premium minimal sphere with radial gradient, pulsing outer ring,
 * and three orbiting dots at different speeds.
 */
@Composable
fun CitrosHeroSphere(
    flavor: CitrosFlavor,
    size: Dp = 200.dp,
    modifier: Modifier = Modifier
) {
    val infiniteTransition = androidx.compose.animation.core.rememberInfiniteTransition(label = "sphere_glow")

    // Pulsing glow alpha: 0.15 → 0.4 → 0.15, 3s duration
    val glowAlpha by infiniteTransition.animateFloat(
        initialValue = 0.15f,
        targetValue = 0.4f,
        animationSpec = infiniteRepeatable(
            animation = keyframes {
                durationMillis = 3000
                0.15f at 0
                0.4f at 1500
                0.15f at 3000
            }
        ),
        label = "glow_pulse"
    )

    // Three orbiting dots at different speeds/radii
    val orbitAngle1 by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = keyframes {
                durationMillis = 8000
                0f at 0
                360f at 8000
            }
        ),
        label = "orbit1"
    )

    val orbitAngle2 by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = keyframes {
                durationMillis = 12000
                0f at 0
                360f at 12000
            }
        ),
        label = "orbit2"
    )

    val orbitAngle3 by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = keyframes {
                durationMillis = 16000
                0f at 0
                360f at 16000
            }
        ),
        label = "orbit3"
    )

    Box(
        modifier = modifier.size(size),
        contentAlignment = Alignment.Center
    ) {
        Canvas(modifier = Modifier.size(size)) {
            val centerX = this.size.width / 2f
            val centerY = this.size.height / 2f
            val radius = this.size.width / 2f

            // Ambient glow background (larger, 10% alpha)
            drawCircle(
                color = flavor.primary.copy(alpha = 0.1f),
                radius = radius * 1.3f,
                center = Offset(centerX, centerY)
            )

            // Main sphere with radial gradient
            drawCircle(
                brush = Brush.radialGradient(
                    colors = listOf(
                        flavor.glow.copy(alpha = 0.6f),
                        flavor.primary.copy(alpha = 0.3f),
                        Color.Transparent
                    ),
                    center = Offset(centerX, centerY),
                    radius = radius
                ),
                radius = radius,
                center = Offset(centerX, centerY)
            )

            // Pulsing outer glow ring
            drawCircle(
                color = flavor.primary.copy(alpha = glowAlpha),
                radius = radius * 1.15f,
                center = Offset(centerX, centerY),
                style = Stroke(width = 2f)
            )

            // Orbiting dot 1 (small radius, fast)
            drawOrbitingDot(
                centerX, centerY, radius * 0.4f, orbitAngle1.toDouble(),
                flavor.glow, 6f
            )

            // Orbiting dot 2 (medium radius, medium speed)
            drawOrbitingDot(
                centerX, centerY, radius * 0.6f, orbitAngle2.toDouble(),
                flavor.glow, 5f
            )

            // Orbiting dot 3 (large radius, slow)
            drawOrbitingDot(
                centerX, centerY, radius * 0.8f, orbitAngle3.toDouble(),
                flavor.glow, 4f
            )
        }
    }
}

/**
 * Helper to draw an orbiting dot.
 */
private fun DrawScope.drawOrbitingDot(
    centerX: Float,
    centerY: Float,
    radius: Float,
    angleRadians: Double,
    color: Color,
    dotRadius: Float
) {
    val x = centerX + (radius * cos(angleRadians * PI / 180)).toFloat()
    val y = centerY + (radius * sin(angleRadians * PI / 180)).toFloat()

    drawCircle(
        color = color,
        radius = dotRadius,
        center = Offset(x, y)
    )
}
