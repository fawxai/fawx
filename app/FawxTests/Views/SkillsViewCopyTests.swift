import XCTest
@testable import Fawx

final class SkillsViewCopyTests: XCTestCase {
    func testLoadedSkillsCopyUsesServerLoadedLanguage() {
        let copy = LoadedSkillsCopy.serverLoaded

        XCTAssertEqual(copy.sectionTitle, "Installed")
        XCTAssertEqual(copy.subtitle, "Installed skills")
        XCTAssertEqual(copy.searchPrompt, "Search installed skills")
        XCTAssertEqual(copy.emptyTitle, "No installed skills")
        XCTAssertTrue(copy.emptyMessage.contains("Marketplace"))
        XCTAssertEqual(copy.statusLabel, "Installed")
    }

    func testBuiltInToolsCopyUsesReadOnlyLanguage() {
        let copy = BuiltInToolsCopy.serverLoaded

        XCTAssertEqual(copy.sectionTitle, "Built-in")
        XCTAssertEqual(copy.subtitle, "Native tools bundled with Fawx")
        XCTAssertEqual(copy.searchPrompt, "Search built-in tools")
        XCTAssertEqual(copy.emptyTitle, "No built-in tools reported")
        XCTAssertTrue(copy.emptyMessage.contains("source = builtin"))
    }

    func testLoadedSkillsSectionUsesServerLoadedCopy() {
        XCTAssertEqual(SkillsSection.loadedOnServer.title, LoadedSkillsCopy.serverLoaded.sectionTitle)
        XCTAssertEqual(SkillsSection.loadedOnServer.subtitle, LoadedSkillsCopy.serverLoaded.subtitle)
        XCTAssertEqual(SkillsSection.builtInTools.title, BuiltInToolsCopy.serverLoaded.sectionTitle)
        XCTAssertEqual(SkillsSection.builtInTools.subtitle, BuiltInToolsCopy.serverLoaded.subtitle)
    }
}
