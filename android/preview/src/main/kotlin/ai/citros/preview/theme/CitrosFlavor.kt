package ai.citros.preview.theme

import androidx.compose.ui.graphics.Color

/**
 * Flavor enum defining the color theme system for Citros.
 *
 * Each flavor includes:
 * - storageValue: String identifier for persistence
 * - displayName: User-friendly name for UI
 * - primary: Primary accent color for the flavor
 * - glow: Glowing/highlight variant of the primary color
 * - tint: Darker tint variant for shadows and text tinting
 */
enum class CitrosFlavor(
    val storageValue: String,
    val displayName: String,
    val primary: Color,
    val glow: Color,
    val tint: Color,
) {
    LEMON(
        storageValue = "lemon",
        displayName = "Lemon",
        primary = Color(0xFFFFD600),
        glow = Color(0xFFFFF9C4),
        tint = Color(0xFF332B00),
    ),

    TANGERINE(
        storageValue = "tangerine",
        displayName = "Tangerine",
        primary = Color(0xFFFF8C00),
        glow = Color(0xFFFFE0B2),
        tint = Color(0xFF331C00),
    ),

    LIME(
        storageValue = "lime",
        displayName = "Lime",
        primary = Color(0xFF7CB342),
        glow = Color(0xFFDCEDC8),
        tint = Color(0xFF1A2E0D),
    ),

    BLOOD_ORANGE(
        storageValue = "blood_orange",
        displayName = "Blood Orange",
        primary = Color(0xFFD84315),
        glow = Color(0xFFFFCCBC),
        tint = Color(0xFF2E0D04),
    ),

    GRAPEFRUIT(
        storageValue = "grapefruit",
        displayName = "Grapefruit",
        primary = Color(0xFFE91E63),
        glow = Color(0xFFF8BBD0),
        tint = Color(0xFF2E0413),
    );

    companion object
}

/**
 * Extension to find a flavor by storage value.
 *
 * @param storageValue The storage identifier
 * @param default The default flavor to return if not found
 * @return The matching flavor or default
 */
fun CitrosFlavor.Companion.fromStorageValue(
    storageValue: String,
    default: CitrosFlavor = CitrosFlavor.LEMON,
): CitrosFlavor {
    return CitrosFlavor.entries.firstOrNull { it.storageValue == storageValue } ?: default
}
