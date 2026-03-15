import XCTest
@testable import Fawx

final class JSONValueTests: XCTestCase {
    func testDecodePreservesBooleanValues() throws {
        let value = try JSONDecoder().decode(JSONValue.self, from: Data("true".utf8))

        XCTAssertEqual(value, .bool(true))
    }

    func testRoundTripPreservesNestedObjectsAndArrays() throws {
        let original: JSONValue = .object([
            "flag": .bool(true),
            "items": .array([.number(1), .string("ok")]),
        ])

        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(JSONValue.self, from: data)

        XCTAssertEqual(decoded, original)
    }

    func testValueAtTraversesNestedObjectPath() {
        let value: JSONValue = .object([
            "outer": .object([
                "inner": .string("hello"),
            ]),
        ])

        XCTAssertEqual(value.value(at: ["outer", "inner"]), .string("hello"))
    }
}
