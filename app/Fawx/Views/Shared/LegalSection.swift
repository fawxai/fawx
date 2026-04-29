import SwiftUI

enum LegalDocument: String, CaseIterable, Hashable, Identifiable {
    case privacyPolicy = "privacy-policy"
    case termsOfService = "terms-of-service"
    case eula = "eula"

    var id: Self { self }

    var title: String {
        switch self {
        case .privacyPolicy:
            "Privacy Policy"
        case .termsOfService:
            "Terms of Service"
        case .eula:
            "EULA"
        }
    }

    var resourceName: String {
        rawValue
    }
}

struct LegalSection: View {
    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            ForEach(LegalDocument.allCases) { document in
                LegalDocumentLink(document: document)
            }
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay(roundedBorder)
    }

    private var roundedBorder: some View {
        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
            .stroke(Color.fawxBorder, lineWidth: 1)
    }
}

private struct LegalDocumentLink: View {
    let document: LegalDocument

    var body: some View {
        NavigationLink {
            LegalDocumentView(title: document.title, resourceName: document.resourceName)
        } label: {
            HStack(spacing: FawxSpacing.paddingMD) {
                Text(document.title)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: 0)

                Image(systemName: "chevron.right")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
