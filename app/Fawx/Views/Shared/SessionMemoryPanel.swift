import Observation
import SwiftUI

struct SessionMemoryPanel: View {
    @Bindable var appState: AppState
    let session: Session
    let dismissAction: () -> Void

    @State private var draft = SessionMemory()
    @State private var savedMemory = SessionMemory()
    @State private var didLoadSnapshot = false
    @State private var isLoading = false
    @State private var isSaving = false
    @State private var isBackendUnsupported = false
    @State private var statusKind: ConnectionTestKind = .idle
    @State private var statusMessage: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    summaryCard
                    overviewCard
                    decisionsCard
                    filesCard
                    customContextCard
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .navigationTitle("Session Memory")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Reload") {
                        Task {
                            await loadMemory()
                        }
                    }
                    .disabled(isLoading || isSaving)
                }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                footer
            }
        }
        .frame(minWidth: 540, minHeight: 620)
        .task(id: session.id) {
            await loadMemory()
        }
    }

    private var summaryCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text(session.displayTitle)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text("Review and edit the durable context Fawx carries forward for this session.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                infoRow(label: "Session", value: session.id)
                infoRow(label: "Last updated", value: lastUpdatedLabel)

                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
                        Text("Memory budget")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                            .frame(width: 92, alignment: .leading)

                        Text("\(sanitizedDraft.estimatedTokens) / \(SessionMemory.maxTokens) tokens")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(tokenUsageColor)
                    }

                    ProgressView(
                        value: min(Double(sanitizedDraft.estimatedTokens), Double(SessionMemory.maxTokens)),
                        total: Double(SessionMemory.maxTokens)
                    )
                    .tint(tokenUsageColor)
                    .padding(.leading, 92 + FawxSpacing.paddingSM)
                }
            }

            if isLoading {
                ProgressView("Loading session memory...")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            if let validationMessage {
                SetupStatusMessageView(kind: .failure, message: validationMessage)
            }

            SetupStatusMessageView(kind: statusKind, message: statusMessage)
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private var overviewCard: some View {
        memoryCard(
            title: "Overview",
            subtitle: "Keep the high-level project and current state concise so compaction can preserve the essentials."
        ) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                memoryField(
                    title: "Project",
                    placeholder: "What is this session about?",
                    text: Binding(
                        get: { draft.project ?? "" },
                        set: { draft.project = $0 }
                    )
                )

                memoryField(
                    title: "Current State",
                    placeholder: "What is the latest state of work?",
                    text: Binding(
                        get: { draft.currentState ?? "" },
                        set: { draft.currentState = $0 }
                    ),
                    axis: .vertical
                )
            }
        }
    }

    private var decisionsCard: some View {
        memoryCard(
            title: "Key Decisions",
            subtitle: "\(sanitizedDraft.keyDecisions.count) / \(SessionMemory.maxItems) important decisions"
        ) {
            SessionMemoryListEditor(
                title: "Key Decisions",
                itemLabel: "Decision",
                placeholder: "Capture an important decision or constraint",
                items: $draft.keyDecisions,
                addButtonTitle: "Add Decision",
                isDisabled: isEditorDisabled
            )
        }
    }

    private var filesCard: some View {
        memoryCard(
            title: "Active Files",
            subtitle: "Track the files this session is actively touching."
        ) {
            SessionMemoryListEditor(
                title: "Active Files",
                itemLabel: "File",
                placeholder: "app/Fawx/Views/Shared/SessionMemoryPanel.swift",
                items: $draft.activeFiles,
                addButtonTitle: "Add File",
                isDisabled: isEditorDisabled
            )
        }
    }

    private var customContextCard: some View {
        memoryCard(
            title: "Custom Context",
            subtitle: "\(sanitizedDraft.customContext.count) / \(SessionMemory.maxItems) custom reminders"
        ) {
            SessionMemoryListEditor(
                title: "Custom Context",
                itemLabel: "Context",
                placeholder: "Anything else Fawx should remember",
                items: $draft.customContext,
                addButtonTitle: "Add Context",
                isDisabled: isEditorDisabled
            )
        }
    }

    private var footer: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            Divider()
                .opacity(0.5)

            HStack(spacing: FawxSpacing.paddingMD) {
                Button("Cancel", action: dismissAction)
                    .buttonStyle(.bordered)
                    .disabled(isSaving)

                Spacer(minLength: 0)

                Button(isSaving ? "Saving..." : "Save") {
                    Task {
                        await saveMemory()
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!canSave)
            }
            .padding(.horizontal, FawxSpacing.paddingLG)
            .padding(.top, FawxSpacing.paddingSM)
            .padding(.bottom, FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground)
    }

    private var sanitizedDraft: SessionMemory {
        draft.sanitizedForSaving
    }

    private var isEditorDisabled: Bool {
        isLoading || isSaving || isBackendUnsupported || !didLoadSnapshot
    }

    private var canSave: Bool {
        didLoadSnapshot
            && !isEditorDisabled
            && validationMessage == nil
            && sanitizedDraft != savedMemory.sanitizedForSaving
    }

    private var validationMessage: String? {
        if sanitizedDraft.keyDecisions.count > SessionMemory.maxItems {
            return "Keep key decisions to \(SessionMemory.maxItems) items or fewer."
        }

        if sanitizedDraft.customContext.count > SessionMemory.maxItems {
            return "Keep custom context to \(SessionMemory.maxItems) items or fewer."
        }

        if sanitizedDraft.estimatedTokens > SessionMemory.maxTokens {
            return "Session memory exceeds the \(SessionMemory.maxTokens)-token cap."
        }

        return nil
    }

    private var tokenUsageColor: Color {
        let ratio = Double(sanitizedDraft.estimatedTokens) / Double(SessionMemory.maxTokens)
        switch ratio {
        case ..<0.6:
            return .fawxSuccess
        case ..<0.85:
            return .fawxWarning
        default:
            return .fawxError
        }
    }

    private var lastUpdatedLabel: String {
        let timestamp = didLoadSnapshot ? savedMemory.lastUpdated : draft.lastUpdated
        guard timestamp > 0 else {
            return isBackendUnsupported ? "Unavailable on this server" : "Not saved yet"
        }

        return relativeTimestampString(timestamp)
    }

    private func loadMemory() async {
        isLoading = true
        defer { isLoading = false }

        do {
            let memory = try await appState.client.sessionMemory(id: session.id)
            draft = memory
            savedMemory = memory
            didLoadSnapshot = true
            isBackendUnsupported = false
            statusKind = .idle
            statusMessage = nil
        } catch {
            let failure = sessionMemoryFailure(for: error)
            isBackendUnsupported = failure.isUnsupported
            didLoadSnapshot = false
            draft = SessionMemory()
            savedMemory = SessionMemory()
            statusKind = failure.kind
            statusMessage = failure.message

            if !failure.isUnsupported {
                await appState.noteRecoverableRequestFailure(error)
            }
        }
    }

    private func saveMemory() async {
        guard validationMessage == nil else {
            statusKind = .failure
            statusMessage = validationMessage
            return
        }

        guard didLoadSnapshot, !isBackendUnsupported else {
            return
        }

        isSaving = true
        defer { isSaving = false }

        do {
            let saved = try await appState.client.updateSessionMemory(
                id: session.id,
                memory: sanitizedDraft
            )
            draft = saved
            savedMemory = saved
            didLoadSnapshot = true
            statusKind = .success
            statusMessage = "Session memory saved."
            appState.showToast(message: "Session memory saved.", style: .success)
            dismissAction()
        } catch {
            let failure = sessionMemoryFailure(for: error)
            isBackendUnsupported = failure.isUnsupported
            statusKind = failure.kind
            statusMessage = failure.message

            if !failure.isUnsupported {
                await appState.noteRecoverableRequestFailure(error)
            }
        }
    }

    private func sessionMemoryFailure(
        for error: Error
    ) -> (kind: ConnectionTestKind, message: String, isUnsupported: Bool) {
        if let apiError = error as? APIError, apiError.statusCode == 404 {
            return (
                .warning,
                "Session memory needs the newer backend memory API. Merge or deploy the backend support first.",
                true
            )
        }

        return (.failure, error.localizedDescription, false)
    }

    private func memoryCard<Content: View>(
        title: String,
        subtitle: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text(title)
                    .font(FawxTypography.heading2)
                    .foregroundStyle(Color.fawxText)

                Text(subtitle)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            content()
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func memoryField(
        title: String,
        placeholder: String,
        text: Binding<String>,
        axis: Axis = .horizontal
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            TextField(placeholder, text: text, axis: axis)
                .font(FawxTypography.chatBody)
                .textFieldStyle(.plain)
                .padding(FawxSpacing.paddingMD)
                .background(Color.fawxBackground)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay {
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color.fawxBorder, lineWidth: 1)
                }
                .disabled(isEditorDisabled)
        }
    }

    @ViewBuilder
    private func infoRow(label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
            Text(label)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 92, alignment: .leading)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .textSelection(.enabled)
        }
    }
}

