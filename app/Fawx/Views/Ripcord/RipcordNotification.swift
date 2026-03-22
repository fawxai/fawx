import SwiftUI

enum RipcordResolutionActionKind: Equatable {
    case dismiss
    case approve

    static func forPermissionMode(_ permissionMode: PermissionMode) -> Self {
        permissionMode == .capability ? .dismiss : .approve
    }

    var buttonTitle: String {
        switch self {
        case .dismiss:
            return "Dismiss"
        case .approve:
            return "Approve"
        }
    }
}

struct RipcordNotification: View {
    let snapshot: RipcordNotificationSnapshot
    let actions: RipcordNotificationActions

    var body: some View {
        FawxSurfaceCard(
            maxWidth: snapshot.maxWidth,
            borderColor: Color.fawxWarning.opacity(0.35),
            shadowStyle: FawxShadow.floatingPanel
        ) {
            header
            actionArea
        }
        .accessibilityElement(children: .contain)
    }

    private var header: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            Image(systemName: "exclamationmark.shield.fill")
                .font(.title3)
                .foregroundStyle(Color.fawxWarning)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                titleRow

                Text(snapshot.description)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
                    .lineLimit(2)

                Text(snapshot.entryCountLabel)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .lineLimit(1)
            }
        }
    }

    private var titleRow: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Text(snapshot.title)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            if snapshot.showsProgress {
                ProgressView()
                    .controlSize(.small)
            }
        }
    }

    private var actionArea: some View {
        ViewThatFits(in: .vertical) {
            HStack(spacing: FawxSpacing.paddingSM) {
                actionButtons
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                actionButtons
            }
        }
    }

    private var actionButtons: some View {
        Group {
            Button("Review", action: actions.review)
                .buttonStyle(.bordered)
                .disabled(snapshot.areActionsDisabled)

            Button("Pull Ripcord", role: .destructive, action: actions.pull)
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(snapshot.areActionsDisabled)

            RipcordResolutionButton(
                kind: snapshot.resolutionActionKind,
                action: actions.resolution,
                isDisabled: snapshot.areActionsDisabled
            )
        }
    }
}

struct RipcordNotificationSnapshot: Equatable {
    let title: String
    let description: String
    let entryCountLabel: String
    let resolutionActionKind: RipcordResolutionActionKind
    let showsProgress: Bool
    let areActionsDisabled: Bool
    let maxWidth: CGFloat

    init(
        status: RipcordStatusResponse,
        isPerformingAction: Bool,
        resolutionActionKind: RipcordResolutionActionKind,
        maxWidth: CGFloat = FawxSpacing.ripcordNotificationMaxWidth
    ) {
        title = "Ripcord Active"
        description = status.displayDescription
        entryCountLabel = status.entryCountLabel
        self.resolutionActionKind = resolutionActionKind
        showsProgress = isPerformingAction
        areActionsDisabled = isPerformingAction
        self.maxWidth = maxWidth
    }
}

struct RipcordNotificationActions {
    let review: () -> Void
    let pull: () -> Void
    let resolution: () -> Void
}

struct RipcordReviewTray: View {
    let snapshot: RipcordReviewTraySnapshot
    let actions: RipcordReviewTrayActions

    var body: some View {
        FawxSurfaceCard(
            maxWidth: snapshot.maxWidth,
            borderColor: Color.fawxWarning.opacity(0.35),
            shadowStyle: FawxShadow.floatingPanel
        ) {
            header
            summary
            journalContent
            footer
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            Image(systemName: "exclamationmark.shield.fill")
                .font(.title3)
                .foregroundStyle(Color.fawxWarning)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text(snapshot.title)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Text(snapshot.description)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .lineLimit(2)
            }

            Spacer(minLength: 0)

            headerButton(systemImage: "arrow.clockwise", action: actions.refresh)
                .disabled(snapshot.isLoading || snapshot.isPerformingAction)
                .accessibilityLabel("Refresh ripcord journal")

            headerButton(systemImage: "xmark", action: actions.close)
                .accessibilityLabel("Close ripcord review")
        }
    }

    private var summary: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Text(snapshot.entryCountLabel)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            if snapshot.isPerformingAction {
                ProgressView()
                    .controlSize(.small)
            }
        }
    }

    @ViewBuilder
    private var journalContent: some View {
        if let errorMessage = snapshot.errorMessage, !errorMessage.isEmpty {
            RipcordReviewStateCard(
                title: "Could not load the journal",
                message: errorMessage,
                tint: .fawxError
            )
        } else if snapshot.isLoading && snapshot.entries.isEmpty {
            ProgressView("Loading ripcord journal...")
                .frame(maxWidth: .infinity, minHeight: 180)
        } else if snapshot.entries.isEmpty {
            RipcordReviewStateCard(
                title: "No journaled actions yet",
                message: "Actions captured after the tripwire crossed will show up here."
            )
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    ForEach(snapshot.entries) { entry in
                        RipcordReviewEntryRow(entry: entry)
                    }
                }
            }
            .frame(maxHeight: 220)
        }
    }

    private var footer: some View {
        ViewThatFits(in: .vertical) {
            HStack(spacing: FawxSpacing.paddingSM) {
                footerButtons
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                footerButtons
            }
        }
    }

    private var footerButtons: some View {
        Group {
            Button("Pull Ripcord", role: .destructive, action: actions.pull)
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(snapshot.isPerformingAction)

            RipcordResolutionButton(
                kind: snapshot.resolutionActionKind,
                action: actions.resolution,
                isDisabled: snapshot.isPerformingAction
            )
        }
    }

    private func headerButton(systemImage: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Color.fawxTextSecondary)
                .padding(8)
                .background(Color.fawxBackground)
                .clipShape(Circle())
        }
        .buttonStyle(.plain)
    }
}

