import Foundation
import XCTest
@testable import Synx

/// Every expected value below is synx-core's answer for the same input
/// (`synx parse [--active]` of the working tree, environment as given).
final class SynxHardeningTests: XCTestCase {
    private func active(_ body: String, _ env: [String: String] = [:]) -> String {
        Synx.toJSON(Synx.parseActive("!active\n" + body, options: SynxOptions(env: env)))
    }

    private func plain(_ text: String) -> String { Synx.toJSON(Synx.parse(text)) }

    // MARK: parser

    func test_the_block_under_a_refused_key_goes_with_it() {
        XCTAssertEqual(plain("user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n"),
                       "{\"user\":{\"age\":3,\"profile\":{\"name\":\"x\"}}}")
        let r = Synx.parseFull("a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n")
        XCTAssertEqual(r.mode, .static)
        XCTAssertEqual(Synx.toJSON(r.root), "{\"a\":1,\"c\":2}")
    }

    func test_a_key_without_a_value_ending_an_item_keeps_the_items_after_it() {
        XCTAssertEqual(plain("servers\n  - name a\n    tags\n  - name b\n    tags\n  - name c\n    port 1\n"),
                       "{\"servers\":[{\"name\":\"a\",\"tags\":[]},{\"name\":\"b\",\"tags\":[]},{\"name\":\"c\",\"port\":1}]}")
    }

    func test_a_comment_line_is_no_content() {
        XCTAssertEqual(plain("k\n# note\n  - a\n  - b\nm\n  - x 1\n    # c\n    y 2\n"),
                       "{\"k\":[\"a\",\"b\"],\"m\":[{\"x\":1,\"y\":2}]}")
    }

    func test_the_key_on_a_dash_line_keeps_its_markers_and_constraints() {
        XCTAssertEqual(active("users\n  - password:secret hunter2\n    name bob\n  - host:env H\n    port[min:10] 1\n", ["H": "h1"]),
                       "{\"users\":[{\"name\":\"bob\",\"password\":\"[SECRET]\"},"
                       + "{\"host\":\"h1\",\"port\":\"CONSTRAINT_ERR: 'port' value 1 is below min 10\"}]}")
    }

    // MARK: secrets

    func test_a_secret_is_masked_before_its_turn_in_groups_and_lists_too() {
        XCTAssertEqual(active("leak:alias password\npassword:secret:default:x hunter2\na\n  x {password}\n"
                              + "keys:secret\n  - hunter2\n  - other\ncreds:secret\n  user bob\n  pass hunter2\ns:secret\n"),
                       "{\"a\":{\"x\":\"[SECRET]\"},\"creds\":{\"pass\":\"[SECRET]\",\"user\":\"[SECRET]\"},"
                       + "\"keys\":[\"[SECRET]\",\"[SECRET]\"],\"leak\":\"[SECRET]\",\"password\":\"[SECRET]\",\"s\":{}}")
    }

    func test_an_unset_secret_is_null_and_required_sees_it() {
        XCTAssertEqual(active("k[required]:secret:env SYNX_UNSET_ZZ\nname[required, min:1]\nopt:secret:env SYNX_UNSET_ZZ\npin:secret:env PIN\n", ["PIN": "00123"]),
                       "{\"k\":\"CONSTRAINT_ERR: 'k' is required\",\"name\":\"CONSTRAINT_ERR: 'name' is required\",\"opt\":null,\"pin\":\"[SECRET]\"}")
        let pin = Synx.parseActive("!active\npin:secret:env PIN\n", options: SynxOptions(env: ["PIN": "00123"]))
        XCTAssertEqual(pin["pin"], .secret("00123"))
    }

    // MARK: numbers

    func test_inf_nan_and_out_of_range_results_stay_what_they_are() {
        XCTAssertEqual(active("x(float):round nan\ny(float):round inf\nz:round 1" + String(repeating: "0", count: 30) + ".5\n"
                              + "p[max:-inf] 5\nq[min:nan] 5\na:sum\n  - 1\n  - inf\nb:sum\n  - nan\n  - 2\nh:env HOST\nw:random inf\n"
                              + "s:sort\n  - b\n  - 10\n  - 1a\n  - 9\n  - nan\n", ["HOST": "inf"]),
                       "{\"a\":1,\"b\":2,\"h\":\"inf\",\"p\":5,\"q\":5,\"s\":[9,10,\"1a\",\"b\",\"nan\"],\"w\":\"inf\",\"x\":null,\"y\":null,\"z\":1e30}")
    }

    // MARK: limits

