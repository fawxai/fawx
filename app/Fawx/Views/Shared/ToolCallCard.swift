import SwiftUI

struct ToolActivityGroupCardSnapshot: Equatable {
    enum DetailStyle: Equatable {
        case collapsed
        case liveStatusOnly
        case historicalPayload
    }

    let headerTitle: String
    let groupSummary: String
    let statusText: String
    let accessibilityLabel: String
    let accessibilityHint: String
    let showsProgress: Bool
    let canExpand: Bool
    let isExpanded: Bool
    let visibleToolCalls: [ToolCallRecord]
    let detailStyle: DetailStyle

    init(group: ToolActivityGroupRecord, isExpanded: Bool) {
        let canExpand = !group.toolCalls.isEmpty
        let effectiveExpanded = canExpand && isExpanded
        let primaryToolCall = group.toolCalls.last(where: \.isRunning) ?? group.toolCalls.last
        let detailStyle: DetailStyle

        if !effectiveExpanded {
            detailStyle = .collapsed
        } else if group.isLive {
            detailStyle = .liveStatusOnly
        } else {
            detailStyle = .historicalPayload
        }

        let headerTitle: String
        if !effectiveExpanded, let primaryToolCall {
            let additionalToolCount = max(0, group.toolCount - 1)
            if additionalToolCount > 0 {
                headerTitle = "\(primaryToolCall.displayName) +\(additionalToolCount)"
            } else {
                headerTitle = primaryToolCall.displayName
            }
        } else {
            headerTitle = "Tool Activity"
        }

        let countLabel = group.toolCount == 1 ? "1 tool" : "\(group.toolCount) tools"
        let groupSummary: String
        if group.runningCount > 0 {
            groupSummary = "\(countLabel), \(group.runningCount) running"
        } else if group.errorCount > 0 {
            let failedLabel = group.errorCount == 1 ? "1 failed" : "\(group.errorCount) failed"
            groupSummary = "\(countLabel), \(failedLabel)"
        } else if group.completedCount == group.toolCount {
            groupSummary = "\(countLabel), completed"
        } else {
            groupSummary = countLabel
        }

        let statusText: String
        if group.runningCount > 0 {
            statusText = "Running"
        } else if group.errorCount > 0 {
            statusText = "Error"
        } else {
            statusText = "Complete"
        }

        self.headerTitle = headerTitle
        self.groupSummary = groupSummary
        self.statusText = statusText
        accessibilityLabel = "\(headerTitle), \(groupSummary)"
        if !canExpand {
            accessibilityHint = "Tool activity details are unavailable."
        } else if effectiveExpanded {
            if group.isLive {
                accessibilityHint = "Collapse tool activity. Detailed arguments and output appear after the response finishes."
            } else {
                accessibilityHint = "Collapse tool activity"
            }
        } else {
            accessibilityHint = "Expand tool activity"
        }
        showsProgress = group.runningCount > 0
        self.canExpand = canExpand
        self.isExpanded = effectiveExpanded
        visibleToolCalls = effectiveExpanded ? group.toolCalls : []
        self.detailStyle = detailStyle
    }

    var showsPayloadDetails: Bool {
        detailStyle == .historicalPayload
    }
}

struct ToolActivityGroupCard: View {
    @Environment(\.containerWidth) private var containerWidth

    let group: ToolActivityGroupRecord

    @State private var isExpanded: Bool

    init(group: ToolActivityGroupRecord) {
        self.group = group
        _isExpanded = State(initialValue: false)
    }

