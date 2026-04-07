import Foundation

func relativeTimestampString(_ epochSeconds: Int) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(epochSeconds))
    let formatter = RelativeDateTimeFormatter()
    formatter.unitsStyle = .short
    return formatter.localizedString(for: date, relativeTo: .now)
}

func timeString(_ epochSeconds: Int) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(epochSeconds))
    let formatter = DateFormatter()
    formatter.timeStyle = .short
    formatter.dateStyle = .none
    return formatter.string(from: date)
}

func uptimeString(_ seconds: Int) -> String {
    guard seconds > 0 else {
        return "0m"
    }

    let days = seconds / 86_400
    let hours = (seconds % 86_400) / 3_600
    let minutes = (seconds % 3_600) / 60

    if days > 0 {
        return "\(days)d \(hours)h"
    }
    if hours > 0 {
        return "\(hours)h \(minutes)m"
    }
    return "\(max(minutes, 1))m"
}

func abbreviateModelName(_ modelID: String) -> String {
    let withoutProvider: String
    if let slashIndex = modelID.lastIndex(of: "/") {
        withoutProvider = String(modelID[modelID.index(after: slashIndex)...])
    } else {
        withoutProvider = modelID
    }

    let trimmed = withoutProvider.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? modelID : trimmed
}

func compactModelName(_ modelID: String, limit: Int? = nil) -> String {
    let abbreviated = abbreviateModelName(modelID)
    guard let limit, abbreviated.count > limit else {
        return abbreviated
    }

    guard limit > 6 else {
        let endIndex = abbreviated.index(abbreviated.startIndex, offsetBy: max(1, limit - 1))
        return String(abbreviated[..<endIndex]) + "…"
    }

    let visibleCharacters = limit - 1
    let trailingCount = max(4, visibleCharacters / 2)
    let leadingCount = max(3, visibleCharacters - trailingCount)
    let startIndex = abbreviated.index(abbreviated.startIndex, offsetBy: leadingCount)
    let endIndex = abbreviated.index(abbreviated.endIndex, offsetBy: -trailingCount)
    return String(abbreviated[..<startIndex]) + "…" + String(abbreviated[endIndex...])
}

func displayModelName(_ model: ModelInfo) -> String {
    if let displayName = model.displayName?.trimmingCharacters(in: .whitespacesAndNewlines),
       !displayName.isEmpty {
        return displayName
    }
    return abbreviateModelName(model.modelID)
}

func displayThinkingLevel(_ level: ThinkingLevel?, modelID: String? = nil) -> String {
    guard let level else {
        return "—"
    }

    if level == .adaptive, usesAdaptiveDefaultThinkingLabel(for: modelID) {
        return "Adaptive (default)"
    }

    return level.displayName
}

func displayProviderName(_ provider: String) -> String {
    if let knownProvider = ProviderBrand.resolve(provider) {
        return knownProvider.companyName
    }
    return humanReadableSettingToken(provider)
}

func displayAuthMethodName(_ authMethod: String) -> String {
    switch authMethod.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
    case "api_key":
        return "API Key"
    case "setup_token":
        return "Setup Token"
    case "oauth":
        return "OAuth"
    default:
        return humanReadableSettingToken(authMethod)
    }
}

func modelMetadataSummary(_ model: ModelInfo) -> String {
    let provider = displayProviderName(model.provider)
    let authMethod = displayAuthMethodName(model.authMethod)
    if model.recommended {
        return "\(provider) · \(authMethod)"
    }
    return "\(provider) · \(authMethod) · Not Recommended"
}

// Claude 4.6 models default to adaptive thinking today, so they get the
// explicit label. Update this list if Anthropic adds new 4.6-style variants.
private func usesAdaptiveDefaultThinkingLabel(for modelID: String?) -> Bool {
    guard let modelID else {
        return false
    }

    let normalizedModelID = abbreviateModelName(modelID).lowercased()
    return normalizedModelID.hasPrefix("claude-opus-4-6")
        || normalizedModelID.hasPrefix("claude-sonnet-4-6")
}

func permissionPresetLabel(_ rawValue: String?) -> String {
    switch rawValue?.lowercased() {
    case "safe":
        return "Safe"
    case "power", "power-user", "power_user":
        return "Power User"
    case "cautious":
        return "Cautious"
    case "experimental":
        return "Experimental"
    case "custom":
        return "Custom"
    default:
        return "Power User"
    }
}

