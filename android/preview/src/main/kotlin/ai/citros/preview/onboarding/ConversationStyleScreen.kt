package ai.citros.preview.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import ai.citros.preview.components.CitrosStepHeader
import ai.citros.preview.components.CitrusPrimaryButton
import ai.citros.preview.components.PersonalityOptionChip
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.extendedColors
import ai.citros.preview.theme.CitrosFlavor
import androidx.compose.ui.unit.sp

/**
 * Conversation style selection screen - step 2 of onboarding.
 *
 * Users customize how Citros communicates through three categories:
 * tone (Casual, Professional, Playful), detail level (Brief, Balanced, Detailed),
 * and autonomy (Ask before everything, Ask for risky stuff, Full autonomy).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ConversationStyleScreen(
    flavor: CitrosFlavor,
    selectedTone: String?,
    selectedDetail: String?,
    selectedAutonomy: String?,
    onToneSelected: (String) -> Unit,
    onDetailSelected: (String) -> Unit,
    onAutonomySelected: (String) -> Unit,
    onContinue: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val isAllSelected = selectedTone != null && selectedDetail != null && selectedAutonomy != null

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = CitrosDimensions.spacingLg)
                .verticalScroll(rememberScrollState()),
        ) {
            // Step header
            CitrosStepHeader(
                title = "Conversation Style",
                stepIndex = 2,
                totalSteps = 7,
                onBack = onBack,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            // Subtitle
            Text(
                text = "Customize how your Citros assistant communicates",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.extendedColors.textDim,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Tone section
            Text(
                text = "How should I talk to you?",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                    CitrosDimensions.spacingMd,
                ),
            ) {
                val toneOptions = listOf("Casual", "Professional", "Playful")
                toneOptions.forEach { option ->
                    PersonalityOptionChip(
                        text = option,
                        selected = selectedTone == option,
                        onClick = { onToneSelected(option) },
                        flavor = flavor,
                    )
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Detail section
            Text(
                text = "How much should I explain?",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                    CitrosDimensions.spacingMd,
                ),
            ) {
                val detailOptions = listOf("Brief", "Balanced", "Detailed")
                detailOptions.forEach { option ->
                    PersonalityOptionChip(
                        text = option,
                        selected = selectedDetail == option,
                        onClick = { onDetailSelected(option) },
                        flavor = flavor,
                    )
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingLg))

            // Autonomy section
            Text(
                text = "Comfort level",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingSm))

            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                    CitrosDimensions.spacingMd,
                ),
            ) {
                val autonomyOptions = listOf(
                    "Ask before everything",
                    "Ask for risky stuff",
                    "Full autonomy",
                )
                autonomyOptions.forEach { option ->
                    PersonalityOptionChip(
                        text = option,
                        selected = selectedAutonomy == option,
                        onClick = { onAutonomySelected(option) },
                        flavor = flavor,
                    )
                }
            }

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXxl))

            // Continue button - enabled only when all selections made
            CitrusPrimaryButton(
                text = "Continue",
                onClick = onContinue,
                flavor = flavor,
                enabled = isAllSelected,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(modifier = Modifier.height(CitrosDimensions.spacingXl))
        }
    }
}
