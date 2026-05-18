import XCTest
@testable import Synx

final class SynxDiffTests: XCTestCase {

    func test_identical() {
        var a = SynxObject()
        a.set("x", to: .int(1)); a.set("y", to: .int(2))
        let b = a
        let d = SynxDiff.diff(a, b)
        XCTAssertTrue(d.added.isEmpty)
        XCTAssertTrue(d.removed.isEmpty)
        XCTAssertTrue(d.changed.isEmpty)
        XCTAssertEqual(d.unchanged.count, 2)
    }

    func test_added_removed() {
        var a = SynxObject(); a.set("x", to: .int(1))
        var b = SynxObject(); b.set("y", to: .int(2))
        let d = SynxDiff.diff(a, b)
        XCTAssertEqual(d.added.count, 1)
        XCTAssertEqual(d.removed.count, 1)
    }

    func test_changed() {
        var a = SynxObject(); a.set("name", to: .string("Alice"))
        var b = SynxObject(); b.set("name", to: .string("Bob"))
        let d = SynxDiff.diff(a, b)
        XCTAssertEqual(d.changed.count, 1)
        XCTAssertEqual(d.changed.first?.key, "name")
    }
}