func canonicalizeServerURL(_ input: String) -> String? {
    var normalized = input.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalized.isEmpty else {
        return nil
    }

    if let range = normalized.range(of: "://") {
        let remaining = normalized[range.upperBound...]
        if remaining.range(of: "://") != nil {
            return nil
        }
    }

    if !normalized.contains("://") {
        guard let host = inferredHost(from: normalized) else {
            return nil
        }
        normalized = "\(defaultServerScheme(for: host))://" + normalized
    }

    guard var components = URLComponents(string: normalized) else {
        return nil
    }

    components.scheme = components.scheme?.lowercased()
    components.host = components.host?.lowercased()

    guard let host = components.host, !host.isEmpty else {
        return nil
    }

    guard let scheme = components.scheme, supportedServerSchemes.contains(scheme) else {
        return nil
    }

    if scheme == "http", !allowsInsecureServerTransport(for: host) {
        return nil
    }

    components.path = ""
    components.query = nil
    components.fragment = nil

    return components.string
}

func serverURLValidationMessage(_ input: String) -> String {
    let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        return "Enter a valid server URL."
    }

    let candidate = trimmed.contains("://") ? trimmed : "https://" + trimmed
    guard
        let components = URLComponents(string: candidate),
        let scheme = components.scheme?.lowercased(),
        let host = components.host?.lowercased(),
        !host.isEmpty
    else {
        return "Enter a valid server URL."
    }

    guard supportedServerSchemes.contains(scheme) else {
        return "Enter a valid server URL."
    }

    if scheme == "http", !allowsInsecureServerTransport(for: host) {
        return "Use HTTPS for remote servers. Cleartext HTTP is only allowed for localhost or local-network hosts."
    }

    return "Enter a valid server URL."
}

private let supportedServerSchemes: Set<String> = ["http", "https"]

private func inferredHost(from value: String) -> String? {
    let hostPortSegment = value
        .split(separator: "/", maxSplits: 1, omittingEmptySubsequences: true)
        .first
        .map(String.init) ?? value

    if hostPortSegment.hasPrefix("["),
       let closingBracket = hostPortSegment.firstIndex(of: "]") {
        let host = hostPortSegment[hostPortSegment.index(after: hostPortSegment.startIndex) ..< closingBracket]
        return host.isEmpty ? nil : String(host).lowercased()
    }

    let components = hostPortSegment.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
    guard let host = components.first?.trimmingCharacters(in: .whitespacesAndNewlines), !host.isEmpty else {
        return nil
    }

    return host.lowercased()
}

private func defaultServerScheme(for host: String) -> String {
    prefersLoopbackHTTPDefault(for: host) ? "http" : "https"
}

private func allowsInsecureServerTransport(for host: String) -> Bool {
    let normalizedHost = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]")).lowercased()

    if normalizedHost == "localhost" || normalizedHost == "::1" || normalizedHost == "127.0.0.1" {
        return true
    }

    if normalizedHost.hasSuffix(".local") {
        return true
    }

    if let ipv4 = IPv4Address(normalizedHost) {
        return ipv4.isPrivate || ipv4.isLoopback || ipv4.isLinkLocal
    }

    return false
}

private func prefersLoopbackHTTPDefault(for host: String) -> Bool {
    let normalizedHost = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]")).lowercased()

    if normalizedHost == "localhost" || normalizedHost == "::1" || normalizedHost == "127.0.0.1" {
        return true
    }

    if let ipv4 = IPv4Address(normalizedHost) {
        return ipv4.isLoopback
    }

    return false
}

private struct IPv4Address {
    let octets: [UInt8]

    init?(_ rawValue: String) {
        let parts = rawValue.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else {
            return nil
        }

        var octets: [UInt8] = []
        octets.reserveCapacity(4)

        for part in parts {
            guard let value = UInt8(part) else {
                return nil
            }
            octets.append(value)
        }

        self.octets = octets
    }

    var isLoopback: Bool {
        octets.first == 127
    }

    var isLinkLocal: Bool {
        octets[0] == 169 && octets[1] == 254
    }

    var isPrivate: Bool {
        switch (octets[0], octets[1]) {
        case (10, _):
            true
        case (172, 16 ... 31):
            true
        case (192, 168):
            true
        default:
            false
        }
    }
}

private func humanReadableSettingToken(_ rawValue: String) -> String {
    let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        return "Unknown"
    }

    return trimmed
        .replacingOccurrences(of: "_", with: " ")
        .replacingOccurrences(of: "-", with: " ")
        .localizedCapitalized
}
