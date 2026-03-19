import SwiftUI

struct SandboxStatusCard: View {
    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("OS Sandbox")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                Label("Not available", systemImage: "lock.slash")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text("OS-level enforcement requires Linux 5.13+ with Landlock support.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)

                Text("Your security is enforced at the application level via capability mode.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
            .padding(FawxSpacing.paddingMD)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.fawxBorder, lineWidth: 1)
            }
        }
    }
}
