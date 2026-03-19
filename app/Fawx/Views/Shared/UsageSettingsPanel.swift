import Observation
import SwiftUI

struct UsageSettingsPanel: View {
    @Bindable var viewModel: UsageViewModel

    private static let currencyFormatter: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.numberStyle = .currency
        formatter.currencyCode = "USD"
        formatter.minimumFractionDigits = 2
        formatter.maximumFractionDigits = 2
        return formatter
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            if viewModel.isLoading && viewModel.usage == nil {
                ProgressView("Loading usage...")
                    .frame(maxWidth: .infinity, minHeight: 160)
            } else if let errorMessage = viewModel.errorMessage, viewModel.usage == nil {
                Text(errorMessage)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxError)
            } else if let usage = viewModel.usage {
                if usageUnavailable(usage) {
                    Text("Usage tracking not yet available")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                } else {
                    sessionCard(usage.session)
                    todayCard(usage.today)
                    providerBreakdown(usage.providers)
                }
            } else {
                Text("Usage tracking not yet available")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
        .task {
            await viewModel.refresh()
        }
    }

    private func sessionCard(_ session: SessionUsage) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Current Session")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                Text(currencyString(session.estimatedCostUsd))
                    .font(FawxTypography.heading1)
                    .foregroundStyle(Color.fawxText)

                Text("\(session.totalTokens.formatted()) tokens")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Text("\(session.inputTokens.formatted()) in · \(session.outputTokens.formatted()) out")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func todayCard(_ today: PeriodUsage) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("Today")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            HStack {
                usageMetric(title: "Input", value: today.inputTokens.formatted())
                usageMetric(title: "Output", value: today.outputTokens.formatted())
                usageMetric(title: "Total", value: today.totalTokens.formatted())
                usageMetric(title: "Cost", value: currencyString(today.estimatedCostUsd))
            }
        }
    }

    private func providerBreakdown(_ providers: [ProviderUsage]) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("By Provider")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            VStack(spacing: FawxSpacing.paddingSM) {
                ForEach(Array(providers.enumerated()), id: \.element) { index, provider in
                    HStack(spacing: FawxSpacing.paddingMD) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(providerColor(index))
                            .frame(width: 4, height: 34)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(abbreviateModelName(provider.model))
                                .font(FawxTypography.chatBody)
                                .foregroundStyle(Color.fawxText)

                            Text("\(providerDisplayName(provider.provider)) · \(provider.inputTokens + provider.outputTokens) tokens")
                                .font(FawxTypography.status)
                                .foregroundStyle(Color.fawxTextSecondary)
                        }

                        Spacer(minLength: 0)

                        Text(currencyString(provider.estimatedCostUsd))
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxText)
                    }
                    .padding(FawxSpacing.paddingMD)
                    .background(Color.fawxBackground)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                }
            }

            Text("Estimated based on published API pricing.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private func usageMetric(title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private func usageUnavailable(_ usage: UsageResponse) -> Bool {
        usage.session.totalTokens == 0
            && usage.today.totalTokens == 0
            && usage.providers.allSatisfy { provider in
                provider.inputTokens == 0
                    && provider.outputTokens == 0
                    && provider.estimatedCostUsd == 0
            }
    }

    private func currencyString(_ value: Double) -> String {
        Self.currencyFormatter.string(from: NSNumber(value: value)) ?? "$0.00"
    }

    private func providerColor(_ index: Int) -> Color {
        let palette: [Color] = [.fawxAccent, .fawxSuccess, .fawxWarning, .fawxError]
        return palette[index % palette.count]
    }

    private func providerDisplayName(_ provider: String) -> String {
        switch provider.lowercased() {
        case "openai":
            "OpenAI"
        case "anthropic":
            "Anthropic"
        case "google":
            "Google"
        case "openrouter":
            "OpenRouter"
        default:
            provider
                .replacingOccurrences(of: "-", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }
    }
}
