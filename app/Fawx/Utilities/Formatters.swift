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

func displayThinkingLevel(_ level: ThinkingLevel?) -> String {
    level?.rawValue.capitalized ?? "—"
}

func modelMetadataSummary(_ model: ModelInfo) -> String {
    let provider = humanReadableSettingToken(model.provider)
    let authMethod = humanReadableSettingToken(model.authMethod)
    return "\(provider) · \(authMethod)"
}

func permissionPresetLabel(_ rawValue: String?) -> String {
    switch rawValue?.lowercased() {
    case "power":
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
        normalized = "http://" + normalized
    }

    guard var components = URLComponents(string: normalized) else {
        return nil
    }

    components.scheme = components.scheme?.lowercased()
    components.host = components.host?.lowercased()

    guard let host = components.host, !host.isEmpty else {
        return nil
    }

    if components.path == "/" {
        components.path = ""
    }

    return components.string
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
