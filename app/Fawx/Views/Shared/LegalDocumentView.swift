import MarkdownUI
import SwiftUI

struct LegalDocumentView: View {
    let title: String
    let resourceName: String

    var body: some View {
        ScrollView {
            Group {
                if let content = LegalDocumentLoader.markdown(named: resourceName) {
                    LegalMarkdownContent(content: content)
                } else {
                    Text("Document unavailable")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle(title)
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private enum LegalDocumentLoader {
    static func markdown(named resourceName: String, bundle: Bundle = .main) -> String? {
        guard let url = bundle.url(forResource: resourceName, withExtension: "md"),
              let content = try? String(contentsOf: url, encoding: .utf8) else {
            return nil
        }
        return content
    }
}

private struct LegalMarkdownContent: View {
    let content: String

    var body: some View {
        Markdown(content)
            .markdownTextStyle {
                FontSize(FawxTypography.chatBodyPointSize)
                ForegroundColor(Color.fawxText)
            }
            .markdownTextStyle(\.strong) {
                FontWeight(.semibold)
            }
            .markdownTextStyle(\.link) {
                ForegroundColor(Color.fawxAccent)
            }
            .textSelection(.enabled)
    }
}
