import XCTest
@testable import Fawx

@MainActor
final class SkillsViewModelTests: XCTestCase {
    func testIsLoadedOnServerMatchesSkillsReturnedByServer() {
        let appState = AppState(startLoadingPersistedState: false)
        let sut = SkillsViewModel(appState: appState)
        sut.skills = [
            SkillSummary(name: "weather", description: nil, tools: [], capabilities: []),
        ]

        XCTAssertTrue(
            sut.isLoadedOnServer(
                MarketplaceSkillSummary(
                    name: "weather",
                    title: "Weather",
                    description: "Weather tools",
                    publisher: "Fawx",
                    signed: true
                )
            )
        )
        XCTAssertFalse(
            sut.isLoadedOnServer(
                MarketplaceSkillSummary(
                    name: "github",
                    title: "GitHub",
                    description: "GitHub tools",
                    publisher: "Fawx",
                    signed: true
                )
            )
        )
    }
}
