import XCTest
@testable import Fawx

final class ThinkingLevelTests: XCTestCase {
    private let decoder = JSONDecoder()

    func testThinkingConfigDecodesValidLevelsFromNewBackendField() throws {
        let payload = """
        {
            "level": "adaptive",
            "valid_levels": ["off", "low", "medium", "high", "max", "adaptive"]
        }
        """

        let config = try decoder.decode(ThinkingConfig.self, from: Data(payload.utf8))

        XCTAssertEqual(config.level, .adaptive)
        XCTAssertEqual(
            config.validLevels.map(\.rawValue),
            ["off", "low", "medium", "high", "max", "adaptive"]
        )
    }

    func testThinkingConfigFallsBackToLegacyAvailableLevels() throws {
        let payload = """
        {
            "level": "high",
            "available": ["off", "low", "high"],
            "budget_tokens": 10000
        }
        """

        let config = try decoder.decode(ThinkingConfig.self, from: Data(payload.utf8))

        XCTAssertEqual(config.level, .high)
        XCTAssertEqual(config.budgetTokens, 10000)
        XCTAssertEqual(config.validLevels.map(\.rawValue), ["off", "low", "high"])
    }

    func testThinkingConfigFallsBackToCurrentLevelWhenBackendOmitsLevelList() throws {
        let payload = """
        {
            "level": "minimal"
        }
        """

        let config = try decoder.decode(ThinkingConfig.self, from: Data(payload.utf8))

        XCTAssertEqual(config.level.rawValue, "minimal")
        XCTAssertEqual(config.validLevels.map(\.rawValue), ["minimal"])
    }

    func testSetThinkingResponseDecodesValidLevelsFromNewBackendField() throws {
        let payload = """
        {
            "previous_level": "low",
            "level": "adaptive",
            "valid_levels": ["off", "low", "medium", "high", "adaptive"]
        }
        """

        let response = try decoder.decode(SetThinkingResponse.self, from: Data(payload.utf8))

        XCTAssertEqual(response.previousLevel, .low)
        XCTAssertEqual(response.level, .adaptive)
        XCTAssertEqual(
            response.validLevels.map(\.rawValue),
            ["off", "low", "medium", "high", "adaptive"]
        )
    }

    func testSetThinkingResponseFallsBackToLegacyAvailableLevels() throws {
        let payload = """
        {
            "previous_level": "off",
            "level": "high",
            "available": ["off", "low", "high"],
            "budget_tokens": 10000
        }
        """

        let response = try decoder.decode(SetThinkingResponse.self, from: Data(payload.utf8))

        XCTAssertEqual(response.previousLevel, .off)
        XCTAssertEqual(response.level, .high)
        XCTAssertEqual(response.budgetTokens, 10000)
        XCTAssertEqual(response.validLevels.map(\.rawValue), ["off", "low", "high"])
    }
}
