package ai.citros.chat

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.cos
import kotlin.math.sin
import kotlin.random.Random

internal enum class CitrosFlavor(
    val storageValue: String,
    val displayName: String,
    val primary: Color,
    val glow: Color,
    val tint: Color
) {
    LEMON(
        storageValue = "lemon",
        displayName = "Lemon",
        primary = Color(0xFFFFD600),
        glow = Color(0xFFFFF9C4),
        tint = Color(0xFF332B00)
    ),
    TANGERINE(
        storageValue = "tangerine",
        displayName = "Tangerine",
        primary = Color(0xFFFF8C00),
        glow = Color(0xFFFFE0B2),
        tint = Color(0xFF331C00)
    ),
    LIME(
        storageValue = "lime",
        displayName = "Lime",
        primary = Color(0xFF7CB342),
        glow = Color(0xFFDCEDC8),
        tint = Color(0xFF1A2E0D)
    ),
    BLOOD_ORANGE(
        storageValue = "blood_orange",
        displayName = "Blood Orange",
        primary = Color(0xFFD84315),
        glow = Color(0xFFFFCCBC),
        tint = Color(0xFF2E0D04)
    ),
    GRAPEFRUIT(
        storageValue = "grapefruit",
        displayName = "Grapefruit",
        primary = Color(0xFFE91E63),
        glow = Color(0xFFF8BBD0),
        tint = Color(0xFF2E0413)
    );

    companion object {
        fun fromStorage(value: String?): CitrosFlavor {
            return entries.firstOrNull { it.storageValue == value } ?: TANGERINE
        }
    }
}

internal data class CitrosPlanSpec(
    val id: String,
    val title: String,
    val subtitle: String,
    val details: String,
    val cta: String,
    val accent: Color,
    val recommended: Boolean = false,
    val comingSoon: Boolean = false
)

@Composable
internal fun CitrosStepHeader(
    title: String,
    stepIndex: Int,
    totalSteps: Int,
    onBack: (() -> Unit)? = null,
    titleColor: Color = MaterialTheme.colorScheme.onBackground,
    backLabelColor: Color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.75f),
    stepCounterColor: Color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.65f),
    activeProgressColor: Color = MaterialTheme.colorScheme.primary,
    inactiveProgressColor: Color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.16f),
    titleShadow: Shadow? = null,
    centerTitle: Boolean = false,
    modifier: Modifier = Modifier
) {
    Column(modifier = modifier.fillMaxWidth()) {
        if (centerTitle) {
            Box(
                modifier = Modifier.fillMaxWidth(),
                contentAlignment = Alignment.Center
            ) {
                if (onBack != null) {
                    Text(
                        text = "Back",
                        style = MaterialTheme.typography.labelLarge,
                        color = backLabelColor,
                        modifier = Modifier
                            .align(Alignment.CenterStart)
                            .clickable(onClick = onBack)
                    )
                }
                Text(
                    text = title,
                    style = MaterialTheme.typography.headlineSmall.copy(
                        shadow = titleShadow
                    ),
                    fontWeight = FontWeight.SemiBold,
                    color = titleColor
                )
                Text(
                    text = "$stepIndex/$totalSteps",
                    style = MaterialTheme.typography.labelMedium,
                    color = stepCounterColor,
                    modifier = Modifier.align(Alignment.CenterEnd)
                )
            }
        } else {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (onBack != null) {
                    Text(
                        text = "Back",
                        style = MaterialTheme.typography.labelLarge,
                        color = backLabelColor,
                        modifier = Modifier.clickable(onClick = onBack)
                    )
                    Spacer(Modifier.width(12.dp))
                }
                Text(
                    text = title,
                    style = MaterialTheme.typography.headlineSmall.copy(
                        shadow = titleShadow
                    ),
                    fontWeight = FontWeight.SemiBold,
                    color = titleColor
                )
                Spacer(Modifier.weight(1f))
                Text(
                    text = "$stepIndex/$totalSteps",
                    style = MaterialTheme.typography.labelMedium,
                    color = stepCounterColor
                )
            }
        }
        Spacer(Modifier.height(12.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            repeat(totalSteps) { index ->
                val active = index < stepIndex
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(4.dp)
                        .clip(CircleShape)
                        .background(
                            if (active) {
                                activeProgressColor
                            } else {
                                inactiveProgressColor
                            }
                        )
                )
            }
        }
    }
}

@Composable
internal fun CitrusHeroBadge(
    flavor: CitrosFlavor,
    size: Int = 68
) {
    Box(
        modifier = Modifier
            .size(size.dp)
            .clip(CircleShape)
            .background(
                brush = Brush.radialGradient(
                    colors = listOf(flavor.glow, flavor.primary, flavor.tint)
                )
            )
    )
}

