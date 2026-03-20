import SwiftUI

struct RipcordNotification: View {
    let status: RipcordStatusResponse
    let isPerformingAction: Bool
    let reviewAction: () -> Void
    let pullAction: () -> Void
    let approveAction: () -> Void
    let dismissAction: () -> Void

    var snapshot: RipcordNotificationSnapshot {
        RipcordNotificationSnapshot(status: status, isPerformingAction: isPerformingAction)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            header
            actionArea
        }
        .frame(maxWidth: snapshot.maxWidth, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxWarning.opacity(0.35), lineWidth: 1)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .fawxShadow(FawxShadow.floatingPanel)
        .accessibilityElement(children: .contain)
    }

    private var header: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            Image(systemName: "exclamationmark.shield.fill")
                .font(.title3)
                .foregroundStyle(Color.fawxWarning)

            notificationCopy

            Spacer(minLength: 0)

            dismissButton
        }
    }

    private var notificationCopy: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Text(snapshot.title)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                if snapshot.showsProgress {
                    ProgressView()
                        .controlSize(.small)
                }
            }

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

    private var dismissButton: some View {
        Button(action: dismissAction) {
            Image(systemName: "xmark")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Color.fawxTextSecondary)
                .padding(8)
                .background(Color.fawxBackground)
                .clipShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Dismiss ripcord notification")
        .disabled(snapshot.isDismissDisabled)
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
            Button("Review", action: reviewAction)
                .buttonStyle(.bordered)
                .disabled(snapshot.areActionsDisabled)

            Button("Pull Ripcord", role: .destructive, action: pullAction)
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(snapshot.areActionsDisabled)

            Button("Approve", action: approveAction)
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(snapshot.areActionsDisabled)
        }
    }
}

struct RipcordNotificationSnapshot: Equatable {
    let title: String
    let description: String
    let entryCountLabel: String
    let showsProgress: Bool
    let areActionsDisabled: Bool
    let isDismissDisabled: Bool
    let maxWidth: CGFloat

    init(status: RipcordStatusResponse, isPerformingAction: Bool) {
        title = "Ripcord Active"
        description = status.displayDescription
        entryCountLabel = status.entryCountLabel
        showsProgress = isPerformingAction
        areActionsDisabled = isPerformingAction
        isDismissDisabled = isPerformingAction
        maxWidth = FawxSpacing.ripcordNotificationMaxWidth
    }
}
