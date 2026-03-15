import XCTest
@testable import Fawx

final class ServerStatusTests: XCTestCase {
    func testNormalizedPercentagePrefersServerValue() {
        let context = ContextInfo(
            usedTokens: 25,
            maxTokens: 100,
            percentage: 62,
            compactionThreshold: 80
        )

        XCTAssertEqual(context.normalizedPercentage, 62)
    }

    func testNormalizedPercentageFallsBackToDerivedValueWhenReportedValueIsInvalid() {
        let context = ContextInfo(
            usedTokens: 25,
            maxTokens: 100,
            percentage: .infinity,
            compactionThreshold: 80
        )

        XCTAssertEqual(context.normalizedPercentage, 25)
    }
}