private data class HeroDustParticle(
    val baseAngleDeg: Float,
    val orbitScale: Float,
    val sizeScale: Float,
    val speedScale: Float,
    val verticalScale: Float,
    val baseAlpha: Float,
    val alphaSwing: Float,
    val flickerOffset: Float
)

private data class HeroBokehParticle(
    val baseAngleDeg: Float,
    val orbitScale: Float,
    val radiusScale: Float,
    val speedScale: Float,
    val baseAlpha: Float,
    val flickerOffset: Float
)

private data class HeroFacetPoint(
    val baseAngleDeg: Float,
    val radiusScale: Float,
    val wobbleScale: Float,
    val wobblePhaseA: Float,
    val wobblePhaseB: Float
)

private data class FloatingBackdropSprite(
    val baseX: Float,
    val baseY: Float,
    val sizeScale: Float,
    val speedX: Float,
    val speedY: Float,
    val alpha: Float,
    val phaseOffset: Float
)

@Composable
internal fun CitrosFloatingSpriteBackdrop(
    flavor: CitrosFlavor,
    modifier: Modifier = Modifier,
    density: Float = 1f,
    alpha: Float = 1f
) {
    val clampedDensity = density.coerceIn(0.2f, 2.2f)
    val clampedAlpha = alpha.coerceIn(0f, 1f)
    val particleCount = remember(clampedDensity) {
        (30f * clampedDensity).toInt().coerceIn(12, 84)
    }
    val particles = remember(flavor, particleCount) {
        buildFloatingBackdropSprites(flavor = flavor, count = particleCount)
    }
    val transition = rememberInfiniteTransition(label = "citros_topbar_floaters")
    val elapsedSeconds by transition.animateFloat(
        initialValue = 0f,
        targetValue = 120f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 120000, easing = LinearEasing)
        ),
        label = "citros_topbar_floaters_elapsed"
    )
    val glow = remember(flavor) { lerp(flavor.glow, Color.White, 0.35f) }
    val warm = remember(flavor) { lerp(flavor.primary, Color.White, 0.08f) }

    Canvas(modifier = modifier.graphicsLayer(alpha = clampedAlpha)) {
        val width = size.width
        val height = size.height
        if (width <= 0f || height <= 0f) return@Canvas

        particles.forEach { particle ->
            val xNorm = ((particle.baseX + (elapsedSeconds * particle.speedX)) % 1f + 1f) % 1f
            val yDrift = sin((elapsedSeconds * particle.speedY) + particle.phaseOffset) * 0.08f
            val yNorm = (particle.baseY + yDrift).coerceIn(0.04f, 0.96f)
            val center = Offset(x = width * xNorm, y = height * yNorm)
            // Keep top-bar sprites roughly half the prior visual size.
            val radius = (height * (0.010f + particle.sizeScale * 0.028f)).coerceAtLeast(0.9f)
            val flicker = (
                particle.alpha +
                    sin((elapsedSeconds * 0.82f) + particle.phaseOffset) * 0.12f
                ).coerceIn(0.08f, 0.76f)
            val spriteRadius = radius * 2.8f

            drawCircle(
                color = glow.copy(alpha = flicker * 0.24f),
                center = center,
                radius = spriteRadius
            )
            drawCircle(
                color = warm.copy(alpha = flicker * 0.50f),
                center = center,
                radius = spriteRadius * 0.54f
            )
        }
    }
}

