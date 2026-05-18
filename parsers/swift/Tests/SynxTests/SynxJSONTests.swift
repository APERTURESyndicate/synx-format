import XCTest
@testable import Synx

final class SynxJSONTests: XCTestCase {

    func test_primitives() {
        XCTAssertEqual(SynxJSON.encode(.null), "null")
        XCTAssertEqual(SynxJSON.encode(.bool(true)), "true")
        XCTAssertEqual(SynxJSON.encode(.int(42)), "42")
        XCTAssertEqual(SynxJSON.encode(.string("hi")), "\"hi\"")
    }

    func test_secret_redacted() {
        XCTAssertEqual(SynxJSON.encode(.secret("xxx")), "\"[SECRET]\"")
    }

    func test_object_sorted_keys() {
        var o = SynxObject()
        o.set("b", to: .int(2))
        o.set("a", to: .int(1))
        let j = SynxJSON.encode(.object(o))
        XCTAssertTrue(j.contains("\"a\":1"))
        XCTAssertTrue(j.contains("\"b\":2"))
        if let pa = j.range(of: "\"a\""), let pb = j.range(of: "\"b\"") {
            XCTAssertLessThan(pa.lowerBound, pb.lowerBound)
        }
    }

    func test_escapes() {
        let j = SynxJSON.encode(.string("line\nbreak\ttab\"quote\\back"))
        XCTAssertTrue(j.contains("\\n"))
        XCTAssertTrue(j.contains("\\t"))
        XCTAssertTrue(j.contains("\\\""))
        XCTAssertTrue(j.contains("\\\\"))
    }
}
