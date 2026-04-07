import Foundation

struct ModelSelectionProviderOption: Identifiable, Hashable {
    let id: String
    let title: String
}

enum ModelSelectionScope: String, CaseIterable, Identifiable {
    case recommended
    case all

    var id: String { rawValue }

    var title: String {
        switch self {
        case .recommended:
            "Recommended"
        case .all:
            "All Models"
        }
    }
}

struct ModelSelectionSection: Identifiable, Equatable {
    let providerID: String
    let title: String
    let models: [ModelInfo]

    var id: String { providerID }
}

enum ModelSelectionCatalog {
    static let allProvidersID = "__all__"

    static func providerOptions(for models: [ModelInfo]) -> [ModelSelectionProviderOption] {
        let providerOptions = orderedProviderIDs(in: models).map { providerID in
            ModelSelectionProviderOption(
                id: providerID,
                title: displayProviderName(providerID)
            )
        }

        return [ModelSelectionProviderOption(id: allProvidersID, title: "All Providers")]
            + providerOptions
    }

    static func filteredSections(
        models: [ModelInfo],
        scope: ModelSelectionScope,
        providerFilterID: String,
        query: String
    ) -> [ModelSelectionSection] {
        let filteredModels = filterModels(
            models,
            scope: scope,
            providerFilterID: providerFilterID,
            query: query
        )

        return orderedProviderGroups(from: filteredModels).map { providerID, providerModels in
            ModelSelectionSection(
                providerID: providerID,
                title: displayProviderName(providerID),
                models: sortedModels(providerModels)
            )
        }
    }

    private static func filterModels(
        _ models: [ModelInfo],
        scope: ModelSelectionScope,
        providerFilterID: String,
        query: String
    ) -> [ModelInfo] {
        let trimmedQuery = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return models.filter { model in
            matchesProviderFilter(model, providerFilterID: providerFilterID)
                && matchesScopeFilter(model, scope: scope)
                && matchesQuery(model, query: trimmedQuery)
        }
    }

    private static func matchesProviderFilter(
        _ model: ModelInfo,
        providerFilterID: String
    ) -> Bool {
        providerFilterID == allProvidersID || model.provider == providerFilterID
    }

    private static func matchesScopeFilter(
        _ model: ModelInfo,
        scope: ModelSelectionScope
    ) -> Bool {
        scope == .all || model.recommended
    }

    private static func matchesQuery(_ model: ModelInfo, query: String) -> Bool {
        guard !query.isEmpty else {
            return true
        }

        return searchHaystack(for: model).contains(query.lowercased())
    }

    private static func searchHaystack(for model: ModelInfo) -> String {
        [
            model.modelID,
            abbreviateModelName(model.modelID),
            model.displayName ?? "",
            model.provider,
            displayProviderName(model.provider),
            model.authMethod,
            displayAuthMethodName(model.authMethod),
        ]
        .joined(separator: " ")
        .lowercased()
    }

    private static func orderedProviderIDs(in models: [ModelInfo]) -> [String] {
        orderedProviderGroups(from: models).map(\.0)
    }

    private static func orderedProviderGroups(
        from models: [ModelInfo]
    ) -> [(providerID: String, models: [ModelInfo])] {
        var providerOrder: [String] = []
        var groupedModels: [String: [ModelInfo]] = [:]

        for model in models {
            if groupedModels[model.provider] == nil {
                providerOrder.append(model.provider)
            }
            groupedModels[model.provider, default: []].append(model)
        }

        return providerOrder.compactMap { providerID in
            guard let providerModels = groupedModels[providerID] else {
                return nil
            }
            return (providerID, providerModels)
        }
    }

    private static func sortedModels(_ models: [ModelInfo]) -> [ModelInfo] {
        models.sorted { left, right in
            if left.recommended != right.recommended {
                return left.recommended && !right.recommended
            }

            return displayModelName(left).localizedCaseInsensitiveCompare(displayModelName(right))
                == .orderedAscending
        }
    }
}