    func test_copies_and_interpolation_stop_at_the_resolution_budget() {
        var doc = "!active\na0 " + String(repeating: "x", count: 1000) + "\n"
        for i in 0..<28 { doc += "a\(i + 1) {a\(i)}{a\(i)}\n" }
        let root = Synx.parseActive(doc, options: SynxOptions(env: [:]))
        XCTAssertEqual(root["a12"]?.stringValue?.utf8.count, 4_096_000)
        XCTAssertEqual(root["a26"]?.stringValue?.utf8.count, 4_194_304)
        XCTAssertEqual(root["a27"], .string("LIMIT_ERR: resolution budget exceeded"))
    }

    func test_an_inheritance_chain_is_budgeted_and_linear() {
        // Every link copied the whole metadata table: 2 000 links took more than 40 s.
        var doc = "!active\nb0\n  v 0\n"
        for i in 0..<2000 { doc += "b\(i + 1):inherit b\(i)\n  w\(i) 1\n" }
        let started = Date()
        let root = Synx.parseActive(doc, options: SynxOptions(env: [:]))
        XCTAssertLessThan(Date().timeIntervalSince(started), 10)
        XCTAssertEqual(root["b1999"], .string("LIMIT_ERR: resolution budget exceeded"))
        // The next link inherits from text, not an object: only its own field.
        XCTAssertEqual(Synx.toJSON(root["b2000"]!), "{\"w1999\":1}")
    }

    func test_a_file_including_itself_stops_at_the_first_repeat() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("synx-swift-cycle-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        try "!active\nb:include b.synx\n".write(to: dir.appendingPathComponent("a.synx"), atomically: true, encoding: .utf8)
        try "!active\na:include a.synx\nv 1\n".write(to: dir.appendingPathComponent("b.synx"), atomically: true, encoding: .utf8)
        let out = Synx.toJSON(Synx.parseActive("!active\nb:include b.synx\n", options: SynxOptions(basePath: dir.path)))
        XCTAssertEqual(out, "{\"b\":{\"a\":{\"b\":\"INCLUDE_ERR: circular include: 'b.synx'\"},\"v\":1}}")
    }

    // MARK: patterns

    func test_the_pattern_dialect_is_the_regex_crates() {
        XCTAssertEqual(active("r[pattern:{] y\nc[pattern:(?i)^abc$] ABC\nd[pattern:(?i)^abc$] x\ne[pattern:^\\p{L}+$] мир\n"
                              + "f[pattern:^\\d+$] ١٢٣\ng[pattern:\\bfoo\\b] food\nh[pattern:a**] b\n"),
                       "{\"c\":\"ABC\",\"d\":\"CONSTRAINT_ERR: 'd' does not match pattern /(?i)^abc$/\",\"e\":\"мир\",\"f\":\"١٢٣\","
                       + "\"g\":\"CONSTRAINT_ERR: 'g' does not match pattern /\\\\bfoo\\\\b/\",\"h\":\"b\",\"r\":\"y\"}")
        XCTAssertNil(SynxPattern.compile("(?=a)"))
        XCTAssertNil(SynxPattern.compile("\\1"))
        XCTAssertTrue(SynxPattern.compile("(?i)\\p{Greek}+")!.isMatch("ΑΒΓ"))
    }

    func test_a_pattern_that_backtracks_elsewhere_answers_in_linear_time() {
        // ICU took more than 40 s on the second one.
        let value = String(repeating: "a", count: 5000) + "!"
        let started = Date()
        XCTAssertFalse(SynxPattern.compile("^(a|aa)+$")!.isMatch(value))
        XCTAssertFalse(SynxPattern.compile("^(.*)(.*)(.*)(.*)(.*)(.*)(.*)x$")!.isMatch(value))
        XCTAssertLessThan(Date().timeIntervalSince(started), 10)
    }

    // MARK: writers

    func test_a_key_that_would_read_back_as_something_else_is_renamed() {
        var o = SynxObject()
        o.set("a\n!active\nleak:env PATH", to: .string("v"))
        o.set("a b", to: .int(1))
        o.set("a_b", to: .int(2))
        o.set("a:b", to: .int(3))
        o.set("__proto__", to: .string("p"))
        let text = Synx.stringify(o)
        XCTAssertEqual(text, "__proto___ p\na_!active_leak_env_PATH v\na_b_2 1\na_b_3 3\na_b 2\n")
        let back = Synx.parseFull(text)
        XCTAssertEqual(back.mode, .static)
        XCTAssertEqual(Synx.toJSON(back.root), "{\"__proto___\":\"p\",\"a_!active_leak_env_PATH\":\"v\",\"a_b\":2,\"a_b_2\":1,\"a_b_3\":3}")
    }
}
