#if os(macOS)
import SwiftUI

enum MenuBarStatus: String {
    case active
    case connecting
    case error
    case stopped
}

struct MenuBarStatusSnapshot: Equatable {
    let title: String
    let detail: String
    let color: Color
    let status: MenuBarStatus
}

struct MenuBarView: View {
    let snapshot: MenuBarStatusSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Circle()
                    .fill(snapshot.color)
                    .frame(width: 8, height: 8)

                Text(snapshot.title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.fawxText)
            }

            Text(snapshot.detail)
                .font(.system(size: 11))
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(2)
        }
        .padding(10)
        .frame(width: 220, alignment: .leading)
        .background(Color.fawxBackground)
    }
}
#endif