struct RipcordReviewTraySnapshot: Equatable {
    let title: String
    let description: String
    let entryCountLabel: String
    let entries: [JournalEntry]
    let isLoading: Bool
    let errorMessage: String?
    let isPerformingAction: Bool
    let resolutionActionKind: RipcordResolutionActionKind
    let maxWidth: CGFloat

    init(
        status: RipcordStatusResponse,
        entries: [JournalEntry],
        isLoading: Bool,
        errorMessage: String?,
        isPerformingAction: Bool,
        resolutionActionKind: RipcordResolutionActionKind,
        maxWidth: CGFloat = FawxSpacing.ripcordReviewTrayMaxWidth
    ) {
        title = "Ripcord Review"
        description = status.displayDescription
        entryCountLabel = status.entryCountLabel
        self.entries = entries
        self.isLoading = isLoading
        self.errorMessage = errorMessage
        self.isPerformingAction = isPerformingAction
        self.resolutionActionKind = resolutionActionKind
        self.maxWidth = maxWidth
    }
}

struct RipcordReviewTrayActions {
    let refresh: () -> Void
    let pull: () -> Void
    let resolution: () -> Void
    let close: () -> Void
}

struct FawxSurfaceCard<Content: View>: View {
    let spacing: CGFloat
    let padding: CGFloat
    let maxWidth: CGFloat?
    let backgroundColor: Color
    let borderColor: Color
    let shadowStyle: FawxShadowStyle?
    let content: Content

    init(
        spacing: CGFloat = FawxSpacing.paddingMD,
        padding: CGFloat = FawxSpacing.paddingLG,
        maxWidth: CGFloat? = nil,
        backgroundColor: Color = .fawxSurface,
        borderColor: Color = .fawxBorder,
        shadowStyle: FawxShadowStyle? = nil,
        @ViewBuilder content: () -> Content
    ) {
        self.spacing = spacing
        self.padding = padding
        self.maxWidth = maxWidth
        self.backgroundColor = backgroundColor
        self.borderColor = borderColor
        self.shadowStyle = shadowStyle
        self.content = content()
    }

    @ViewBuilder
    var body: some View {
        if let shadowStyle {
            baseCard
                .fawxShadow(shadowStyle)
        } else {
            baseCard
        }
    }

    private var baseCard: some View {
        VStack(alignment: .leading, spacing: spacing) {
            content
        }
        .frame(maxWidth: maxWidth, alignment: .leading)
        .padding(padding)
        .background(backgroundColor)
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(borderColor, lineWidth: 1)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct RipcordResolutionButton: View {
    let kind: RipcordResolutionActionKind
    let action: () -> Void
    let isDisabled: Bool

    @ViewBuilder
    var body: some View {
        if kind == .approve {
            Button(kind.buttonTitle, action: action)
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(isDisabled)
        } else {
            Button(kind.buttonTitle, action: action)
                .buttonStyle(.bordered)
                .disabled(isDisabled)
        }
    }
}

private struct RipcordReviewStateCard: View {
    let title: String
    let message: String
    let tint: Color

    init(title: String, message: String, tint: Color = .fawxWarning) {
        self.title = title
        self.message = message
        self.tint = tint
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.opacity(0.08))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(tint.opacity(0.25), lineWidth: 1)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct RipcordReviewEntryRow: View {
    let entry: JournalEntry

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                Text("#\(entry.id)")
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(Color.fawxTextSecondary)

                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.toolName)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    if let actionSummary = entry.actionSummary {
                        Text(actionSummary)
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxText)
                            .textSelection(.enabled)
                    }
                }

                Spacer(minLength: 0)

                Text(entry.reversible ? "Reversible" : "Audit only")
                    .font(FawxTypography.status)
                    .foregroundStyle(entry.reversible ? Color.fawxSuccess : Color.fawxWarning)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, FawxSpacing.paddingXS)
                    .background((entry.reversible ? Color.fawxSuccess : Color.fawxWarning).opacity(0.12))
                    .clipShape(Capsule())
            }

            if let actionContext = entry.actionContext {
                Text(actionContext)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            HStack(spacing: FawxSpacing.paddingSM) {
                ForEach(entry.metadataLabels, id: \.self) { label in
                    Text(label)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.fawxBackground)
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderSubtle), lineWidth: 1)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}