    private var snapshot: ToolActivityGroupCardSnapshot {
        ToolActivityGroupCardSnapshot(group: group, isExpanded: isExpanded)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            cardContent

            Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var cardContent: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Button {
                guard snapshot.canExpand else {
                    return
                }
                withAnimation(.easeInOut(duration: 0.18)) {
                    isExpanded.toggle()
                }
            } label: {
                headerLabel
            }
            .buttonStyle(.plain)
            .disabled(!snapshot.canExpand)
            .accessibilityLabel(snapshot.accessibilityLabel)
            .accessibilityHint(snapshot.accessibilityHint)

            if snapshot.isExpanded {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                    if snapshot.detailStyle == .liveStatusOnly {
                        Text("Detailed arguments and output appear after the response finishes.")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }

                    ForEach(Array(snapshot.visibleToolCalls.enumerated()), id: \.element.id) { index, toolCall in
                        if index > 0 {
                            Divider()
                                .overlay(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                        }
                        toolRow(toolCall, showsPayloadDetails: snapshot.showsPayloadDetails)
                    }
                }
            }
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth), alignment: .leading)
        .background(cardBackground)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(cardBorderColor, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private var headerLabel: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: "wrench.and.screwdriver")
                .foregroundStyle(statusColor)
                .padding(8)
                .background(statusColor.opacity(FawxOpacity.fillSubtle))
                .clipShape(Circle())

            VStack(alignment: .leading, spacing: 2) {
                Text(snapshot.headerTitle)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)
                    .lineLimit(1)

                Text(snapshot.groupSummary)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if snapshot.showsProgress {
                ProgressView()
                    .controlSize(.small)
            }

            Text(snapshot.statusText)
                .font(FawxTypography.status)
                .foregroundStyle(statusColor)
                .padding(.horizontal, FawxSpacing.paddingSM)
                .padding(.vertical, FawxSpacing.paddingXS)
                .background(statusColor.opacity(FawxOpacity.fillSubtle))
                .clipShape(Capsule())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Status \(snapshot.statusText)")

            if snapshot.canExpand {
                Image(systemName: snapshot.isExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
    }

    private var statusColor: Color {
        if group.runningCount > 0 {
            return .fawxAccent
        }

        if group.errorCount > 0 {
            return .fawxError
        }

        return .fawxTextSecondary
    }

    private var cardBackground: Color {
        if group.runningCount > 0 {
            return Color.fawxAccentSubtle
        }

        if group.errorCount > 0 {
            return Color.fawxError.opacity(FawxOpacity.errorFill)
        }

        return Color.fawxSurface
    }

    private var cardBorderColor: Color {
        if group.runningCount > 0 {
            return Color.fawxAccent.opacity(FawxOpacity.accentBorder)
        }

        if group.errorCount > 0 {
            return Color.fawxError.opacity(FawxOpacity.errorBorder)
        }

        return Color.fawxBorder
    }

    private func toolRow(_ toolCall: ToolCallRecord, showsPayloadDetails: Bool) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(toolCall.displayName)
                        .font(FawxTypography.chatBody.weight(.semibold))
                        .foregroundStyle(Color.fawxText)

                    Text(toolCallSummary(toolCall))
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)

                Text(toolCallStatusText(toolCall))
                    .font(FawxTypography.status)
                    .foregroundStyle(toolCallStatusColor(toolCall))
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, FawxSpacing.paddingXS)
                    .background(toolCallStatusColor(toolCall).opacity(FawxOpacity.fillSubtle))
                    .clipShape(Capsule())
            }

            if showsPayloadDetails, !toolCall.arguments.isEmpty {
                detailSection(title: "Arguments") {
                    CodeBlock(language: "json", content: toolCall.arguments)
                }
            }

            if showsPayloadDetails, let result = toolCall.result, !result.isEmpty {
                detailSection(title: toolCall.isError ? "Error Output" : "Result") {
                    CodeBlock(language: nil, content: result)
                }
            }
        }
    }

    private func toolCallSummary(_ toolCall: ToolCallRecord) -> String {
        if toolCall.isRunning {
            return "Working with live output"
        }

        if toolCall.isError {
            return "Finished with an error"
        }

        if let result = toolCall.result, !result.isEmpty {
            return "Completed with output"
        }

        return "Completed"
    }

    private func toolCallStatusText(_ toolCall: ToolCallRecord) -> String {
        if toolCall.isRunning {
            return "Running"
        }

        if toolCall.isError {
            return "Error"
        }

        return "Complete"
    }

    private func toolCallStatusColor(_ toolCall: ToolCallRecord) -> Color {
        if toolCall.isRunning {
            return .fawxAccent
        }

        if toolCall.isError {
            return .fawxError
        }

        return .fawxTextSecondary
    }

    private func detailSection<Content: View>(
        title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .textCase(.uppercase)
                .accessibilityAddTraits(.isHeader)

            content()
        }
        .accessibilityElement(children: .contain)
    }
}
