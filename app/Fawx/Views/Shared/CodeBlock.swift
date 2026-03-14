import SwiftUI

#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

struct CodeBlock: View {
    let language: String?
    let content: String

    @State private var isHovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Text((language?.isEmpty == false ? language! : "plain text").uppercased())
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Spacer()

                if shouldShowCopyButton {
                    Button {
                        copyContent()
                    } label: {
                        Label("Copy", systemImage: "doc.on.doc")
                            .font(FawxTypography.status)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Color.fawxTextSecondary)
                }
            }
            .padding(.horizontal, FawxSpacing.paddingMD)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(Color.fawxSurfaceHover)

            ScrollView(.horizontal, showsIndicators: true) {
                Text(content)
                    .font(FawxTypography.code)
                    .foregroundStyle(Color.fawxText)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingMD)
            }
            .background(Color.fawxCode)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .onHover { hovering in
            isHovering = hovering
        }
    }

    private var shouldShowCopyButton: Bool {
#if os(macOS)
        isHovering
#else
        true
#endif
    }

    private func copyContent() {
#if os(iOS)
        UIPasteboard.general.string = content
#elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(content, forType: .string)
#endif
    }
}