@Composable
internal fun CitrosFloatingAppIconGraphic(
    flavor: CitrosFlavor,
    modifier: Modifier = Modifier,
    size: Dp = 58.dp,
    cornerRadius: Dp = 14.dp,
    backgroundAlpha: Float = 0.96f,
    showBackground: Boolean = true,
    orbOnly: Boolean = false
) {
    val transition = rememberInfiniteTransition(label = "citros_floating_icon")
    val elapsedSeconds by transition.animateFloat(
        initialValue = 0f,
        targetValue = 72f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 72000, easing = LinearEasing)
        ),
        label = "citros_floating_icon_elapsed_seconds"
    )
    val iconRes = remember(flavor) { launcherIconForegroundResForFlavor(flavor) }
    val pulse = 1f + sin(elapsedSeconds * 0.46f) * 0.020f
    val swayX = cos(elapsedSeconds * 0.34f) * 1.4f
    val bobY = sin(elapsedSeconds * 0.56f) * 1.9f
    val wobble = sin(elapsedSeconds * 0.39f) * 1.6f
    val frameShape = RoundedCornerShape(cornerRadius)
    val iconShape = if (orbOnly) CircleShape else frameShape
    val iconFrameSize = if (showBackground) size * 0.72f else size * 0.92f
    val isDarkTheme = LocalCitrosIsDark.current

    Box(
        modifier = modifier
            .size(size)
            .graphicsLayer {
                translationX = swayX
                translationY = bobY
                scaleX = pulse
                scaleY = pulse
                rotationZ = wobble
            },
        contentAlignment = Alignment.Center
    ) {
        if (showBackground) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .clip(frameShape)
            ) {
                CitrosHeroShaderSphere(
                    flavor = flavor,
                    modifier = Modifier.fillMaxSize()
                )
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(
                            Brush.radialGradient(
                                colors = listOf(
                                    Color.Transparent,
                                    if (isDarkTheme) {
                                        Color.Black.copy(alpha = 0.38f * backgroundAlpha)
                                    } else {
                                        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.16f * backgroundAlpha)
                                    }
                                ),
                                radius = Float.POSITIVE_INFINITY
                            )
                        )
                )
            }

            CitrosLiquidGlassSurface(
                modifier = Modifier.size(iconFrameSize),
                shape = RoundedCornerShape(cornerRadius),
                baseColor = if (isDarkTheme) {
                    Color.Black.copy(alpha = 0.24f * backgroundAlpha)
                } else {
                    MaterialTheme.colorScheme.surface.copy(alpha = 0.82f * backgroundAlpha)
                },
                borderColor = flavor.primary.copy(alpha = 0.34f * backgroundAlpha),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 1.08f
            ) {
                Image(
                    painter = painterResource(id = iconRes),
                    contentDescription = "Citros app icon",
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop
                )
            }
        } else {
            if (orbOnly) {
                Box(
                    modifier = Modifier
                        .size(iconFrameSize)
                        .clip(CircleShape)
                ) {
                    CitrosHeroShaderSphere(
                        flavor = flavor,
                        modifier = Modifier.fillMaxSize(),
                        particleSizeScale = 0.5f,
                        clipCircle = true
                    )
                }
            } else {
                Image(
                    painter = painterResource(id = iconRes),
                    contentDescription = "Citros app icon",
                    modifier = Modifier
                        .size(iconFrameSize)
                        .clip(iconShape),
                    contentScale = ContentScale.Crop
                )
            }
        }
    }
}

