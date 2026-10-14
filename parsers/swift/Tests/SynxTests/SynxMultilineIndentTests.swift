import XCTest
@testable import Synx

/// SYNX 3.7 §8.4.1 — the `|+` indent-preserving multiline opener, plus the
/// 3.7.2–3.7.4 parser rules that sit around block bodies.
final class SynxMultilineIndentTests: XCTestCase {

    private func str(_ src: String, _ key: String) -> String? {
        guard case .object(let o) = SynxParser.parse(src).root,
              case .string(let s) = o[key] ?? .null else { return nil }
        return s
    }

    func test_preserve_indent_relative_to_first_continuation_line() {
        // The worked example from the normative text.
        let v = str("prompt |+\n  Outline:\n    - step one\n    - step two\n      sub-step\n  End.\n", "prompt")
        XCTAssertEqual(v, "Outline:\n  - step one\n  - step two\n    sub-step\nEnd.")
    }

    func test_plain_pipe_still_trims_every_line() {
        let v = str("prompt |\n  Outline:\n    - step one\n  End.\n", "prompt")
        XCTAssertEqual(v, "Outline:\n- step one\nEnd.")
    }

    func test_base_indent_locked_on_first_non_empty_line() {
        let v = str("k |+\n    deep\n  shallow\n", "k")
        XCTAssertEqual(v, "deep\nshallow")
    }

    func test_trailing_whitespace_stripped_per_line() {
        let v = str("k |+\n  a   \n  b\t\n", "k")
        XCTAssertEqual(v, "a\nb")
    }

    func test_block_ends_at_opener_indent() {
        let r = SynxParser.parse("k |+\n  body\nother value\n")
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["k"], .string("body"))
        XCTAssertEqual(o["other"], .string("value"))
    }

    func test_non_ascii_survives_the_slice() {
        let v = str("k |+\n  Привет\n    мир\n", "k")
        XCTAssertEqual(v, "Привет\n  мир")
    }

    func test_unicode_whitespace_indent_never_splits_a_character() {
        // Indent is measured in UTF-8 bytes over Unicode whitespace: the NBSP and
        // ideographic space below are multi-byte indent characters.
        let v = str("k |+\n  first\n  \u{00A0} second\n  \u{3000}third\n", "k")
        XCTAssertEqual(v, "first\n\u{00A0} second\n\u{3000}third")
    }

    // MARK: - body lines are raw text

    func test_comment_markers_in_block_body_are_content() {
        let v = str("k |+\n  # hash\n  // slashes\n  ###\n  end\nafter ok\n", "k")
        XCTAssertEqual(v, "# hash\n// slashes\n###\nend")
        XCTAssertEqual(str("k |+\n  # hash\n  ###\nafter ok\n", "after"), "ok")
    }

    func test_directives_in_block_body_are_content() {
        let r = SynxParser.parse("k |\n  !active\n  !include other.synx\ntail end\n")
        XCTAssertEqual(r.mode, .static)
        XCTAssertTrue(r.includes.isEmpty)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["k"], .string("!active\n!include other.synx"))
        XCTAssertEqual(o["tail"], .string("end"))
    }

    // MARK: - BOM and type hint spacing

    func test_bom_is_dropped_and_active_still_switches_mode() {
        let r = SynxParser.parse("\u{FEFF}!active\nname Wario\n")
        XCTAssertEqual(r.mode, .active)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["name"], .string("Wario"))
        XCTAssertNil(o["\u{FEFF}!active"])
    }

    func test_type_hint_only_applies_directly_after_the_key() {
        let r = SynxParser.parse("a (int) 5\nb(int) 5\n")
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["a"], .string("(int) 5"))
        XCTAssertEqual(o["b"], .int(5))
    }

    // MARK: - canonical JSON floats (ryu layout)

    func test_json_floats_use_shortest_round_trip_digits() {
        XCTAssertEqual(SynxJSON.encode(.float(3.14)), "3.14")
        XCTAssertEqual(SynxJSON.encode(.float(1.0)), "1.0")
        XCTAssertEqual(SynxJSON.encode(.float(-0.5)), "-0.5")
        XCTAssertEqual(SynxJSON.encode(.float(100.0)), "100.0")
        XCTAssertEqual(SynxJSON.encode(.float(0.00001)), "0.00001")
        XCTAssertEqual(SynxJSON.encode(.float(1e-7)), "1e-7")
        XCTAssertEqual(SynxJSON.encode(.float(1e16)), "1e16")
        XCTAssertEqual(SynxJSON.encode(.float(1.5e20)), "1.5e20")
        XCTAssertEqual(SynxJSON.encode(.float(123456789012345.0)), "123456789012345.0")
    }
}
