import Observation
import SwiftUI

struct SkillsView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Bindable var skillsViewModel: SkillsViewModel
    let showsHeader: Bool

    init(skillsViewModel: SkillsViewModel, showsHeader: Bool = true) {
        _skillsViewModel = Bindable(skillsViewModel)
        self.showsHeader = showsHeader
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
                if showsHeader {
                    header
                }

                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(containerPadding)
        }
        .background(Color.fawxBackground)
        .task {
            await skillsViewModel.refresh()
        }
        .refreshable {
            await skillsViewModel.refresh()
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text("Skills")
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text("Loaded on server")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    @ViewBuilder
    private var content: some View {
        if skillsViewModel.isLoading && skillsViewModel.skills.isEmpty {
            ProgressView("Loading skills...")
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(maxWidth: .infinity, minHeight: 280)
        } else if let errorMessage = skillsViewModel.errorMessage, skillsViewModel.skills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "exclamationmark.triangle",
                title: "Could not load skills",
                message: errorMessage,
                actionTitle: "Try Again",
                action: {
                    Task {
                        await skillsViewModel.refresh()
                    }
                }
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        } else if skillsViewModel.skills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "puzzlepiece.extension",
                title: "No skills loaded",
                message: "Skills are loaded on the Fawx server. Check your server configuration."
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        } else {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                if !showsHeader {
                    Text("Loaded on server")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                LazyVGrid(columns: gridColumns, spacing: FawxSpacing.paddingMD) {
                    ForEach(skillsViewModel.skills) { skill in
                        SkillCardView(skill: skill)
                    }
                }
                .accessibilityIdentifier("skillsGrid")
            }
        }
    }

    private var gridColumns: [GridItem] {
#if os(macOS)
        return [
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
        ]
#else
        if horizontalSizeClass == .regular {
            return [
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            ]
        }
        return [GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD)]
#endif
    }

    private var containerPadding: CGFloat {
#if os(macOS)
        FawxSpacing.paddingXL
#else
        FawxSpacing.paddingLG
#endif
    }
}

private struct SkillCardView: View {
    let skill: SkillSummary

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(Color.fawxAccentSubtle)
                    .frame(width: 32, height: 32)
                    .overlay {
                        Image(systemName: "puzzlepiece.extension.fill")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color.fawxAccent)
                    }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(skill.name)
                        .font(FawxTypography.heading2)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)

                    SkillStatusPill(isEnabled: skill.isEnabled)
                }

                Spacer(minLength: 0)
            }

            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 72), spacing: FawxSpacing.paddingXS)],
                alignment: .leading,
                spacing: FawxSpacing.paddingXS
            ) {
                ForEach(skill.tools, id: \.self) { tool in
                    Text(tool)
                        .font(.system(size: 11, weight: .medium, design: .monospaced))
                        .foregroundStyle(Color.fawxTextSecondary)
                        .padding(.horizontal, FawxSpacing.paddingSM)
                        .padding(.vertical, 6)
                        .background(Color.fawxSurface)
                        .clipShape(Capsule())
                }
            }

            Text(skill.displayDescription ?? "Loaded on the Fawx server.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxBackground)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct SkillStatusPill: View {
    let isEnabled: Bool

    var body: some View {
        Text(isEnabled ? "Enabled" : "Disabled")
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(isEnabled ? Color.fawxSuccess : Color.fawxTextSecondary)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 5)
            .background((isEnabled ? Color.fawxSuccess : Color.fawxSurfaceActive).opacity(0.12))
            .clipShape(Capsule())
    }
}

private struct SkillsPlaceholderView: View {
    let systemImage: String
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: systemImage)
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Color.fawxAccent.opacity(0.35))

            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)

            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.bordered)
            }
        }
        .frame(maxWidth: .infinity)
    }
}
