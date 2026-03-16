import SwiftUI

enum IOSRootSection {
    case sessions
    case skills
    case fleet
    case experiments
    case git
    case settings
}

struct SectionMenuButton: View {
    let disabledSection: IOSRootSection?
    let showSessions: () -> Void
    let showSkills: () -> Void
    let showFleet: () -> Void
    let showExperiments: () -> Void
    let showGit: () -> Void
    let showSettings: () -> Void

    var body: some View {
        Menu {
            sectionButton(
                title: "Sessions",
                systemImage: "list.bullet",
                section: .sessions,
                action: showSessions
            )

            sectionButton(
                title: "Skills",
                systemImage: "puzzlepiece.extension",
                section: .skills,
                action: showSkills
            )

            sectionButton(
                title: "Fleet",
                systemImage: "point.3.connected.trianglepath.dotted",
                section: .fleet,
                action: showFleet
            )

            sectionButton(
                title: "Experiments",
                systemImage: "waveform.path.ecg.rectangle",
                section: .experiments,
                action: showExperiments
            )

            sectionButton(
                title: "Git",
                systemImage: "arrow.trianglehead.branch",
                section: .git,
                action: showGit
            )

            sectionButton(
                title: "Settings",
                systemImage: "gear",
                section: .settings,
                action: showSettings
            )
        } label: {
            Image(systemName: "line.3.horizontal")
        }
        .accessibilityIdentifier("sectionMenuButton")
    }

    @ViewBuilder
    private func sectionButton(
        title: String,
        systemImage: String,
        section: IOSRootSection,
        action: @escaping () -> Void
    ) -> some View {
        if disabledSection == section {
            Button(title, systemImage: systemImage, action: {})
                .disabled(true)
        } else {
            Button(title, systemImage: systemImage, action: action)
        }
    }
}
