package ai.citros.preview.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ai.citros.preview.components.CitrosHeroSphere
import ai.citros.preview.components.CitrusPrimaryButton
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.CitrosTheme
import ai.citros.preview.theme.extendedColors
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import ai.citros.preview.theme.CitrosFlavor

/**
 * Welcome screen - the first onboarding screen introducing Citros.
 *
 * Displays the hero sphere, app title, subtitle, page indicators, and
 * a "Get Started" button to proceed to flavor selection.
 */
@Composable
fun WelcomeScreen(
    flavor: CitrosFlavor,
    onGetStarted: () -> Unit,
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
            // Hero sphere
            CitrosHeroSphere(
                flavor = flavor,
                size = 220.dp,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXl))

            // Title
            Text(
                text = "Citros",
                style = MaterialTheme.typography.displayLarge,
                fontSize = 48.sp,
                fontWeight = FontWeight.Bold,
                color = Color.White,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            // Subtitle
            Text(
                text = "AI that uses your phone",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.extendedColors.textDim,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXxl))

            // Page indicator dots
            Row(
                modifier = Modifier.align(Alignment.CenterHorizontally),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                repeat(3) { index ->
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .background(
                                color = if (index == 0) {
                                    flavor.primary
                                } else {
                                    MaterialTheme.extendedColors.border
                                },
                                shape = CircleShape,
                            ),
                    )
                    if (index < 2) {
                        Spacer(modifier = Modifier.width(8.dp))
                    }
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXl))

            // Get Started button
            CitrusPrimaryButton(
                text = "Get Started",
                onClick = onGetStarted,
                flavor = flavor,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
