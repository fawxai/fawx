import SwiftUI

struct ThreadRenameSheet: View {
    private enum Layout {
        static let minimumWidth: CGFloat = 360
        static let idealWidth: CGFloat = 420
    }

    let initialTitle: String
    let onCancel: () -> Void
    let onSave: (String) -> Void

    @State private var title: String
    @FocusState private var isTitleFieldFocused: Bool

    init(
        initialTitle: String,
        onCancel: @escaping () -> Void,
        onSave: @escaping (String) -> Void
    ) {
        self.initialTitle = initialTitle
        self.onCancel = onCancel
        self.onSave = onSave
        _title = State(initialValue: initialTitle)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            Text("Rename Thread")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text("Choose a title that will be used throughout the workspace shell.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .fixedSize(horizontal: false, vertical: true)

            TextField("Thread title", text: $title)
                .textFieldStyle(.roundedBorder)
                .focused($isTitleFieldFocused)

            HStack(spacing: FawxSpacing.paddingSM) {
                Spacer(minLength: 0)

                Button("Cancel", action: onCancel)
                    .buttonStyle(.bordered)

                Button("Save") {
                    onSave(title)
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
            }
        }
        .frame(minWidth: Layout.minimumWidth, idealWidth: Layout.idealWidth)
        .padding(FawxSpacing.paddingXL)
        .background(Color.fawxBackground)
        .onAppear {
            isTitleFieldFocused = true
        }
    }
}
