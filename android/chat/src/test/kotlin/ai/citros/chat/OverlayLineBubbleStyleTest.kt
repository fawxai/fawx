package ai.citros.chat

import ai.citros.core.OverlayLineType
import org.junit.Test
import kotlin.test.assertEquals

class OverlayLineBubbleStyleTest {

    @Test
    fun `user style uses flavor token bubble colors and end anchor`() {
        val flavor = CitrosFlavor.LIME
        val surfaces = citrosDirectiveSurfaces(isDarkTheme = false)
        val flavorTokens = citrosDirectiveFlavorTokens(flavor, surfaces)

        val style = overlayLineBubbleStyle(
            type = OverlayLineType.USER,
            flavor = flavor,
            surfaces = surfaces,
            flavorTokens = flavorTokens
        )

        assertEquals(flavorTokens.userBubbleBackground, style.bubbleColor)
        assertEquals(surfaces.separatorLight, style.bubbleBorder)
        assertEquals(flavorTokens.userBubbleText, style.bubbleTextColor)
        assertEquals(OverlayBubbleHorizontalAnchor.END, style.horizontalAnchor)
    }

    @Test
    fun `system style uses directive surface colors and start anchor`() {
        val flavor = CitrosFlavor.LIME
        val surfaces = citrosDirectiveSurfaces(isDarkTheme = false)
        val flavorTokens = citrosDirectiveFlavorTokens(flavor, surfaces)

        val style = overlayLineBubbleStyle(
            type = OverlayLineType.SYSTEM,
            flavor = flavor,
            surfaces = surfaces,
            flavorTokens = flavorTokens
        )

        assertEquals(surfaces.surface2, style.bubbleColor)
        assertEquals(surfaces.separatorLight, style.bubbleBorder)
        assertEquals(surfaces.labelPrimary, style.bubbleTextColor)
        assertEquals(OverlayBubbleHorizontalAnchor.START, style.horizontalAnchor)
    }

    @Test
    fun `queued style uses primary tint variants and start anchor`() {
        val flavor = CitrosFlavor.LIME
        val surfaces = citrosDirectiveSurfaces(isDarkTheme = false)
        val flavorTokens = citrosDirectiveFlavorTokens(flavor, surfaces)

        val style = overlayLineBubbleStyle(
            type = OverlayLineType.QUEUED,
            flavor = flavor,
            surfaces = surfaces,
            flavorTokens = flavorTokens
        )

        assertEquals(flavor.primary.copy(alpha = 0.14f), style.bubbleColor)
        assertEquals(flavor.primary.copy(alpha = 0.34f), style.bubbleBorder)
        assertEquals(surfaces.labelSecondary, style.bubbleTextColor)
        assertEquals(OverlayBubbleHorizontalAnchor.START, style.horizontalAnchor)
    }
}
