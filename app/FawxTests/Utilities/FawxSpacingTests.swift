import XCTest
@testable import Fawx

final class FawxSpacingTests: XCTestCase {
    func testMaxMessageWidthUsesProportionalWidthInBounds() {
        XCTAssertEqual(FawxSpacing.maxMessageWidth(for: 800), 680)
    }

    func testMaxMessageWidthClampsToMinimumWidth() {
        XCTAssertEqual(FawxSpacing.maxMessageWidth(for: 300), 400)
    }

    func testMaxMessageWidthClampsToMaximumWidth() {
        XCTAssertEqual(FawxSpacing.maxMessageWidth(for: 2000), 1200)
    }

    func testResolvedChatContainerWidthSubtractsOuterPadding() {
        XCTAssertEqual(FawxSpacing.resolvedChatContainerWidth(for: 600), 552)
    }

    func testResolvedChatContainerWidthClampsToPositiveMinimum() {
        XCTAssertEqual(FawxSpacing.resolvedChatContainerWidth(for: 24), 1)
    }
}
