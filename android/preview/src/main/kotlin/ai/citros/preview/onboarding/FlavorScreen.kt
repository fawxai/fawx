package ai.citros.preview.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import ai.citros.preview.components.CitrosStepHeader
import ai.citros.preview.components.CitrusPrimaryButton
import ai.citros.preview.components.FlavorOptionCard
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.extendedColors
import ai.citros.preview.theme.CitrosFlavor
import androidx.compose.ui.unit.sp

/**
 * Flavor selection screen - step 1 of onboarding.
 *
 * Allows users to select their preferred color theme. Each CitrosFlavor
 * is presented as a selectable card. Selection is required to continue.
 */
@Composable
fun FlavorScreen(
    selectedFlavor: CitrosFlavor,
    onFlavorSelected: (CitrosFlavor) -> Unit,
    onContinue: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = CitrosDimensions.spacingLg),
        ) {
            // Step header
            CitrosStepHeader(
                title = "Choose Your Flavor",
                stepIndex = 1,
                totalSteps = 7,
                onBack = onBack,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            // Subtitle
            Text(
                text = "Pick a color theme for your Citros experience",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.extendedColors.textDim,
                modifier = Modifier.padding(horizontal = CitrosDimensions.spacingMd),
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Flavor options
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f),
            ) {
                items(
                    items = CitrosFlavor.entries,
                    key = { flavor -> flavor.storageValue },
                ) { flavor ->
                    FlavorOptionCard(
                        flavor = flavor,
                        selected = selectedFlavor == flavor,
                        onClick = { onFlavorSelected(flavor) },
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = CitrosDimensions.spacingMd),
                    )
                    Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Continue button
            CitrusPrimaryButton(
                text = "Continue",
                onClick = onContinue,
                flavor = selectedFlavor,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = CitrosDimensions.spacingMd),
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXl))
        }
    }
}
