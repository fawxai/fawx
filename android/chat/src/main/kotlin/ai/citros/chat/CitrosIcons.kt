package ai.citros.chat

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

internal object CitrosIcons {
    val ArrowBack: ImageVector by lazy {
        buildIcon("CitrosArrowBack") {
            path(strokeLineWidth = 2.6f) {
                moveTo(16f, 5f)
                lineTo(8f, 12f)
                lineTo(16f, 19f)
            }
        }
    }

    val Send: ImageVector by lazy {
        buildIcon("CitrosSend") {
            path(strokeLineWidth = 1.8f) {
                moveTo(3f, 12f)
                lineTo(21f, 4f)
                lineTo(14f, 20f)
                lineTo(11.4f, 13.2f)
                close()
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(11.2f, 13f)
                lineTo(21f, 4f)
            }
        }
    }

    val Visibility: ImageVector by lazy {
        buildIcon("CitrosVisibility") {
            path(strokeLineWidth = 1.8f) {
                moveTo(2.5f, 12f)
                curveTo(5.5f, 7f, 8.8f, 5f, 12f, 5f)
                curveTo(15.2f, 5f, 18.5f, 7f, 21.5f, 12f)
                curveTo(18.5f, 17f, 15.2f, 19f, 12f, 19f)
                curveTo(8.8f, 19f, 5.5f, 17f, 2.5f, 12f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(9.5f, 12f)
                curveTo(9.5f, 10.6f, 10.6f, 9.5f, 12f, 9.5f)
                curveTo(13.4f, 9.5f, 14.5f, 10.6f, 14.5f, 12f)
                curveTo(14.5f, 13.4f, 13.4f, 14.5f, 12f, 14.5f)
                curveTo(10.6f, 14.5f, 9.5f, 13.4f, 9.5f, 12f)
            }
        }
    }

    val VisibilityOff: ImageVector by lazy {
        buildIcon("CitrosVisibilityOff") {
            path(strokeLineWidth = 1.8f) {
                moveTo(3f, 3f)
                lineTo(21f, 21f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(2.5f, 12f)
                curveTo(5.5f, 7f, 8.8f, 5f, 12f, 5f)
                curveTo(15.2f, 5f, 18.5f, 7f, 21.5f, 12f)
                curveTo(20.1f, 14.2f, 18.7f, 15.8f, 17.3f, 17f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(9.4f, 9.4f)
                curveTo(10f, 8.8f, 10.9f, 8.5f, 12f, 8.5f)
                curveTo(13.9f, 8.5f, 15.5f, 10.1f, 15.5f, 12f)
                curveTo(15.5f, 13.1f, 15.2f, 14f, 14.6f, 14.6f)
            }
        }
    }

    val Palette: ImageVector by lazy {
        buildIcon("CitrosPalette") {
            path(strokeLineWidth = 1.7f) {
                moveTo(12f, 3.5f)
                curveTo(7f, 3.5f, 3f, 7.2f, 3f, 11.8f)
                curveTo(3f, 16f, 6.4f, 19f, 10f, 19f)
                lineTo(12.8f, 19f)
                curveTo(14f, 19f, 15f, 18f, 15f, 16.8f)
                curveTo(15f, 15.7f, 15.9f, 14.8f, 17f, 14.8f)
                lineTo(17.7f, 14.8f)
                curveTo(20.2f, 14.8f, 21.8f, 13.1f, 21.8f, 10.8f)
                curveTo(21.8f, 6.7f, 17.7f, 3.5f, 12f, 3.5f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(8f, 9f); lineTo(8.01f, 9f)
                moveTo(10.8f, 7.5f); lineTo(10.81f, 7.5f)
                moveTo(13.8f, 8f); lineTo(13.81f, 8f)
                moveTo(16.2f, 10f); lineTo(16.21f, 10f)
            }
        }
    }

    val ChatBubble: ImageVector by lazy {
        buildIcon("CitrosChatBubble") {
            path(strokeLineWidth = 1.8f) {
                moveTo(5f, 6.5f)
                lineTo(19f, 6.5f)
                curveTo(20.1f, 6.5f, 21f, 7.4f, 21f, 8.5f)
                lineTo(21f, 14.5f)
                curveTo(21f, 15.6f, 20.1f, 16.5f, 19f, 16.5f)
                lineTo(11f, 16.5f)
                lineTo(7f, 19.5f)
                lineTo(7f, 16.5f)
                lineTo(5f, 16.5f)
                curveTo(3.9f, 16.5f, 3f, 15.6f, 3f, 14.5f)
                lineTo(3f, 8.5f)
                curveTo(3f, 7.4f, 3.9f, 6.5f, 5f, 6.5f)
            }
        }
    }

    val Person: ImageVector by lazy {
        buildIcon("CitrosPerson") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 12f)
                curveTo(10.1f, 12f, 8.5f, 10.4f, 8.5f, 8.5f)
                curveTo(8.5f, 6.6f, 10.1f, 5f, 12f, 5f)
                curveTo(13.9f, 5f, 15.5f, 6.6f, 15.5f, 8.5f)
                curveTo(15.5f, 10.4f, 13.9f, 12f, 12f, 12f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(5.5f, 19f)
                curveTo(6.2f, 16.2f, 8.7f, 14.5f, 12f, 14.5f)
                curveTo(15.3f, 14.5f, 17.8f, 16.2f, 18.5f, 19f)
            }
        }
    }

    val Shield: ImageVector by lazy {
        buildIcon("CitrosShield") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 3.5f)
                lineTo(19.5f, 6.3f)
                lineTo(19.5f, 11.2f)
                curveTo(19.5f, 15.8f, 16.5f, 19.8f, 12f, 21f)
                curveTo(7.5f, 19.8f, 4.5f, 15.8f, 4.5f, 11.2f)
                lineTo(4.5f, 6.3f)
                close()
            }
        }
    }

    val Phone: ImageVector by lazy {
        buildIcon("CitrosPhone") {
            path(strokeLineWidth = 1.8f) {
                moveTo(9f, 3.5f)
                lineTo(15f, 3.5f)
                curveTo(16.1f, 3.5f, 17f, 4.4f, 17f, 5.5f)
                lineTo(17f, 18.5f)
                curveTo(17f, 19.6f, 16.1f, 20.5f, 15f, 20.5f)
                lineTo(9f, 20.5f)
                curveTo(7.9f, 20.5f, 7f, 19.6f, 7f, 18.5f)
                lineTo(7f, 5.5f)
                curveTo(7f, 4.4f, 7.9f, 3.5f, 9f, 3.5f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(11f, 17.2f); lineTo(13f, 17.2f)
            }
        }
    }

    val Star: ImageVector by lazy {
        buildIcon("CitrosStar") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 3.5f)
                lineTo(14.8f, 9f)
                lineTo(21f, 9.8f)
                lineTo(16.5f, 14f)
                lineTo(17.6f, 20.2f)
                lineTo(12f, 17.3f)
                lineTo(6.4f, 20.2f)
                lineTo(7.5f, 14f)
                lineTo(3f, 9.8f)
                lineTo(9.2f, 9f)
                close()
            }
        }
    }

    val Settings: ImageVector by lazy {
        buildIcon("CitrosSettings") {
            path(strokeLineWidth = 1.7f) {
                moveTo(12f, 8.6f)
                curveTo(10.1f, 8.6f, 8.6f, 10.1f, 8.6f, 12f)
                curveTo(8.6f, 13.9f, 10.1f, 15.4f, 12f, 15.4f)
                curveTo(13.9f, 15.4f, 15.4f, 13.9f, 15.4f, 12f)
                curveTo(15.4f, 10.1f, 13.9f, 8.6f, 12f, 8.6f)
            }
            path(strokeLineWidth = 1.7f) {
                moveTo(12f, 3.2f); lineTo(12f, 5.1f)
                moveTo(12f, 18.9f); lineTo(12f, 20.8f)
                moveTo(3.2f, 12f); lineTo(5.1f, 12f)
                moveTo(18.9f, 12f); lineTo(20.8f, 12f)
                moveTo(5.9f, 5.9f); lineTo(7.2f, 7.2f)
                moveTo(16.8f, 16.8f); lineTo(18.1f, 18.1f)
                moveTo(16.8f, 7.2f); lineTo(18.1f, 5.9f)
                moveTo(5.9f, 18.1f); lineTo(7.2f, 16.8f)
            }
        }
    }

    val Volume: ImageVector by lazy {
        buildIcon("CitrosVolume") {
            path(strokeLineWidth = 1.8f) {
                moveTo(5f, 10f)
                lineTo(8.2f, 10f)
                lineTo(12f, 6f)
                lineTo(12f, 18f)
                lineTo(8.2f, 14f)
                lineTo(5f, 14f)
                close()
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(15.5f, 9f)
                curveTo(16.8f, 10.1f, 17.5f, 11f, 17.5f, 12f)
                curveTo(17.5f, 13f, 16.8f, 13.9f, 15.5f, 15f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(18.3f, 7f)
                curveTo(20.4f, 8.7f, 21.5f, 10.2f, 21.5f, 12f)
                curveTo(21.5f, 13.8f, 20.4f, 15.3f, 18.3f, 17f)
            }
        }
    }

    val Brush: ImageVector by lazy {
        buildIcon("CitrosBrush") {
            path(strokeLineWidth = 1.8f) {
                moveTo(14.5f, 4f)
                lineTo(20f, 9.5f)
                lineTo(11f, 18.5f)
                lineTo(5.5f, 13f)
                close()
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(4.5f, 17f)
                curveTo(4.5f, 19f, 3.5f, 20.5f, 2f, 21f)
                curveTo(4.8f, 21.2f, 7f, 20f, 7f, 17f)
            }
        }
    }

    val Info: ImageVector by lazy {
        buildIcon("CitrosInfo") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 10.2f); lineTo(12f, 17f)
                moveTo(12f, 7f); lineTo(12.01f, 7f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 3f)
                curveTo(7f, 3f, 3f, 7f, 3f, 12f)
                curveTo(3f, 17f, 7f, 21f, 12f, 21f)
                curveTo(17f, 21f, 21f, 17f, 21f, 12f)
                curveTo(21f, 7f, 17f, 3f, 12f, 3f)
            }
        }
    }

    val Key: ImageVector by lazy {
        buildIcon("CitrosKey") {
            path(strokeLineWidth = 1.8f) {
                moveTo(14.5f, 10.5f)
                curveTo(14.5f, 8.3f, 12.7f, 6.5f, 10.5f, 6.5f)
                curveTo(8.3f, 6.5f, 6.5f, 8.3f, 6.5f, 10.5f)
                curveTo(6.5f, 12.7f, 8.3f, 14.5f, 10.5f, 14.5f)
                curveTo(12.7f, 14.5f, 14.5f, 12.7f, 14.5f, 10.5f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(13.5f, 11.5f)
                lineTo(21f, 19f)
                moveTo(17.5f, 15.5f)
                lineTo(16f, 17f)
                moveTo(19.5f, 17.5f)
                lineTo(18f, 19f)
            }
        }
    }

    val Security: ImageVector by lazy { Shield }

    val Tune: ImageVector by lazy {
        buildIcon("CitrosTune") {
            path(strokeLineWidth = 1.8f) {
                moveTo(4f, 7f); lineTo(20f, 7f)
                moveTo(4f, 12f); lineTo(20f, 12f)
                moveTo(4f, 17f); lineTo(20f, 17f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(8f, 5f); lineTo(8f, 9f)
                moveTo(14f, 10f); lineTo(14f, 14f)
                moveTo(11f, 15f); lineTo(11f, 19f)
            }
        }
    }

    val ExitToApp: ImageVector by lazy {
        buildIcon("CitrosExitToApp") {
            path(strokeLineWidth = 1.8f) {
                moveTo(10f, 5f)
                lineTo(6f, 5f)
                curveTo(4.9f, 5f, 4f, 5.9f, 4f, 7f)
                lineTo(4f, 17f)
                curveTo(4f, 18.1f, 4.9f, 19f, 6f, 19f)
                lineTo(10f, 19f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(13f, 8f); lineTo(20f, 12f); lineTo(13f, 16f)
                moveTo(20f, 12f); lineTo(9f, 12f)
            }
        }
    }

    val ArrowUp: ImageVector by lazy {
        buildIcon("CitrosArrowUp") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 4f)
                lineTo(18f, 10f)
                moveTo(12f, 4f)
                lineTo(6f, 10f)
                moveTo(12f, 4f)
                lineTo(12f, 20f)
            }
        }
    }

    val Mic: ImageVector by lazy {
        buildIcon("CitrosMic") {
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 4f)
                curveTo(10.3f, 4f, 9f, 5.3f, 9f, 7f)
                lineTo(9f, 11f)
                curveTo(9f, 12.7f, 10.3f, 14f, 12f, 14f)
                curveTo(13.7f, 14f, 15f, 12.7f, 15f, 11f)
                lineTo(15f, 7f)
                curveTo(15f, 5.3f, 13.7f, 4f, 12f, 4f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(7f, 11f)
                curveTo(7f, 13.8f, 9.2f, 16f, 12f, 16f)
                curveTo(14.8f, 16f, 17f, 13.8f, 17f, 11f)
            }
            path(strokeLineWidth = 1.8f) {
                moveTo(12f, 16f); lineTo(12f, 20f)
                moveTo(9.5f, 20f); lineTo(14.5f, 20f)
            }
        }
    }

    val SearchBarMic: ImageVector by lazy {
        buildIcon("CitrosSearchBarMic", viewport = 18f) {
            path(strokeLineWidth = 1.5f) {
                moveTo(6.5f, 4.2f)
                curveTo(6.5f, 2.98f, 7.48f, 2f, 8.7f, 2f)
                lineTo(9.3f, 2f)
                curveTo(10.52f, 2f, 11.5f, 2.98f, 11.5f, 4.2f)
                lineTo(11.5f, 8.8f)
                curveTo(11.5f, 10.02f, 10.52f, 11f, 9.3f, 11f)
                lineTo(8.7f, 11f)
                curveTo(7.48f, 11f, 6.5f, 10.02f, 6.5f, 8.8f)
                close()
            }
            path(strokeLineWidth = 1.5f) {
                moveTo(4f, 9.2f)
                curveTo(4f, 12f, 6.2f, 14.2f, 9f, 14.2f)
                curveTo(11.8f, 14.2f, 14f, 12f, 14f, 9.2f)
            }
            path(strokeLineWidth = 1.5f) {
                moveTo(9f, 14.2f)
                lineTo(9f, 16f)
                moveTo(7.4f, 16f)
                lineTo(10.6f, 16f)
            }
        }
    }

    val SearchBarCheck: ImageVector by lazy {
        buildIcon("CitrosSearchBarCheck", viewport = 10f) {
            path(strokeLineWidth = 1.4f) {
                moveTo(2f, 5.5f)
                lineTo(4.2f, 7.5f)
                lineTo(8f, 3f)
            }
        }
    }

    val Stop: ImageVector by lazy {
        buildIcon("CitrosStop") {
            path(strokeLineWidth = 1.8f) {
                moveTo(7f, 7f)
                lineTo(17f, 7f)
                lineTo(17f, 17f)
                lineTo(7f, 17f)
                close()
            }
        }
    }
}

private fun buildIcon(
    name: String,
    viewport: Float = 24f,
    block: ImageVector.Builder.() -> Unit
): ImageVector {
    return ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = viewport,
        viewportHeight = viewport
    ).apply(block).build()
}

private fun ImageVector.Builder.path(
    strokeLineWidth: Float,
    block: androidx.compose.ui.graphics.vector.PathBuilder.() -> Unit
) {
    path(
        fill = SolidColor(Color.Transparent),
        stroke = SolidColor(Color.Black),
        strokeLineWidth = strokeLineWidth,
        strokeLineCap = StrokeCap.Round,
        strokeLineJoin = StrokeJoin.Round,
        pathFillType = PathFillType.NonZero,
        pathBuilder = block
    )
}
