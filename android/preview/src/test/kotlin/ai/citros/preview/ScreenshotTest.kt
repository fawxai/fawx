package ai.citros.preview

import ai.citros.preview.components.CitrosHeroSphere
import ai.citros.preview.components.CitrusPrimaryButton
import ai.citros.preview.components.CitrusHeroBadge
import ai.citros.preview.components.FlavorOptionCard
import ai.citros.preview.components.PersonalityOptionChip
import ai.citros.preview.onboarding.WelcomeScreen
import ai.citros.preview.onboarding.FlavorScreen
import ai.citros.preview.onboarding.ConversationStyleScreen
import ai.citros.preview.onboarding.ReadyScreen
import ai.citros.preview.theme.CitrosFlavor
import ai.citros.preview.theme.CitrosTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import org.junit.Rule
import org.junit.Test

class ScreenshotTest {

    @get:Rule
    val paparazzi = Paparazzi(
        deviceConfig = DeviceConfig.PIXEL_6.copy(
            softButtons = false,
        ),
        theme = "android:Theme.Material.NoActionBar.Fullscreen",
    )

    @Test
    fun welcomeScreen() {
        paparazzi.snapshot {
            CitrosTheme {
                WelcomeScreen(
                    flavor = CitrosFlavor.TANGERINE,
                    onGetStarted = {},
                )
            }
        }
    }

    @Test
    fun flavorScreen() {
        paparazzi.snapshot {
            CitrosTheme {
                FlavorScreen(
                    selectedFlavor = CitrosFlavor.TANGERINE,
                    onFlavorSelected = {},
                    onContinue = {},
                    onBack = {},
                )
            }
        }
    }

    @Test
    fun conversationStyleScreen() {
        paparazzi.snapshot {
            CitrosTheme {
                ConversationStyleScreen(
                    flavor = CitrosFlavor.TANGERINE,
                    selectedTone = "Friendly",
                    selectedDetail = null,
                    selectedAutonomy = null,
                    onToneSelected = {},
                    onDetailSelected = {},
                    onAutonomySelected = {},
                    onContinue = {},
                    onBack = {},
                )
            }
        }
    }

    @Test
    fun readyScreen() {
        paparazzi.snapshot {
            CitrosTheme {
                ReadyScreen(
                    flavor = CitrosFlavor.TANGERINE,
                    onStartChatting = {},
                )
            }
        }
    }

    @Test
    fun heroSphere_tangerine() {
        paparazzi.snapshot {
            CitrosTheme {
                Surface(color = MaterialTheme.colorScheme.background) {
                    Column(
                        modifier = Modifier.padding(32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        CitrosHeroSphere(
                            flavor = CitrosFlavor.TANGERINE,
                            size = 200.dp,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun heroSphere_allFlavors() {
        paparazzi.snapshot {
            CitrosTheme {
                Surface(color = MaterialTheme.colorScheme.background) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(16.dp),
                    ) {
                        CitrosFlavor.entries.forEach { flavor ->
                            CitrosHeroSphere(
                                flavor = flavor,
                                size = 100.dp,
                            )
                        }
                    }
                }
            }
        }
    }

    @Test
    fun components_gallery() {
        paparazzi.snapshot {
            CitrosTheme {
                Surface(color = MaterialTheme.colorScheme.background) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(16.dp),
                    ) {
                        CitrusPrimaryButton(
                            text = "Get Started",
                            onClick = {},
                            flavor = CitrosFlavor.TANGERINE,
                        )
                        CitrusHeroBadge(
                            flavor = CitrosFlavor.TANGERINE,
                            size = 68,
                        )
                        FlavorOptionCard(
                            flavor = CitrosFlavor.LEMON,
                            selected = true,
                            onClick = {},
                        )
                        FlavorOptionCard(
                            flavor = CitrosFlavor.BLOOD_ORANGE,
                            selected = false,
                            onClick = {},
                        )
                        PersonalityOptionChip(
                            text = "Friendly",
                            selected = true,
                            onClick = {},
                            flavor = CitrosFlavor.TANGERINE,
                        )
                        PersonalityOptionChip(
                            text = "Professional",
                            selected = false,
                            onClick = {},
                        )
                    }
                }
            }
        }
    }
}
