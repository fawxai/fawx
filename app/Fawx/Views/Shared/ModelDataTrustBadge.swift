import SwiftUI

enum ModelDataTrustBadgeStyle {
    case compact
    case titled
}

struct ModelDataTrustBadge: View {
    let trust: ModelDataTrust
    let style: ModelDataTrustBadgeStyle

    init(trust: ModelDataTrust, style: ModelDataTrustBadgeStyle = .compact) {
        self.trust = trust
        self.style = style
    }

    var body: some View {
        Label {
            Text(title)
        } icon: {
            Image(systemName: systemImage)
        }
        .font(FawxTypography.status)
        .foregroundStyle(tint)
        .labelStyle(.titleAndIcon)
        .lineLimit(1)
        .help(trust.detail)
        .accessibilityLabel("\(trust.title): \(trust.detail)")
    }

    private var title: String {
        switch style {
        case .compact:
            return trust.shortTitle
        case .titled:
            return trust.title
        }
    }

    private var systemImage: String {
        switch trust {
        case .providerDirect:
            return "lock.shield"
        case .knownRouter:
            return "point.3.connected.trianglepath.dotted"
        case .freeOrUntrusted:
            return "exclamationmark.triangle"
        case .unknown:
            return "questionmark.diamond"
        }
    }

    private var tint: Color {
        switch trust {
        case .providerDirect:
            return .fawxSuccess
        case .knownRouter:
            return .fawxAccent
        case .freeOrUntrusted:
            return .fawxError
        case .unknown:
            return .fawxTextSecondary
        }
    }
}

enum ModelProviderBadgeStyle {
    case compact
    case titled
}

struct ModelProviderBadge: View {
    let provider: String
    let style: ModelProviderBadgeStyle

    init(provider: String, style: ModelProviderBadgeStyle = .compact) {
        self.provider = provider
        self.style = style
    }

    var body: some View {
        Label {
            Text(title)
        } icon: {
            Image(systemName: systemImage)
        }
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .labelStyle(.titleAndIcon)
        .lineLimit(1)
        .help("Provider: \(providerName)")
        .accessibilityLabel("Provider: \(providerName)")
    }

    private var title: String {
        switch style {
        case .compact:
            return providerName
        case .titled:
            return "Provider: \(providerName)"
        }
    }

    private var providerName: String {
        displayProviderName(provider)
    }

    private var systemImage: String {
        switch ProviderBrand.resolve(provider) {
        case .anthropic:
            return "sparkles"
        case .openai:
            return "circle.hexagongrid"
        case .openrouter:
            return "point.3.connected.trianglepath.dotted"
        case .fireworks:
            return "sparkle.magnifyingglass"
        case .google:
            return "g.circle"
        case nil:
            return "server.rack"
        }
    }
}