@Composable
internal fun CitrosHeroSphere(
    flavor: CitrosFlavor,
    size: Dp = 200.dp,
    modifier: Modifier = Modifier
) {
    val transition = rememberInfiniteTransition(label = "citros_hero_sphere")
    val elapsedSeconds by transition.animateFloat(
        initialValue = 0f,
        targetValue = 120f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 120000, easing = LinearEasing)
        ),
        label = "hero_elapsed_seconds"
    )
    val dustCount = remember(size) {
        when {
            size >= 180.dp -> 56
            size >= 130.dp -> 38
            else -> 26
        }
    }
    val bokehBackCount = remember(size) {
        when {
            size >= 180.dp -> 24
            size >= 130.dp -> 12
            else -> 8
        }
    }
    val bokehFrontCount = remember(size) {
        when {
            size >= 180.dp -> 16
            size >= 130.dp -> 8
            else -> 5
        }
    }
    val facetPoints = remember(flavor) {
        buildHeroFacetPoints(flavor = flavor, count = 22)
    }
    val dustParticles = remember(flavor, dustCount) {
        buildHeroDustParticles(flavor = flavor, count = dustCount)
    }
    val backBokeh = remember(flavor, bokehBackCount) {
        buildHeroBokehParticles(flavor = flavor, count = bokehBackCount, layerSeed = 1_009)
    }
    val frontBokeh = remember(flavor, bokehFrontCount) {
        buildHeroBokehParticles(flavor = flavor, count = bokehFrontCount, layerSeed = 2_017)
    }
    val deepCore = remember(flavor) {
        lerp(Color(0xFF130800), flavor.tint, 0.22f)
    }
    val primary = remember(flavor) {
        lerp(Color(0xFFF59E0B), flavor.primary, 0.22f)
    }
    val highlightAmber = remember(flavor) {
        lerp(Color(0xFFFFC53A), flavor.primary, 0.20f)
    }
    val warmAccent = remember(flavor) {
        lerp(Color(0xFFFF7C1E), flavor.primary, 0.18f)
    }
    val wireColor = remember(flavor) {
        lerp(Color(0xFF4C240A), flavor.tint, 0.50f)
    }
    val pulse = 1f + (sin(elapsedSeconds * 0.4f) * 0.018f)

    Canvas(
        modifier = modifier
            .size(size)
            .graphicsLayer {
                scaleX = pulse
                scaleY = pulse
            }
    ) {
        val c = center
        val minDim = minOf(this.size.width, this.size.height)
        val wobbleX = cos(elapsedSeconds * 0.08f) * 0.04f
        val wobbleY = sin(elapsedSeconds * 0.05f) * 0.06f
        val orbCenter = Offset(
            x = c.x + (minDim * wobbleX * 0.22f),
            y = c.y + (minDim * wobbleY * 0.22f)
        )
        val coreRadius = minDim * 0.41f
        val ringStroke = coreRadius * 0.010f
        val surfacePhase = elapsedSeconds * 0.27f
        val ring1Rotation = (elapsedSeconds * 0.12f * RAD_TO_DEG) + 18f
        val ring2Rotation = (elapsedSeconds * -0.08f * RAD_TO_DEG) + 62f
        val ring3Rotation = (elapsedSeconds * 0.05f * RAD_TO_DEG) + 130f
        val particleRotationDeg = elapsedSeconds * 0.03f * RAD_TO_DEG
        val particleTiltMod = sin(elapsedSeconds * 0.02f) * 0.08f

        drawHeroBokehLayer(
            particles = backBokeh,
            center = orbCenter,
            coreRadius = coreRadius,
            elapsedSeconds = elapsedSeconds,
            primary = highlightAmber,
            warmAccent = warmAccent,
            alphaMultiplier = 1.05f
        )

        // Deep ambient shell behind orb.
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(
                    primary.copy(alpha = 0.22f),
                    warmAccent.copy(alpha = 0.12f),
                    Color.Transparent
                ),
                center = orbCenter,
                radius = coreRadius * 2.10f
            ),
            radius = coreRadius * 2.10f,
            center = orbCenter
        )

        val vertices = buildAnimatedHeroVertices(
            points = facetPoints,
            center = orbCenter,
            radius = coreRadius,
            elapsedSeconds = elapsedSeconds
        )
        val blobPath = buildHeroPolygonPath(vertices)

        drawPath(
            path = blobPath,
            brush = Brush.radialGradient(
                colors = listOf(
                    deepCore.copy(alpha = 0.97f),
                    primary.copy(alpha = 0.95f),
                    warmAccent.copy(alpha = 0.94f)
                ),
                center = Offset(
                    x = orbCenter.x - (coreRadius * 0.16f),
                    y = orbCenter.y - (coreRadius * 0.14f)
                ),
                radius = coreRadius * 1.52f
            )
        )

        // Bright top-left diffuse highlight for the blown-out glow look.
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(
                    highlightAmber.copy(alpha = 0.34f),
                    Color.Transparent
                ),
                center = Offset(
                    x = orbCenter.x - (coreRadius * 0.42f),
                    y = orbCenter.y - (coreRadius * 0.36f)
                ),
                radius = coreRadius * 0.92f
            ),
            radius = coreRadius * 0.92f,
            center = Offset(
                x = orbCenter.x - (coreRadius * 0.42f),
                y = orbCenter.y - (coreRadius * 0.36f)
            )
        )

        // Central dark well.
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(
                    deepCore.copy(alpha = 0.58f),
                    deepCore.copy(alpha = 0.36f),
                    Color.Transparent
                ),
                center = orbCenter,
                radius = coreRadius * 0.44f
            ),
            radius = coreRadius * 0.44f,
            center = orbCenter
        )

        // Facet/wire triangulation.
        drawHeroFacetLines(
            vertices = vertices,
            center = orbCenter,
            edgeColor = wireColor.copy(alpha = 0.22f),
            lineColor = wireColor.copy(alpha = 0.16f),
            strokeWidth = ringStroke * 0.95f
        )

        // Outer orb contour.
        drawPath(
            path = blobPath,
            color = warmAccent.copy(alpha = 0.24f),
            style = Stroke(width = ringStroke * 1.18f)
        )

        dustParticles.forEach { particle ->
            val angleRad = (
                particle.baseAngleDeg + (particleRotationDeg * particle.speedScale)
                ) * DEG_TO_RAD
            val orbit = coreRadius * particle.orbitScale
            val yScale = (particle.verticalScale + (particleTiltMod * 0.35f)).coerceIn(0.54f, 1f)
            val x = orbCenter.x + cos(angleRad) * orbit
            val y = orbCenter.y + sin(angleRad) * orbit * yScale
            val alpha = (
                particle.baseAlpha +
                    sin((elapsedSeconds * 0.8f) + particle.flickerOffset) * particle.alphaSwing
                ).coerceIn(0.04f, 0.32f)

            drawCircle(
                color = highlightAmber.copy(alpha = alpha),
                radius = coreRadius * particle.sizeScale,
                center = Offset(x, y)
            )
        }

        drawHeroRing(
            center = orbCenter,
            radius = coreRadius * 1.57f,
            tilt = 0.62f,
            rotationDeg = ring1Rotation,
            color = warmAccent.copy(alpha = 0.12f),
            strokeWidth = ringStroke
        )
        drawHeroRing(
            center = orbCenter,
            radius = coreRadius * 1.62f,
            tilt = 0.40f,
            rotationDeg = ring2Rotation,
            color = highlightAmber.copy(alpha = 0.10f),
            strokeWidth = ringStroke
        )
        drawHeroRing(
            center = orbCenter,
            radius = coreRadius * 2.02f,
            tilt = 0.56f,
            rotationDeg = ring3Rotation,
            color = warmAccent.copy(alpha = 0.08f),
            strokeWidth = ringStroke * 0.82f
        )

        // Fresnel-ish edge lift.
        drawCircle(
            brush = Brush.radialGradient(
                colorStops = arrayOf(
                    0.54f to Color.Transparent,
                    0.82f to primary.copy(alpha = 0.22f),
                    1f to warmAccent.copy(alpha = 0.28f)
                ),
                center = orbCenter,
                radius = coreRadius * 1.10f
            ),
            radius = coreRadius * 1.10f,
            center = orbCenter
        )

        drawHeroBokehLayer(
            particles = frontBokeh,
            center = orbCenter,
            coreRadius = coreRadius,
            elapsedSeconds = elapsedSeconds + 17f,
            primary = highlightAmber,
            warmAccent = warmAccent,
            alphaMultiplier = 1.20f
        )
    }
}

