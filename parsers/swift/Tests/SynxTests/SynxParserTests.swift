import XCTest
@testable import Synx

final class SynxParserTests: XCTestCase {

    func test_parse_simple_kv() {
        let r = SynxParser.parse("name Wario\nage 30\nactive true\nscore 99.5\nempty null")
        guard case .object(let o) = r.root else { return XCTFail("root not object") }
        XCTAssertEqual(o["name"], .string("Wario"))
        XCTAssertEqual(o["age"], .int(30))
        XCTAssertEqual(o["active"], .bool(true))
        XCTAssertEqual(o["score"], .float(99.5))
        XCTAssertEqual(o["empty"], .null)
        XCTAssertEqual(r.mode, .static)
    }

    func test_parse_nested_objects() {
        let r = SynxParser.parse("server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true")
        guard case .object(let o) = r.root,
              case .object(let server) = o["server"] ?? .null else {
            return XCTFail("nested mismatch")
        }
        XCTAssertEqual(server["port"], .int(8080))
        guard case .object(let ssl) = server["ssl"] ?? .null else { return XCTFail() }
        XCTAssertEqual(ssl["enabled"], .bool(true))
    }

    func test_parse_lists() {
        let r = SynxParser.parse("inventory\n  - Sword\n  - Shield\n  - Potion")
        guard case .object(let o) = r.root,
              case .array(let inv) = o["inventory"] ?? .null else {
            return XCTFail()
        }
        XCTAssertEqual(inv.count, 3)
    }

    func test_parse_multiline_block() {
        let r = SynxParser.parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.")
        guard case .object(let o) = r.root, case .string(let s) = o["rules"] ?? .null else {
            return XCTFail()
        }
        XCTAssertTrue(s.contains("\n"))
    }

    func test_parse_active_metadata() {
        let r = SynxParser.parse("!active\nprice 100\ntax:calc price * 0.2")
        XCTAssertEqual(r.mode, .active)
        XCTAssertNotNil(r.metadata[""]?["tax"])
        XCTAssertEqual(r.metadata[""]?["tax"]?.markers, ["calc"])
    }

    func test_parse_prototype_pollution_rejected() {
        let r = SynxParser.parse("__proto__ evil\nconstructor evil\nprototype evil\nname safe\n")
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertFalse(o.contains("__proto__"))
        XCTAssertFalse(o.contains("constructor"))
        XCTAssertFalse(o.contains("prototype"))
        XCTAssertTrue(o.contains("name"))
    }

    func test_parse_constraints() {
        let r = SynxParser.parse("!active\nname[min:3, max:30, required] Wario")
        let c = r.metadata[""]?["name"]?.constraints
        XCTAssertEqual(c?.min, 3.0)
        XCTAssertEqual(c?.max, 30.0)
        XCTAssertEqual(c?.required, true)
    }

    func test_parse_type_hint_string_keeps_string() {
        let r = SynxParser.parse("zip(string) 90210")
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["zip"], .string("90210"))
    }

    func test_parse_tool_directive() {
        let r = SynxParser.parse("!tool\nweb_search\n  query test\n  lang ru\n")
        XCTAssertTrue(r.tool)
        let shaped = SynxParser.reshapeToolOutput(r.root, schema: false)
        guard case .object(let o) = shaped else { return XCTFail() }
        XCTAssertEqual(o["tool"], .string("web_search"))
    }
}
