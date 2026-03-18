import SwiftUI

struct ToolCallCard: View {
    let toolCall: ToolCallRecord

    @State private var isExpanded: Bool

    init(toolCall: ToolCallRecord) {
        self.toolCall = toolCall
        _isExpanded = State(initialValue: toolCall.isRunning)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Button {
                withAnimation(.easeInOut(duration: 0.18)) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: FawxSpacing.paddingSM) {
                    Image(systemName: "wrench.and.screwdriver")
                        .foregroundStyle(statusColor)
                        .padding(8)
                        .background(statusColor.opacity(FawxOpacity.fillSubtle))
                        .clipShape(Circle())

                    VStack(alignment: .leading, spacing: 2) {
                        Text(toolCall.displayName)
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)
                            .lineLimit(1)

                        Text(detailSummary)
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                            .lineLimit(1)
                    }

                    Spacer()

                    if toolCall.isRunning {
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
            .buttonStyle(.plain)
            .accessibilityLabel(toolCallAccessibilityLabel)
            .accessibilityHint(isExpanded ? "Collapse tool details" : "Expand tool details")

            if isExpanded {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
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
        }
        .padding(FawxSpacing.paddingMD)
        .frame(maxWidth: FawxSpacing.maxMessageWidth, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(cardBackground)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(cardBorderColor, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .onChange(of: toolCall.isRunning) { _, isRunning in
            if !isRunning {
                isExpanded = false
            }
        }
    }

    private var toolCallAccessibilityLabel: String {
        if toolCall.isRunning {
            return "\(toolCall.displayName), running"
        }
        if toolCall.isError {
            return "\(toolCall.displayName), error"
        }
        return toolCall.displayName
    }

    private var detailSummary: String {
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

    private var statusText: String {
        if toolCall.isRunning {
            return "Running"
        }
        if toolCall.isError {
            return "Error"
        }
        return "Complete"
    }

    private var statusColor: Color {
        if toolCall.isRunning {
            return .fawxAccent
        }
        if toolCall.isError {
            return .fawxError
        }
        return .fawxTextSecondary
    }

    private var cardBackground: Color {
        if toolCall.isRunning {
            return Color.fawxAccentSubtle
        }
        if toolCall.isError {
            return Color.fawxError.opacity(FawxOpacity.errorFill)
        }
        return Color.fawxSurface
    }

    private var cardBorderColor: Color {
        if toolCall.isRunning {
            return Color.fawxAccent.opacity(FawxOpacity.accentBorder)
        }
        if toolCall.isError {
            return Color.fawxError.opacity(FawxOpacity.errorBorder)
        }
        return Color.fawxBorder
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