private fun DrawScope.drawHeroRing(
    center: Offset,
    radius: Float,
    tilt: Float,
    rotationDeg: Float,
    color: Color,
    strokeWidth: Float
) {
    withTransform({
        scale(scaleX = 1f, scaleY = tilt, pivot = center)
        rotate(degrees = rotationDeg, pivot = center)
    }) {
        drawCircle(
            color = color,
            radius = radius,
            center = center,
            style = Stroke(width = strokeWidth, cap = StrokeCap.Round)
        )
    }
}

private fun buildHeroPolygonPath(vertices: List<Offset>): Path {
    val path = Path()
    vertices.forEachIndexed { index, vertex ->
        if (index == 0) {
            path.moveTo(vertex.x, vertex.y)
        } else {
            path.lineTo(vertex.x, vertex.y)
        }
    }
    path.close()
    return path
}

private fun DrawScope.drawHeroFacetLines(
    vertices: List<Offset>,
    center: Offset,
    edgeColor: Color,
    lineColor: Color,
    strokeWidth: Float
) {
    if (vertices.isEmpty()) return
    vertices.forEachIndexed { index, point ->
        val next = vertices[(index + 1) % vertices.size]
        drawLine(
            color = edgeColor,
            start = point,
            end = next,
            strokeWidth = strokeWidth
        )
        if (index % 2 == 0) {
            drawLine(
                color = lineColor,
                start = point,
                end = center,
                strokeWidth = strokeWidth * 0.72f
            )
        }
        val across = vertices[(index + 6) % vertices.size]
        if (index % 3 == 0) {
            drawLine(
                color = lineColor.copy(alpha = lineColor.alpha * 0.8f),
                start = point,
                end = across,
                strokeWidth = strokeWidth * 0.68f
            )
        }
    }
}

private fun DrawScope.drawHeroBokehLayer(
    particles: List<HeroBokehParticle>,
    center: Offset,
    coreRadius: Float,
    elapsedSeconds: Float,
    primary: Color,
    warmAccent: Color,
    alphaMultiplier: Float
) {
    particles.forEach { particle ->
        val angle = (
            particle.baseAngleDeg + (elapsedSeconds * particle.speedScale * RAD_TO_DEG)
            ) * DEG_TO_RAD
        val orbit = coreRadius * particle.orbitScale
        val px = center.x + cos(angle) * orbit
        val py = center.y + sin(angle) * orbit * 0.82f
        val radius = coreRadius * particle.radiusScale *
            (1f + sin((elapsedSeconds * 0.41f) + particle.flickerOffset) * 0.08f)
        val alpha = (
            particle.baseAlpha +
                sin((elapsedSeconds * 0.65f) + particle.flickerOffset) * 0.10f
            ).coerceIn(0.05f, 0.65f) * alphaMultiplier

        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(
                    primary.copy(alpha = alpha),
                    warmAccent.copy(alpha = alpha * 0.28f),
                    Color.Transparent
                ),
                center = Offset(px, py),
                radius = radius
            ),
            radius = radius,
            center = Offset(px, py)
        )
    }
}

