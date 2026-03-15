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
                        .foregroundStyle(Color.fawxAccent)

                    Text(toolCall.displayName)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)

                    Spacer()

                    if toolCall.isRunning {
                        ProgressView()
                            .controlSize(.small)
                        Text("Running...")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    } else if toolCall.isError {
                        Text("Error")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxError)
                    }

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
                        CodeBlock(language: "json", content: toolCall.arguments)
                    }

                    if let result = toolCall.result, !result.isEmpty {
                        CodeBlock(language: nil, content: result)
                    }
                }
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
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
}
