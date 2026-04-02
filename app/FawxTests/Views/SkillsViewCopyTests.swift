import XCTest
@testable import Fawx

final class SkillsViewCopyTests: XCTestCase {
    func testLoadedSkillsCopyUsesServerLoadedLanguage() {
        let copy = LoadedSkillsCopy.serverLoaded

        XCTAssertEqual(copy.sectionTitle, "Loaded")
        XCTAssertEqual(copy.subtitle, "Loaded on server")
        XCTAssertEqual(copy.searchPrompt, "Search loaded skills")
        XCTAssertEqual(copy.emptyTitle, "No skills loaded")
        XCTAssertTrue(copy.emptyMessage.contains("/v1/skills"))
        XCTAssertEqual(copy.statusLabel, "Loaded")
    }

    func testLoadedSkillsSectionUsesServerLoadedCopy() {
        XCTAssertEqual(SkillsSection.loadedOnServer.title, LoadedSkillsCopy.serverLoaded.sectionTitle)
        XCTAssertEqual(SkillsSection.loadedOnServer.subtitle, LoadedSkillsCopy.serverLoaded.subtitle)
    }
}