private fun buildAnimatedHeroVertices(
    points: List<HeroFacetPoint>,
    center: Offset,
    radius: Float,
    elapsedSeconds: Float
): List<Offset> {
    return points.map { point ->
        val angleDeg = point.baseAngleDeg + sin((elapsedSeconds * 0.11f) + point.wobblePhaseA) * 2.8f
        val angle = angleDeg * DEG_TO_RAD
        val radialScale = point.radiusScale +
            sin((elapsedSeconds * 0.29f) + point.wobblePhaseA) * point.wobbleScale * 0.08f +
            cos((elapsedSeconds * 0.18f) + point.wobblePhaseB) * point.wobbleScale * 0.06f
        val x = center.x + cos(angle) * radius * radialScale
        val y = center.y + sin(angle) * radius * radialScale * 1.07f
        Offset(x, y)
    }
}

private fun buildHeroFacetPoints(
    flavor: CitrosFlavor,
    count: Int
): List<HeroFacetPoint> {
    val random = Random(14_137 + (flavor.ordinal * 47) + count * 19)
    return List(count) { index ->
        HeroFacetPoint(
            baseAngleDeg = (index.toFloat() / count.toFloat()) * 360f + random.nextFloat() * 4f,
            radiusScale = 0.84f + random.nextFloat() * 0.28f,
            wobbleScale = 0.45f + random.nextFloat() * 0.95f,
            wobblePhaseA = random.nextFloat() * TWO_PI,
            wobblePhaseB = random.nextFloat() * TWO_PI
        )
    }
}

private fun buildHeroDustParticles(
    flavor: CitrosFlavor,
    count: Int
): List<HeroDustParticle> {
    val random = Random(91_337 + flavor.ordinal * 101 + count * 13)
    return List(count) {
        HeroDustParticle(
            baseAngleDeg = random.nextFloat() * 360f,
            orbitScale = 1.48f + random.nextFloat() * 1.34f,
            sizeScale = 0.006f + random.nextFloat() * 0.012f,
            speedScale = 0.72f + random.nextFloat() * 0.52f,
            verticalScale = 0.72f + random.nextFloat() * 0.24f,
            baseAlpha = 0.06f + random.nextFloat() * 0.16f,
            alphaSwing = 0.04f + random.nextFloat() * 0.09f,
            flickerOffset = random.nextFloat() * TWO_PI
        )
    }
}

private fun buildHeroBokehParticles(
    flavor: CitrosFlavor,
    count: Int,
    layerSeed: Int
): List<HeroBokehParticle> {
    val random = Random((flavor.ordinal * 173) + (count * 79) + layerSeed)
    return List(count) {
        HeroBokehParticle(
            baseAngleDeg = random.nextFloat() * 360f,
            orbitScale = 0.55f + random.nextFloat() * 2.45f,
            radiusScale = 0.25f + random.nextFloat() * 0.85f,
            speedScale = 0.005f + random.nextFloat() * 0.016f,
            baseAlpha = 0.16f + random.nextFloat() * 0.26f,
            flickerOffset = random.nextFloat() * TWO_PI
        )
    }
}

private fun buildFloatingBackdropSprites(
    flavor: CitrosFlavor,
    count: Int
): List<FloatingBackdropSprite> {
    val random = Random(24_041 + flavor.ordinal * 89 + count * 17)
    return List(count) {
        FloatingBackdropSprite(
            baseX = random.nextFloat(),
            baseY = random.nextFloat(),
            sizeScale = 0.12f + random.nextFloat() * 0.92f,
            speedX = 0.0026f + random.nextFloat() * 0.0104f,
            speedY = 0.24f + random.nextFloat() * 0.54f,
            alpha = 0.22f + random.nextFloat() * 0.40f,
            phaseOffset = random.nextFloat() * TWO_PI
        )
    }
}

private const val TWO_PI = (Math.PI * 2.0).toFloat()
private const val DEG_TO_RAD = (Math.PI / 180.0).toFloat()
private const val RAD_TO_DEG = (180.0 / Math.PI).toFloat()

@Composable
internal fun CitrusPrimaryButton(
    text: String,
    onClick: () -> Unit,
    enabled: Boolean = true,
    modifier: Modifier = Modifier,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE
) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        shape = RoundedCornerShape(999.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = flavor.primary,
            contentColor = flavor.tint,
            disabledContainerColor = flavor.primary.copy(alpha = 0.35f),
            disabledContentColor = flavor.tint.copy(alpha = 0.6f)
        )
    ) {
        Text(text)
    }
}

