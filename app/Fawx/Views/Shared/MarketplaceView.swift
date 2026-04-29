import Observation
import SwiftUI

struct MarketplaceView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Bindable var skillsViewModel: SkillsViewModel
    let searchText: String

    var body: some View {
        if skillsViewModel.isSearchingMarketplace && skillsViewModel.marketplaceSkills.isEmpty {
            ProgressView("Searching marketplace...")
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(maxWidth: .infinity, minHeight: 260)
        } else if !skillsViewModel.marketplaceAvailable {
            SkillsPlaceholderView(
                systemImage: "antenna.radiowaves.left.and.right.slash",
                title: "Marketplace not yet connected",
                message: skillsViewModel.marketplaceMessage ?? "Try again later."
            )
            .frame(maxWidth: .infinity, minHeight: 260)
        } else if let errorMessage = skillsViewModel.marketplaceErrorMessage, skillsViewModel.marketplaceSkills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "exclamationmark.triangle",
                title: "Could not load marketplace",
                message: errorMessage,
                actionTitle: "Try Again",
                action: {
                    Task {
                        await skillsViewModel.searchMarketplace(query: searchText)
                    }
                }
            )
            .frame(maxWidth: .infinity, minHeight: 260)
        } else if skillsViewModel.marketplaceSkills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "magnifyingglass",
                title: searchText.isEmpty ? "Search the marketplace" : "No matching skills",
                message: searchText.isEmpty
                    ? "Find signed skills to install on your Fawx server."
                    : "Try a different search term."
            )
            .frame(maxWidth: .infinity, minHeight: 260)
        } else {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                if let message = skillsViewModel.marketplaceMessage, !message.isEmpty {
                    Text(message)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                LazyVGrid(columns: gridColumns, spacing: FawxSpacing.paddingMD) {
                    ForEach(skillsViewModel.marketplaceSkills) { skill in
                        MarketplaceSkillCard(
                            skill: skill,
                            isLoadedOnServer: skillsViewModel.isLoadedOnServer(skill),
                            isInstalling: skillsViewModel.installingSkillNames.contains(skill.name)
                        ) {
                            Task {
                                await skillsViewModel.installMarketplaceSkill(named: skill.name)
                            }
                        }
                    }
                }
            }
        }
    }

    private var gridColumns: [GridItem] {
#if os(macOS)
        [
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
        ]
#else
        if horizontalSizeClass == .regular {
            [
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            ]
        } else {
            [GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD)]
        }
#endif
    }
}

private struct MarketplaceSkillCard: View {
    let skill: MarketplaceSkillSummary
    let isLoadedOnServer: Bool
    let isInstalling: Bool
    let installAction: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(Color.fawxAccentSubtle)
                    .frame(width: 40, height: 40)
                    .overlay {
                        Text(String(skill.title.prefix(1)).uppercased())
                            .font(.system(size: 18, weight: .bold))
                            .foregroundStyle(Color.fawxAccent)
                    }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    HStack(spacing: FawxSpacing.paddingXS) {
                        Text(skill.title)
                            .font(FawxTypography.heading2)
                            .foregroundStyle(Color.fawxText)
                            .lineLimit(1)

                        if skill.signed {
                            MarketplaceBadge(label: "Verified", tone: .verified)
                        }
                    }

                    Text("by \(skill.publisher)")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)
            }

            Text(skill.description)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)

            Spacer(minLength: 0)

            HStack {
                if isLoadedOnServer {
                    MarketplaceBadge(label: LoadedSkillsCopy.serverLoaded.statusLabel, tone: .loaded)
                } else {
                    Button(isInstalling ? "Installing..." : "Install") {
                        installAction()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(isInstalling)
                }

                Spacer(minLength: 0)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .fawxSurface(.field)
    }
}

private struct MarketplaceBadge: View {
    enum Tone {
        case verified
        case loaded
    }

    let label: String
    let tone: Tone

    var body: some View {
        Text(label)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(foregroundColor)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 5)
            .background(backgroundColor)
            .clipShape(Capsule())
    }

    private var foregroundColor: Color {
        switch tone {
        case .verified:
            .fawxSuccess
        case .loaded:
            .fawxTextSecondary
        }
    }

    private var backgroundColor: Color {
        switch tone {
        case .verified:
            Color.fawxSuccess.opacity(0.12)
        case .loaded:
            Color.fawxSurfaceActive.opacity(0.12)
        }
    }
}
