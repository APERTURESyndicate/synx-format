import XCTest
@testable import Synx

final class SynxCalcTests: XCTestCase {

    func test_basic_ops() {
        XCTAssertEqual(SynxCalc.evaluate("2 + 3").value, 5)
        XCTAssertEqual(SynxCalc.evaluate("10 - 4").value, 6)
        XCTAssertEqual(SynxCalc.evaluate("3 * 7").value, 21)
        XCTAssertEqual(SynxCalc.evaluate("20 / 4").value, 5)
        XCTAssertEqual(SynxCalc.evaluate("10 % 3").value, 1)
    }

    func test_precedence_and_parens() {
        XCTAssertEqual(SynxCalc.evaluate("2 + 3 * 4").value, 14)
        XCTAssertEqual(SynxCalc.evaluate("(2 + 3) * 4").value, 20)
    }

    func test_negatives() {
        XCTAssertEqual(SynxCalc.evaluate("-5 + 3").value, -2)
        XCTAssertEqual(SynxCalc.evaluate("10 * -2").value, -20)
    }

    func test_div_zero() {
        let r = SynxCalc.evaluate("10 / 0")
        XCTAssertFalse(r.ok)
        XCTAssertFalse(r.error.isEmpty)
    }

    func test_empty() {
        XCTAssertTrue(SynxCalc.evaluate("").ok)
        XCTAssertEqual(SynxCalc.evaluate("").value, 0)
    }
}
