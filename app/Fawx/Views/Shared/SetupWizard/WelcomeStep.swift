import Observation
import SwiftUI

struct WelcomeStep: View {
    @Bindable var viewModel: SetupViewModel
    @Bindable var appState: AppState

    var body: some View {
        SetupWizardCard(maxWidth: 460) {
            VStack(spacing: FawxSpacing.paddingLG) {
                VStack(spacing: FawxSpacing.paddingSM) {
                    Image("FawxLogo")
                        .resizable()
                        .interpolation(.high)
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 200, height: 200)
                        .frame(width: 200, height: 200)

                    Text("Welcome to Fawx")
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(Color.fawxText)

                    Text("Your self-hosted AI agent.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)

                    Text("Fawx runs on your Mac and connects to Claude, ChatGPT, or Fireworks. Your conversations stay on your hardware. No cloud required.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                }

                Button("Get started") {
                    viewModel.continueFromWelcome()
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .frame(maxWidth: .infinity)

                if appState.canOpenRemoteOnboarding {
                    Button("Connect to another Fawx server instead") {
                        appState.beginRemoteOnboarding()
                    }
                    .buttonStyle(.plain)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxAccent)
                    .accessibilityIdentifier("connectToRemoteOnboardingButton")
                }

                Text("Fawx stores all data locally on this Mac. No telemetry is collected.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
        }
    }
}
