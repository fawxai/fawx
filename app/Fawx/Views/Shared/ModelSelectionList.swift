import SwiftUI

struct ModelSelectionList: View {
    let models: [ModelInfo]
    let selectedModelID: String?
    let favoriteModelIDs: Set<String>
    let disableSelection: Bool
    let selectModel: (String) -> Void
    let toggleFavorite: (String) -> Void
    let contentInsets: EdgeInsets
    @State private var searchText = ""
    @State private var selectedProviderID = ModelSelectionCatalog.allProvidersID
    @State private var selectedCatalogScope: ModelSelectionScope = .recommended
    @State private var selectedDataTrust: ModelDataTrust?

    init(
        models: [ModelInfo],
        selectedModelID: String?,
        favoriteModelIDs: Set<String>,
        disableSelection: Bool,
        selectModel: @escaping (String) -> Void,
        toggleFavorite: @escaping (String) -> Void,
        contentInsets: EdgeInsets = EdgeInsets(
            top: FawxSpacing.paddingLG,
            leading: FawxSpacing.paddingLG,
            bottom: FawxSpacing.paddingLG,
            trailing: FawxSpacing.paddingLG
        )
    ) {
        self.models = models
        self.selectedModelID = selectedModelID
        self.favoriteModelIDs = favoriteModelIDs
        self.disableSelection = disableSelection
        self.selectModel = selectModel
        self.toggleFavorite = toggleFavorite
        self.contentInsets = contentInsets
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            if models.isEmpty {
                unavailableState
            } else {
                searchField

                if providerOptions.count > 2 {
                    providerFilterBar
                }

                if showsCatalogScopeFilter {
                    catalogScopeFilterBar
                }

                if showsDataTrustFilter {
                    dataTrustFilterBar
                }

                if filteredSections.isEmpty {
                    emptyResultsState
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                            ForEach(filteredSections) { section in
                                providerSection(section)
                            }
                        }
                        .padding(.top, FawxSpacing.paddingXS)
                    }
                }
            }
        }
        .padding(contentInsets)
        .fawxSurface(.page)
        .onAppear {
            normalizeProviderSelection()
        }
        .onChange(of: availableProviderIDs) { _, _ in
            normalizeProviderSelection()
        }
        .onChange(of: availableDataTrusts) { _, _ in
            normalizeDataTrustSelection()
        }
    }

    private var unavailableState: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("No models available")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text("Connect to a server and refresh settings to load the available models.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .fawxSurface(.callout)
    }

    private var emptyResultsState: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text("No matching models")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text(emptyResultsMessage)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .fawxSurface(.callout)
    }

    private var searchField: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(Color.fawxTextSecondary)

            TextField("Search models", text: $searchText)
                .textFieldStyle(.plain)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .accessibilityIdentifier("modelSearchField")

            if !searchText.isEmpty {
                Button {
                    searchText = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Color.fawxTextSecondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear model search")
            }
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .fawxSurface(.field)
    }

    private var providerFilterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: FawxSpacing.paddingSM) {
                ForEach(providerOptions) { option in
                    ProviderFilterChip(
                        title: option.title,
                        isSelected: selectedProviderID == option.id,
                        action: {
                            selectedProviderID = option.id
                        }
                    )
                    .accessibilityIdentifier("modelProviderFilter_\(option.id)")
                }
            }
        }
    }

    private var catalogScopeFilterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: FawxSpacing.paddingSM) {
                ForEach(ModelSelectionScope.allCases) { scope in
                    ProviderFilterChip(
                        title: scope.title,
                        isSelected: selectedCatalogScope == scope,
                        action: {
                            selectedCatalogScope = scope
                        }
                    )
                    .accessibilityIdentifier("modelCatalogScope_\(scope.rawValue)")
                }
            }
        }
    }

    private var dataTrustFilterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: FawxSpacing.paddingSM) {
                ProviderFilterChip(
                    title: "All Routes",
                    isSelected: selectedDataTrust == nil,
                    action: {
                        selectedDataTrust = nil
                    }
                )
                .accessibilityIdentifier("modelDataTrustFilter_all")

                ForEach(availableDataTrusts, id: \.self) { trust in
                    ProviderFilterChip(
                        title: trust.title,
                        isSelected: selectedDataTrust == trust,
                        action: {
                            selectedDataTrust = trust
                        }
                    )
                    .accessibilityIdentifier("modelDataTrustFilter_\(trust.rawValue)")
                }
            }
        }
    }

    private var providerOptions: [ModelSelectionProviderOption] {
        ModelSelectionCatalog.providerOptions(for: models)
    }

    private var availableProviderIDs: [String] {
        providerOptions.map(\.id)
    }

    private var availableDataTrusts: [ModelDataTrust] {
        let presentTrusts = Set(models.map(\.dataTrust))
        return ModelDataTrust.allCases.filter { presentTrusts.contains($0) }
    }

    private var filteredSections: [ModelSelectionSection] {
        ModelSelectionCatalog.filteredSections(
            models: models,
            scope: selectedCatalogScope,
            favoriteModelIDs: favoriteModelIDs,
            providerFilterID: selectedProviderID,
            query: searchText,
            dataTrustFilter: selectedDataTrust
        )
    }

    private var showsCatalogScopeFilter: Bool {
        true
    }

    private var showsDataTrustFilter: Bool {
        availableDataTrusts.count > 1
    }

    private var emptyResultsMessage: String {
        let trimmedQuery = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        let selectedProviderTitle = providerOptions.first(where: { $0.id == selectedProviderID })?.title

        switch selectedCatalogScope {
        case .recommended:
            if !trimmedQuery.isEmpty {
                return "Try a shorter query, switch providers, or show all models."
            }
            if selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No recommended models are available for \(selectedProviderTitle ?? "this provider")."
            }
            return "No recommended models are available. Show all models to browse the full catalog."
        case .favorites:
            if !trimmedQuery.isEmpty, selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No favorite \(selectedProviderTitle ?? "provider") models match \"\(trimmedQuery)\"."
            }
            if !trimmedQuery.isEmpty {
                return "No favorite models match \"\(trimmedQuery)\"."
            }
            if selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No favorite models are available for \(selectedProviderTitle ?? "this provider")."
            }
            if favoriteModelIDs.isEmpty {
                return "Star models from Recommended or All Models to pin them here."
            }
            return "Favorite models will appear here when they are available from the connected server."
        case .all:
            if !trimmedQuery.isEmpty, selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No \(selectedProviderTitle ?? "provider") models match \"\(trimmedQuery)\"."
            }
            if !trimmedQuery.isEmpty {
                return "Try a shorter query or switch providers."
            }
            if selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No models are available for \(selectedProviderTitle ?? "this provider")."
            }
            return "Connect to a server and refresh settings to load the available models."
        }
    }

    private func providerSection(_ section: ModelSelectionSection) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
                Text(section.title)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Text("\(section.models.count)")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, 4)
                    .background(Color.fawxSurface)
                    .clipShape(Capsule())
            }

            VStack(spacing: FawxSpacing.paddingSM) {
                ForEach(section.models) { model in
                    ModelSelectionRow(
                        model: model,
                        isSelected: model.modelID == selectedModelID,
                        isFavorite: favoriteModelIDs.contains(model.modelID),
                        disableSelection: disableSelection,
                        selectModel: {
                            selectModel(model.modelID)
                        },
                        toggleFavorite: {
                            toggleFavorite(model.modelID)
                        }
                    )
                }
            }
        }
    }

    private func normalizeProviderSelection() {
        guard availableProviderIDs.contains(selectedProviderID) else {
            selectedProviderID = ModelSelectionCatalog.allProvidersID
            return
        }
    }

    private func normalizeDataTrustSelection() {
        guard let selectedDataTrust else {
            return
        }
        if !availableDataTrusts.contains(selectedDataTrust) {
            self.selectedDataTrust = nil
        }
    }
}

