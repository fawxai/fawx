import Observation
import SwiftUI

struct ProviderStep: View {
    @Bindable var viewModel: SetupViewModel
    @State private var oauthCoordinator = OAuthSessionCoordinator()

    var body: some View {
        SetupWizardCard(maxWidth: 520) {
            SetupWizardHeader(
                title: "Add an AI Provider",
                detail: "Connect Claude, ChatGPT, or Fireworks so your local server is ready to chat."
            )

            VStack(spacing: FawxSpacing.paddingMD) {
                ForEach(SetupProvider.allCases) { provider in
                    SetupChoiceCard(
                        isSelected: viewModel.selectedProvider == provider,
                        iconText: providerIconText(provider),
                        title: provider.displayName,
                        subtitle: provider.companyName,
                        action: {
                            viewModel.selectProvider(provider)
                        }
                    )
                }
            }
            .disabled(viewModel.isRefreshing)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("How do you want to connect?")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                ForEach(viewModel.availableAuthMethods) { method in
                    SetupRadioRow(
                        isSelected: viewModel.selectedAuthMethod == method,
                        title: method.title,
                        action: {
                            viewModel.selectAuthMethod(method)
                        }
                    )
                }
            }
            .padding(FawxSpacing.paddingMD)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.fawxBorder, lineWidth: 1)
            }
            .disabled(viewModel.isRefreshing)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                if viewModel.selectedProvider == .anthropic && viewModel.selectedAuthMethod == .subscription {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text("Run `claude setup-token` in Terminal to start Anthropic sign-in and generate a setup token.")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxTextSecondary)

                        Text("After `claude setup-token` finishes, paste the returned token below.")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                }

                if viewModel.selectedProvider == .openai && viewModel.selectedAuthMethod == .subscription {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text("Use your ChatGPT subscription to sign in with OpenAI.")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxTextSecondary)

                        Button(viewModel.isSubmittingProvider ? "Connecting..." : "Sign in with ChatGPT") {
                            Task {
                                await viewModel.startOpenAISubscriptionLogin(using: oauthCoordinator)
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.fawxAccent)
                        .disabled(viewModel.isSubmittingProvider)

                        Text("Fawx opens ChatGPT sign-in in a secure browser sheet and completes setup automatically.")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                }

                if viewModel.showsCredentialInput {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text(viewModel.providerFieldTitle)
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)

                        SecureField(viewModel.providerFieldPrompt, text: $viewModel.credentialInput)
                            .textFieldStyle(.roundedBorder)
                    }
                }

                HStack(spacing: FawxSpacing.paddingMD) {
                    if viewModel.showsCredentialSubmitButton {
                        Button(viewModel.isSubmittingProvider ? "Saving..." : viewModel.providerActionTitle) {
                            Task {
                                await viewModel.submitProviderCredentials()
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.fawxAccent)
                        .disabled(viewModel.isSubmittingProvider)
                    }

                    Button("Verify") {
                        Task {
                            await viewModel.verifySelectedProvider()
                        }
                    }
                    .buttonStyle(.bordered)
                    .disabled(viewModel.isSubmittingProvider || !viewModel.selectedProviderConfigured)
                }
            }
            .disabled(viewModel.isRefreshing)

            if viewModel.isRefreshing, let bootstrapProgress = viewModel.bootstrapProgress {
                HStack(spacing: FawxSpacing.paddingSM) {
                    ProgressView()

                    Text(bootstrapProgress)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }

            SetupStatusMessageView(
                kind: viewModel.providerStatusKind,
                message: viewModel.providerStatusMessage
            )

            HStack(spacing: FawxSpacing.paddingMD) {
                Button("Back") {
                    viewModel.goBack()
                }
                .buttonStyle(.bordered)

                Spacer(minLength: 0)

                Button("Skip for now") {
                    viewModel.skipProvider()
                }
                .buttonStyle(.bordered)

                Button("Continue") {
                    viewModel.continueFromProvider()
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
            }
            .disabled(viewModel.isRefreshing)
        }
    }
}

private func providerIconText(_ provider: SetupProvider) -> String {
    switch provider {
    case .anthropic:
        "A"
    case .openai:
        "O"
    case .fireworks:
        "F"
    }
}
