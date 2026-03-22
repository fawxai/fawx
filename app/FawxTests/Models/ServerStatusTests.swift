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

    func testApplyingCompactionUpdatePreservesKnownBudget() {
        let context = ContextInfo(
            usedTokens: 68,
            maxTokens: 100,
            percentage: 68,
            compactionThreshold: 80
        )

        let updated = context.applyingCompaction(usedTokens: 42, usageRatio: 0.42)

        XCTAssertEqual(updated.usedTokens, 42)
        XCTAssertEqual(updated.maxTokens, 100)
        XCTAssertEqual(updated.normalizedPercentage, 42)
        XCTAssertEqual(updated.compactionThreshold, 80)
    }

    func testApplyingCompactionUpdateDerivesBudgetWhenMaxTokensIsUnknown() {
        let context = ContextInfo(
            usedTokens: 68,
            maxTokens: 0,
            percentage: 0,
            compactionThreshold: 80
        )

        let updated = context.applyingCompaction(usedTokens: 42, usageRatio: 0.42)

        XCTAssertEqual(updated.usedTokens, 42)
        XCTAssertEqual(updated.maxTokens, 100)
        XCTAssertEqual(updated.normalizedPercentage, 42)
        XCTAssertEqual(updated.compactionThreshold, 80)
    }
}
