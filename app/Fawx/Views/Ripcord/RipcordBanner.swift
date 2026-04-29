import SwiftUI

struct RipcordBanner: View {
    let status: RipcordStatusResponse
    let isPerformingAction: Bool
    let reviewAction: () -> Void
    let pullAction: () -> Void
    let approveAction: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                Image(systemName: "exclamationmark.shield.fill")
                    .font(.title3)
                    .foregroundStyle(Color.fawxWarning)

                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    HStack(spacing: FawxSpacing.paddingSM) {
                        Text("Ripcord Active")
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)

                        if isPerformingAction {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    Text(status.displayDescription)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxText)

                    Text(status.entryCountLabel)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)
            }

            ViewThatFits(in: .vertical) {
                HStack(spacing: FawxSpacing.paddingSM) {
                    actionButtons
                }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    actionButtons
                }
            }
        }
        .padding(FawxSpacing.paddingLG)
        .fawxTransientSurface(borderColor: Color.fawxWarning.opacity(0.35), shadowStyle: nil)
        .accessibilityElement(children: .contain)
    }

    private var actionButtons: some View {
        Group {
            Button("Review", action: reviewAction)
                .buttonStyle(.bordered)
                .disabled(isPerformingAction)

            Button("Pull Ripcord", role: .destructive, action: pullAction)
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(isPerformingAction)

            Button("Approve", action: approveAction)
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(isPerformingAction)
        }
    }
}
