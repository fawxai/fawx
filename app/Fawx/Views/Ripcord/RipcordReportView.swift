import SwiftUI

struct RipcordReportView: View {
    let report: RipcordReport
    let dismissAction: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    summaryCard

                    if !report.reverted.isEmpty {
                        reportSection(
                            title: "Reverted",
                            tint: .fawxSuccess,
                            items: report.reverted.map { entry in
                                RipcordReportRowModel(
                                    id: entry.id,
                                    toolName: entry.toolName,
                                    detail: entry.description
                                )
                            }
                        )
                    }

                    if !report.skipped.isEmpty {
                        reportSection(
                            title: "Skipped",
                            tint: .fawxWarning,
                            items: report.skipped.map { entry in
                                RipcordReportRowModel(
                                    id: entry.id,
                                    toolName: entry.toolName,
                                    detail: entry.reason
                                )
                            }
                        )
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .navigationTitle("Ripcord Pulled")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done", action: dismissAction)
                }
            }
        }
        .frame(minWidth: 460, minHeight: 520)
    }

    private var summaryCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Undo completed")
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                summaryRow(
                    symbolName: "checkmark.circle.fill",
                    label: "Reverted",
                    value: "\(report.reverted.count) action\(report.reverted.count == 1 ? "" : "s")",
                    tint: .fawxSuccess
                )
                summaryRow(
                    symbolName: "exclamationmark.triangle.fill",
                    label: "Skipped",
                    value: "\(report.skipped.count) action\(report.skipped.count == 1 ? "" : "s")",
                    tint: .fawxWarning
                )
                summaryRow(
                    symbolName: "number.circle.fill",
                    label: "Processed",
                    value: "\(report.total) total entries",
                    tint: .fawxAccent
                )
            }
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func summaryRow(
        symbolName: String,
        label: String,
        value: String,
        tint: Color
    ) -> some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: symbolName)
                .foregroundStyle(tint)

            Text(label)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 72, alignment: .leading)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
    }

    private func reportSection(
        title: String,
        tint: Color,
        items: [RipcordReportRowModel]
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            LazyVStack(spacing: FawxSpacing.paddingSM) {
                ForEach(items) { item in
                    RipcordReportRow(item: item, tint: tint)
                }
            }
        }
    }
}

private struct RipcordReportRowModel: Identifiable {
    let id: Int
    let toolName: String
    let detail: String
}

private struct RipcordReportRow: View {
    let item: RipcordReportRowModel
    let tint: Color

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                Image(systemName: "circle.fill")
                    .font(.system(size: 8))
                    .foregroundStyle(tint)
                    .padding(.top, 6)

                VStack(alignment: .leading, spacing: 2) {
                    Text(item.toolName)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    Text(item.detail)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .textSelection(.enabled)
                }
            }
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}