@Composable
internal fun CitrusLiquidGlassButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tintColor: Color? = null,
    textColor: Color? = null
) {
    val shape = RoundedCornerShape(999.dp)
    val visuals = LocalCitrosSplashVisualTokens.current.glassButton
    val tintedAmber = tintColor?.let { lerp(visuals.amber, it, 0.82f) } ?: visuals.amber
    val tintedWarm = tintColor?.let { lerp(visuals.warm, it, 0.88f) } ?: visuals.warm
    val tintedDeep = tintColor?.let { lerp(visuals.deep, it, 0.42f) } ?: visuals.deep
    val isDarkTheme = LocalCitrosIsDark.current
    val glossColor = if (isDarkTheme) Color.White else MaterialTheme.colorScheme.onSurface
    val backingColor = if (isDarkTheme) {
        Color(0x22000000)
    } else {
        MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.58f)
    }
    val resolvedTextColor = textColor
        ?: tintColor?.let {
            val lift = if (enabled) 0.50f else 0.34f
            lerp(it, MaterialTheme.colorScheme.onSurface, lift)
        }
        ?: if (enabled) visuals.textEnabled else visuals.textDisabled

    Box(
        modifier = modifier
            .clip(shape)
            .background(backingColor)
            .drawBehind {
                val width = size.width
                val height = size.height
                val radius = CornerRadius(height / 2f, height / 2f)

                drawRoundRect(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            glossColor.copy(alpha = if (enabled) 0.20f else 0.08f),
                            tintedAmber.copy(alpha = if (enabled) 0.20f else 0.10f),
                            tintedDeep.copy(alpha = if (enabled) {
                                if (isDarkTheme) 0.42f else 0.28f
                            } else {
                                if (isDarkTheme) 0.24f else 0.16f
                            })
                        )
                    ),
                    cornerRadius = radius
                )
                drawRoundRect(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            tintedWarm.copy(alpha = if (enabled) {
                                if (isDarkTheme) 0.42f else 0.22f
                            } else {
                                if (isDarkTheme) 0.18f else 0.10f
                            }),
                            Color.Transparent
                        ),
                        center = Offset(width * 0.5f, height * 0.05f),
                        radius = width * 0.85f
                    ),
                    cornerRadius = radius
                )
                drawRoundRect(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            glossColor.copy(alpha = if (enabled) {
                                if (isDarkTheme) 0.18f else 0.10f
                            } else {
                                if (isDarkTheme) 0.08f else 0.05f
                            }),
                            Color.Transparent
                        ),
                        startY = 0f,
                        endY = height * 0.62f
                    ),
                    cornerRadius = radius
                )
            }
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 14.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.titleLarge.copy(
                fontSize = MaterialTheme.typography.titleLarge.fontSize * 1.25f
            ),
            fontWeight = FontWeight.SemiBold,
            color = resolvedTextColor
        )
    }
}

@Composable
internal fun CitrosLiquidGlassSurface(
    modifier: Modifier = Modifier,
    shape: Shape = RoundedCornerShape(16.dp),
    onClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    baseColor: Color = Color.Unspecified,
    borderColor: Color = Color.Unspecified,
    borderWidth: Dp = 1.dp,
    highlightColor: Color? = null,
    warmth: Float = 1f,
    contentPadding: PaddingValues = PaddingValues(0.dp),
    content: @Composable BoxScope.() -> Unit
) {
    val visuals = LocalCitrosSplashVisualTokens.current.glassButton
    val isDarkTheme = LocalCitrosIsDark.current
    val warmScale = warmth.coerceIn(0.45f, 1.8f)
    val amber = highlightColor?.let { lerp(visuals.amber, it, 0.45f) } ?: visuals.amber
    val warm = highlightColor ?: visuals.warm
    val glossColor = if (isDarkTheme) Color.White else MaterialTheme.colorScheme.onSurface
    val resolvedBaseColor = if (baseColor == Color.Unspecified) {
        if (isDarkTheme) {
            Color(0x14000000)
        } else {
            MaterialTheme.colorScheme.surface.copy(alpha = 0.78f)
        }
    } else {
        baseColor
    }
    val resolvedBorderColor = if (borderColor == Color.Unspecified) {
        if (isDarkTheme) {
            Color.White.copy(alpha = 0.14f)
        } else {
            MaterialTheme.colorScheme.outline.copy(alpha = 0.28f)
        }
    } else {
        borderColor
    }
    val interactionModifier = if (onClick != null) {
        Modifier.clickable(enabled = enabled, onClick = onClick)
    } else {
        Modifier
    }

    Box(
        modifier = modifier
            .then(interactionModifier)
            .clip(shape)
            .background(resolvedBaseColor, shape)
            .let { base ->
                if (borderWidth > 0.dp) {
                    base.border(BorderStroke(borderWidth, resolvedBorderColor), shape)
                } else {
                    base
                }
            }
            .drawBehind {
                val width = size.width
                val height = size.height

                drawRect(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            glossColor.copy(alpha = 0.10f * warmScale),
                            amber.copy(alpha = 0.10f * warmScale),
                            visuals.deep.copy(alpha = if (isDarkTheme) 0.36f else 0.18f)
                        )
                    )
                )
                drawRect(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            warm.copy(alpha = (if (isDarkTheme) 0.22f else 0.14f) * warmScale),
                            Color.Transparent
                        ),
                        center = Offset(width * 0.5f, height * 0.05f),
                        radius = width * 0.95f
                    )
                )
                drawRect(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            glossColor.copy(alpha = if (isDarkTheme) 0.12f else 0.08f),
                            Color.Transparent
                        ),
                        startY = 0f,
                        endY = height * 0.64f
                    )
                )
            }
    ) {
        Box(modifier = Modifier.padding(contentPadding), content = content)
    }
}

