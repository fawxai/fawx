import SwiftUI

struct ModelSelectionList: View {
    let models: [ModelInfo]
    let selectedModelID: String?
    let disableSelection: Bool
    let selectModel: (String) -> Void
    @State private var searchText = ""
    @State private var selectedProviderID = ModelSelectionCatalog.allProvidersID
    @State private var selectedCatalogScope: ModelSelectionScope = .recommended

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
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxBackground)
        .onAppear {
            normalizeProviderSelection()
        }
        .onChange(of: availableProviderIDs) { _, _ in
            normalizeProviderSelection()
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
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
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
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
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
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
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

    private var providerOptions: [ModelSelectionProviderOption] {
        ModelSelectionCatalog.providerOptions(for: models)
    }

    private var availableProviderIDs: [String] {
        providerOptions.map(\.id)
    }

    private var filteredSections: [ModelSelectionSection] {
        ModelSelectionCatalog.filteredSections(
            models: models,
            scope: selectedCatalogScope,
            providerFilterID: selectedProviderID,
            query: searchText
        )
    }

    private var showsCatalogScopeFilter: Bool {
        models.contains(where: { !$0.recommended })
    }

    private var emptyResultsMessage: String {
        let trimmedQuery = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        let selectedProviderTitle = providerOptions.first(where: { $0.id == selectedProviderID })?.title

        if selectedCatalogScope == .recommended {
            if !trimmedQuery.isEmpty {
                return "Try a shorter query, switch providers, or show all models."
            }
            if selectedProviderID != ModelSelectionCatalog.allProvidersID {
                return "No recommended models are available for \(selectedProviderTitle ?? "this provider")."
            }
            return "No recommended models are available. Show all models to browse the full catalog."
        }
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
                    Button {
                        guard !disableSelection else {
                            return
                        }
                        selectModel(model.modelID)
                    } label: {
                        ModelSelectionRow(
                            model: model,
                            isSelected: model.modelID == selectedModelID
                        )
                    }
                    .buttonStyle(.plain)
                    .disabled(disableSelection)
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
}

private struct ModelSelectionRow: View {
    let model: ModelInfo
    let isSelected: Bool

    var body: some View {
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

                Text(modelMetadataSummary(model))
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            if isSelected {
                Image(systemName: "checkmark")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.fawxAccent)
                    .padding(.top, 2)
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(isSelected ? Color.fawxAccent.opacity(0.08) : Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(
                    isSelected ? Color.fawxAccent.opacity(0.35) : Color.fawxBorder,
                    lineWidth: 1
                )
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
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
