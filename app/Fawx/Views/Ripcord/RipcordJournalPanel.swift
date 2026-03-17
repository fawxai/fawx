import SwiftUI

struct RipcordJournalPanel: View {
    let status: RipcordStatusResponse?
    let entries: [JournalEntry]
    let isLoading: Bool
    let errorMessage: String?
    let isPerformingAction: Bool
    let refreshAction: () -> Void
    let pullAction: () -> Void
    let approveAction: () -> Void
    let dismissAction: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    overviewCard

                    if let errorMessage, !errorMessage.isEmpty {
                        statusCard(
                            title: "Could not load the journal",
                            message: errorMessage,
                            tint: .fawxError
                        )
                    }

                    if isLoading && entries.isEmpty {
                        ProgressView("Loading ripcord journal...")
                            .frame(maxWidth: .infinity, minHeight: 220)
                    } else if entries.isEmpty {
                        statusCard(
                            title: "No journaled actions yet",
                            message: "Actions recorded after the tripwire crosses will appear here."
                        )
                    } else {
                        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                            Text("Journal Entries")
                                .font(FawxTypography.heading2)
                                .foregroundStyle(Color.fawxText)

                            LazyVStack(spacing: FawxSpacing.paddingSM) {
                                ForEach(entries) { entry in
                                    RipcordJournalEntryCard(entry: entry)
                                }
                            }
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .navigationTitle("Ripcord Journal")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done", action: dismissAction)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Refresh", action: refreshAction)
                        .disabled(isLoading || isPerformingAction)
                }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                footer
            }
        }
        .frame(minWidth: 480, minHeight: 560)
    }

    private var overviewCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text(status?.displayDescription ?? "Tripwire crossed")
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text("Review everything captured since the ripcord activated, then either keep the changes or undo what can be reverted.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                ripcordInfoRow(label: "Tripwire ID", value: status?.tripwireId ?? "Unavailable")
                ripcordInfoRow(label: "Active since", value: activeSinceLabel)
                ripcordInfoRow(label: "Journaled", value: "\(entryCountForDisplay) actions")
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

    private var footer: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            Divider()
                .opacity(0.5)

            ViewThatFits(in: .vertical) {
                HStack(spacing: FawxSpacing.paddingMD) {
                    footerButtons
                }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    footerButtons
                }
            }
            .padding(.horizontal, FawxSpacing.paddingLG)
            .padding(.top, FawxSpacing.paddingSM)
            .padding(.bottom, FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.opacity(0.96))
    }

    private var footerButtons: some View {
        Group {
            Button("Pull Ripcord - Undo All", role: .destructive, action: pullAction)
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(isLoading || isPerformingAction || entries.isEmpty)

            Button("Approve - Keep Changes", action: approveAction)
                .buttonStyle(.bordered)
                .disabled(isLoading || isPerformingAction || status == nil)
        }
    }

    private var activeSinceLabel: String {
        if let date = status?.activatedDate ?? entries.first?.timestampDate {
            return makeRipcordJournalDateFormatter().string(from: date)
        }
        return "Unavailable"
    }

    private var entryCountForDisplay: Int {
        status?.entryCount ?? entries.count
    }

    @ViewBuilder
    private func ripcordInfoRow(label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
            Text(label)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 92, alignment: .leading)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func statusCard(
        title: String,
        message: String,
        tint: Color = .fawxWarning
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.opacity(0.08))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(tint.opacity(0.25), lineWidth: 1)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private struct RipcordJournalEntryCard: View {
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
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private func makeRipcordJournalDateFormatter() -> DateFormatter {
    let formatter = DateFormatter()
    formatter.dateStyle = .medium
    formatter.timeStyle = .short
    return formatter
}
