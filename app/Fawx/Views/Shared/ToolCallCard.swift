import SwiftUI

struct ToolActivityGroupCard: View {
    @Environment(\.containerWidth) private var containerWidth

    let group: ToolActivityGroupRecord

    @State private var isExpanded: Bool

    init(group: ToolActivityGroupRecord) {
        self.group = group
        _isExpanded = State(initialValue: group.runningCount > 0)
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
                withAnimation(.easeInOut(duration: 0.18)) {
                    isExpanded.toggle()
                }
            } label: {
                headerLabel
            }
            .buttonStyle(.plain)
            .accessibilityLabel(accessibilityLabel)
            .accessibilityHint(isExpanded ? "Collapse tool activity" : "Expand tool activity")

            if isExpanded {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                    ForEach(Array(group.toolCalls.enumerated()), id: \.element.id) { index, toolCall in
                        if index > 0 {
                            Divider()
                                .overlay(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                        }
                        toolRow(toolCall)
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
        .onChange(of: group.toolCalls) { _, toolCalls in
            if toolCalls.contains(where: \.isRunning) {
                isExpanded = true
            }
        }
    }

    private var headerLabel: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: "wrench.and.screwdriver")
                .foregroundStyle(statusColor)
                .padding(8)
                .background(statusColor.opacity(FawxOpacity.fillSubtle))
                .clipShape(Circle())

            VStack(alignment: .leading, spacing: 2) {
                Text("Tool Activity")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)
                    .lineLimit(1)

                Text(groupSummary)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if group.runningCount > 0 {
                ProgressView()
                    .controlSize(.small)
            }

            Text(statusText)
                .font(FawxTypography.status)
                .foregroundStyle(statusColor)
                .padding(.horizontal, FawxSpacing.paddingSM)
                .padding(.vertical, FawxSpacing.paddingXS)
                .background(statusColor.opacity(FawxOpacity.fillSubtle))
                .clipShape(Capsule())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Status \(statusText)")

            Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private var groupSummary: String {
        let countLabel = group.toolCount == 1 ? "1 tool" : "\(group.toolCount) tools"

        if group.runningCount > 0 {
            return "\(countLabel), \(group.runningCount) running"
        }

        if group.errorCount > 0 {
            let failedLabel = group.errorCount == 1 ? "1 failed" : "\(group.errorCount) failed"
            return "\(countLabel), \(failedLabel)"
        }

        if group.completedCount == group.toolCount {
            return "\(countLabel), completed"
        }

        return countLabel
    }

    private var statusText: String {
        if group.runningCount > 0 {
            return "Running"
        }

        if group.errorCount > 0 {
            return "Error"
        }

        return "Complete"
    }

    private var accessibilityLabel: String {
        "Tool Activity, \(groupSummary)"
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

    private func toolRow(_ toolCall: ToolCallRecord) -> some View {
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

            if !toolCall.arguments.isEmpty {
                detailSection(title: "Arguments") {
                    CodeBlock(language: "json", content: toolCall.arguments)
                }
            }

            if let result = toolCall.result, !result.isEmpty {
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