private struct SessionMemoryListEditor: View {
    let title: String
    let itemLabel: String
    let placeholder: String
    @Binding var items: [String]
    let addButtonTitle: String
    let isDisabled: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if items.isEmpty {
                Text("No \(title.lowercased()) yet.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                ForEach(Array(items.indices), id: \.self) { index in
                    HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                        Text("\(itemLabel) \(index + 1)")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                            .frame(width: 72, alignment: .leading)

                        TextField(
                            placeholder,
                            text: Binding(
                                get: { items[index] },
                                set: { items[index] = $0 }
                            ),
                            axis: .vertical
                        )
                        .font(FawxTypography.chatBody)
                        .textFieldStyle(.plain)
                        .padding(FawxSpacing.paddingMD)
                        .background(Color.fawxBackground)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .overlay {
                            RoundedRectangle(cornerRadius: 12)
                                .stroke(Color.fawxBorder, lineWidth: 1)
                        }
                        .disabled(isDisabled)

                        Button {
                            items.remove(at: index)
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(Color.fawxTextSecondary)
                        }
                        .buttonStyle(.plain)
                        .disabled(isDisabled)
                        .accessibilityLabel("Delete \(itemLabel.lowercased()) \(index + 1)")
                    }
                }
            }

            Button(addButtonTitle) {
                items.append("")
            }
            .buttonStyle(.bordered)
            .disabled(isDisabled)
        }
    }
}