private struct ModelSelectionRow: View {
    let model: ModelInfo
    let isSelected: Bool
    let isFavorite: Bool
    let disableSelection: Bool
    let selectModel: () -> Void
    let toggleFavorite: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
            Button {
                guard !disableSelection else {
                    return
                }
                selectModel()
            } label: {
                selectionContent
            }
            .buttonStyle(.plain)
            .disabled(disableSelection)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .accessibilityIdentifier("modelSelectionRow_\(model.modelID)")

            Button(action: toggleFavorite) {
                Image(systemName: isFavorite ? "star.fill" : "star")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(isFavorite ? Color.fawxAccent : Color.fawxTextSecondary)
                    .frame(width: 28, height: 28)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(favoriteAccessibilityLabel)
            .accessibilityIdentifier("modelFavoriteButton_\(model.modelID)")
            .help(isFavorite ? "Remove from favorites" : "Add to favorites")
        }
        .padding(FawxSpacing.paddingMD)
        .fawxRowChrome(isSelected: isSelected)
        .overlay {
            if isSelected {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .stroke(Color.fawxAccent.opacity(0.35), lineWidth: 1)
            }
        }
        .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private var selectionContent: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text(displayModelName(model))
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Color.fawxText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .multilineTextAlignment(.leading)
                    .lineLimit(2)

                if model.displayName != nil {
                    Text(compactModelName(model.modelID, limit: 40))
                        .font(.system(size: 12, weight: .regular, design: .monospaced))
                        .foregroundStyle(Color.fawxTextSecondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .multilineTextAlignment(.leading)
                        .lineLimit(1)
                }

                HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
                    Text(modelProviderMetadataSummary(model))
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .lineLimit(1)

                    ModelDataTrustBadge(trust: model.dataTrust)
                        .fixedSize(horizontal: true, vertical: false)
                }
            }

            if isSelected {
                Image(systemName: "checkmark")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.fawxAccent)
                    .padding(.top, 2)
            }
        }
    }

    private var favoriteAccessibilityLabel: String {
        if isFavorite {
            return "Remove \(displayModelName(model)) from favorites"
        }
        return "Add \(displayModelName(model)) to favorites"
    }
}

private struct ProviderFilterChip: View {
    let title: String
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(isSelected ? Color.fawxAccent : Color.fawxTextSecondary)
                .padding(.horizontal, FawxSpacing.paddingMD)
                .padding(.vertical, 7)
                .background(isSelected ? Color.fawxAccentSubtle : Color.fawxSurface)
                .overlay {
                    Capsule()
                        .stroke(isSelected ? Color.fawxAccent.opacity(0.35) : Color.fawxBorder, lineWidth: 1)
                }
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}