@Composable
internal fun CitrusSecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true
) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        shape = RoundedCornerShape(999.dp)
    ) {
        Text(text)
    }
}

@Composable
internal fun FlavorOptionCard(
    flavor: CitrosFlavor,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    CitrusLiquidGlassButton(
        text = if (selected) "✓ ${flavor.displayName}" else flavor.displayName,
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        tintColor = flavor.primary
    )
}

@Composable
internal fun PersonalityOptionChip(
    text: String,
    selected: Boolean,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    scale: Float = 1f,
    onClick: () -> Unit
) {
    val clampedScale = scale.coerceIn(1f, 1.22f)
    val shape = RoundedCornerShape(999.dp)
    val borderColor = if (selected) {
        flavor.primary.copy(alpha = 0.86f)
    } else {
        flavor.primary.copy(alpha = 0.34f)
    }
    val textColor = if (selected) {
        lerp(flavor.primary, MaterialTheme.colorScheme.onSurface, 0.44f)
    } else {
        flavor.primary.copy(alpha = 0.86f)
    }

    CitrosLiquidGlassSurface(
        modifier = Modifier.widthIn(max = 260.dp),
        shape = shape,
        onClick = onClick,
        borderColor = borderColor,
        borderWidth = if (selected) 2.dp else 1.dp,
        highlightColor = if (selected) flavor.primary else null,
        warmth = if (selected) 1.10f else 0.78f,
        contentPadding = PaddingValues(
            horizontal = 12.dp * clampedScale,
            vertical = 8.dp * clampedScale
        )
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelLarge.copy(
                fontSize = MaterialTheme.typography.labelLarge.fontSize * clampedScale
            ),
            color = textColor,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis
        )
    }
}

@Composable
internal fun PlanCard(
    plan: CitrosPlanSpec,
    onSelect: () -> Unit,
    modifier: Modifier = Modifier,
    testTag: String? = null
) {
    val shape = RoundedCornerShape(18.dp)
    val borderColor = MaterialTheme.colorScheme.outline
    val stroke = 1.dp
    val cardModifier = (if (testTag != null) modifier.testTag(testTag) else modifier)
        .fillMaxWidth()
    val accent = if (plan.recommended) plan.accent else null

    CitrosLiquidGlassSurface(
        modifier = cardModifier,
        shape = shape,
        onClick = onSelect,
        borderColor = borderColor.copy(alpha = 0.35f),
        borderWidth = stroke,
        highlightColor = accent,
        warmth = if (plan.recommended) 1.22f else 0.84f,
        contentPadding = PaddingValues(16.dp)
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = plan.title,
                    style = MaterialTheme.typography.titleMedium,
                    color = plan.accent.copy(alpha = 0.96f),
                    modifier = Modifier.weight(1f)
                )
                if (plan.recommended) {
                    Text(
                        text = "Recommended",
                        style = MaterialTheme.typography.labelSmall,
                        color = plan.accent
                    )
                }
            }
            Text(
                text = plan.subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.88f)
            )
            Text(
                text = plan.details,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
            )

            if (plan.comingSoon) {
                Text(
                    text = "Coming Soon",
                    style = MaterialTheme.typography.labelMedium,
                    color = plan.accent
                )
            }

            CitrusLiquidGlassButton(
                text = plan.cta,
                onClick = onSelect,
                modifier = Modifier.fillMaxWidth(),
                tintColor = plan.accent
            )
        }
    }
}
