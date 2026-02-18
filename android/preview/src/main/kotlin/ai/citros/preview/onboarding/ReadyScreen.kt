package ai.citros.preview.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ai.citros.preview.components.CitrusHeroBadge
import ai.citros.preview.components.CitrusPrimaryButton
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.extendedColors
import ai.citros.preview.theme.CitrosFlavor

/**
 * Ready/completion screen - final step of onboarding.
 *
 * Celebrates successful onboarding and lists key capabilities.
 * Primary CTA is "Start Chatting" to enter the main app experience.
 */
@Composable
fun ReadyScreen(
    flavor: CitrosFlavor,
    onStartChatting: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = CitrosDimensions.spacingXl),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            // Hero badge - celebration size
            CitrusHeroBadge(
                flavor = flavor,
                size = 80,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Headline
            Text(
                text = "You're all set!",
                style = MaterialTheme.typography.displaySmall,
                fontSize = 28.sp,
                fontWeight = FontWeight.SemiBold,
                color = Color.White,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            // Subheadline
            Text(
                text = "Your AI phone agent is ready to go",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.extendedColors.textDim,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXxl))

            // Capabilities list
            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                    CitrosDimensions.spacingMd,
                ),
            ) {
                val capabilities = listOf(
                    "Use your phone for you",
                    "Remember your context",
                    "Search the web",
                    "Learn your preferences",
                )

                capabilities.forEach { capability ->
                    CapabilityItem(
                        text = capability,
                        flavor = flavor,
                    )
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXxl))

            // Start Chatting button
            CitrusPrimaryButton(
                text = "Start Chatting",
                onClick = onStartChatting,
                flavor = flavor,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/**
 * Individual capability item with checkmark icon.
 */
@Composable
private fun CapabilityItem(
    text: String,
    flavor: CitrosFlavor,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Checkmark circle
        Box(
            modifier = Modifier
                .size(24.dp)
                .background(
                    color = flavor.primary.copy(alpha = 0.15f),
                    shape = CircleShape,
                ),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Default.CheckCircle,
                contentDescription = "Capability",
                tint = flavor.primary,
                modifier = Modifier.size(16.dp),
            )
        }

        Spacer(modifier = Modifier.width(CitrosDimensions.spacingMd))

        // Capability text
        Text(
            text = text,
            style = MaterialTheme.typography.bodyMedium,
            color = Color.White,
            fontWeight = FontWeight.Medium,
        )
    }
}
