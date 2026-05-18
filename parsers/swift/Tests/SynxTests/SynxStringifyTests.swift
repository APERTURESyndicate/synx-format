import XCTest
@testable import Synx

final class SynxStringifyTests: XCTestCase {

    func test_basic_roundtrip() {
        let text = "active true\nage 30\nname Wario\n"
        let r = SynxParser.parse(text)
        let out = SynxStringify.stringify(r.root)
        XCTAssertTrue(out.contains("name Wario"))
        XCTAssertTrue(out.contains("age 30"))
        XCTAssertTrue(out.contains("active true"))
    }

    func test_multiline_uses_pipe() {
        var v = SynxObject()
        v.set("rules", to: .string("a\nb\nc"))
        let out = SynxStringify.stringify(.object(v))
        XCTAssertTrue(out.contains("rules |"))
    }

    func test_formatter_sorts_keys() {
        let out = SynxFormatter.format("b 2\na 1\nc 3\n")
        guard let a = out.range(of: "a 1"), let b = out.range(of: "b 2") else {
            return XCTFail("missing keys")
        }
        XCTAssertLessThan(a.lowerBound, b.lowerBound)
    }

    func test_formatter_preserves_directive() {
        let out = SynxFormatter.format("!active\nname X\n")
        XCTAssertTrue(out.hasPrefix("!active"))
    }
}
