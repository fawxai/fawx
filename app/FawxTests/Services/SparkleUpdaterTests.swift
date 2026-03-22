#if os(macOS)
import XCTest
@testable import Fawx

final class SparkleUpdaterTests: XCTestCase {
    func testConfigurationIsNotReadyWhenInfoDictionaryIsEmpty() {
        XCTAssertFalse(
            SparkleConfiguration.isReady(infoDictionary: [:])
        )
    }

    func testConfigurationIsNotReadyWhenFeedURLIsMissing() {
        XCTAssertFalse(
            SparkleConfiguration.isReady(
                infoDictionary: [
                    SparkleConfiguration.publicEDKeyKey: "real-public-key"
                ]
            )
        )
    }

    func testConfigurationIsNotReadyWhenPublicKeyIsMissing() {
        XCTAssertFalse(
            SparkleConfiguration.isReady(
                infoDictionary: [
                    SparkleConfiguration.feedURLKey: "https://fawx.ai/appcast.xml"
                ]
            )
        )
    }

    func testConfigurationIsNotReadyWhenFeedURLIsEmptyOrWhitespace() {
        let invalidValues = ["", "   ", "\n\t  "]

        for invalidValue in invalidValues {
            XCTAssertFalse(
                SparkleConfiguration.isReady(
                    infoDictionary: [
                        SparkleConfiguration.feedURLKey: invalidValue,
                        SparkleConfiguration.publicEDKeyKey: "real-public-key"
                    ]
                )
            )
        }
    }

    func testConfigurationIsNotReadyWhenPublicKeyIsEmptyOrWhitespace() {
        let invalidValues = ["", "   ", "\n\t  "]

        for invalidValue in invalidValues {
            XCTAssertFalse(
                SparkleConfiguration.isReady(
                    infoDictionary: [
                        SparkleConfiguration.feedURLKey: "https://fawx.ai/appcast.xml",
                        SparkleConfiguration.publicEDKeyKey: invalidValue
                    ]
                )
            )
        }
    }

    func testConfigurationIsNotReadyWithPlaceholderPublicKey() {
        XCTAssertFalse(
            SparkleConfiguration.isReady(
                infoDictionary: [
                    SparkleConfiguration.feedURLKey: "https://fawx.ai/appcast.xml",
                    SparkleConfiguration.publicEDKeyKey: SparkleConfiguration.publicKeyPlaceholder
                ]
            )
        )
    }

    func testConfigurationIsReadyWithFeedURLAndRealPublicKey() {
        XCTAssertTrue(
            SparkleConfiguration.isReady(
                infoDictionary: [
                    SparkleConfiguration.feedURLKey: "https://fawx.ai/appcast.xml",
                    SparkleConfiguration.publicEDKeyKey: "real-public-key"
                ]
            )
        )
    }

    @MainActor
    func testUpdaterStartsDormantWithoutRealKeysConfigured() {
        let updater = SparkleUpdater(
            infoDictionary: [
                SparkleConfiguration.feedURLKey: "https://fawx.ai/appcast.xml",
                SparkleConfiguration.publicEDKeyKey: SparkleConfiguration.publicKeyPlaceholder
            ]
        )

        XCTAssertFalse(updater.canCheckForUpdates)
        updater.checkForUpdates()
        XCTAssertFalse(updater.canCheckForUpdates)
    }

    @MainActor
    func testUpdaterStartsDormantWhenInfoDictionaryIsEmpty() {
        let updater = SparkleUpdater(infoDictionary: [:])

        XCTAssertFalse(updater.canCheckForUpdates)
        updater.checkForUpdates()
        XCTAssertFalse(updater.canCheckForUpdates)
    }
}
#endif
