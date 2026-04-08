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

    func testSkillSettingsFieldValidateRequiresValueWhenMarkedRequired() {
        let field = SkillSettingsField(
            key: "api_key",
            label: "API Key",
            fieldType: .secret,
            placeholder: nil,
            helpText: nil,
            required: true,
            minLength: nil,
            pattern: nil
        )

        XCTAssertEqual(field.validate(nil), "API Key is required.")
        XCTAssertEqual(field.validate(""), "API Key is required.")
    }

    func testSkillSettingsFieldValidateChecksBooleanStrings() {
        let field = SkillSettingsField(
            key: "safesearch",
            label: "Safe Search",
            fieldType: .boolean,
            placeholder: nil,
            helpText: nil,
            required: false,
            minLength: nil,
            pattern: nil
        )

        XCTAssertNil(field.validate("true"))
        XCTAssertNil(field.validate("false"))
        XCTAssertEqual(
            field.validate("yes"),
            "Safe Search must be either true or false."
        )
    }
}
