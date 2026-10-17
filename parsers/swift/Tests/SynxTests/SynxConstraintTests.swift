import XCTest
@testable import Synx

/// Constraint and type-hint failures are written into the value, exactly like
/// the Rust engine (`CONSTRAINT_ERR: …`, `TYPE_ERR: …`).
final class SynxConstraintTests: XCTestCase {

    private func resolve(_ text: String) -> SynxObject {
        return Synx.parseActive(text)
    }

    private func str(_ o: SynxObject, _ key: String) -> String? {
        return o[key]?.stringValue
    }

    func test_required_type_enum_messages() {
        let o = resolve("!active\nmissing[required] null\nblank[required] \"\"\n"
            + "age_bad[type:int] forty\nflag[type:bool] yes\n"
            + "color_bad[enum:red|green|blue] purple\n")
        XCTAssertEqual(str(o, "missing"), "CONSTRAINT_ERR: 'missing' is required")
        XCTAssertEqual(str(o, "blank"), "CONSTRAINT_ERR: 'blank' is required")
        XCTAssertEqual(str(o, "age_bad"), "CONSTRAINT_ERR: 'age_bad' expected type 'int'")
        XCTAssertEqual(str(o, "flag"), "CONSTRAINT_ERR: 'flag' expected type 'bool'")
        XCTAssertEqual(str(o, "color_bad"), "CONSTRAINT_ERR: 'color_bad' must be one of [red|green|blue]")
    }

    func test_float_type_accepts_int() {
        let o = resolve("!active\nratio[type:float] 3\n")
        XCTAssertEqual(o["ratio"], .int(3))
    }

    func test_min_max_numbers_print_like_rust_display() {
        let o = resolve("!active\nlow[min:1, max:10] 0\nhigh[min:1, max:10] 11\nscore[min:0.5] 0.25\n")
        XCTAssertEqual(str(o, "low"), "CONSTRAINT_ERR: 'low' value 0 is below min 1")
        XCTAssertEqual(str(o, "high"), "CONSTRAINT_ERR: 'high' value 11 exceeds max 10")
        XCTAssertEqual(str(o, "score"), "CONSTRAINT_ERR: 'score' value 0.25 is below min 0.5")
    }

    func test_string_length_counts_unicode_scalars_not_bytes() {
        let o = resolve("!active\ncyr[max:4] мир\ncyr_long[max:4] мирок\nemoji[max:2] 😀😀\n")
        XCTAssertEqual(o["cyr"], .string("мир"))
        XCTAssertEqual(o["emoji"], .string("😀😀"))
        XCTAssertEqual(str(o, "cyr_long"), "CONSTRAINT_ERR: 'cyr_long' value 5 exceeds max 4")
    }

    func test_pattern_mismatch_message() {
        let o = resolve("!active\nok[pattern:^[a-z]+$] abc\nbad[pattern:^[a-z]+$] ABC\n")
        XCTAssertEqual(o["ok"], .string("abc"))
        XCTAssertEqual(str(o, "bad"), "CONSTRAINT_ERR: 'bad' does not match pattern /^[a-z]+$/")
    }

    func test_constraints_run_after_markers() {
        let o = resolve("!active\nbase 7\ntotal[max:10]:calc base * 2\nsmall[max:20]:calc base * 2\n")
        XCTAssertEqual(str(o, "total"), "CONSTRAINT_ERR: 'total' value 14 exceeds max 10")
        XCTAssertEqual(o["small"], .int(14))
    }

    func test_field_registry_applies_by_name_across_the_document() {
        let o = resolve("!active\na\n  port[type:int, min:1, max:65535] 8080\nb\n  port 70000\nc\n  port abc\n")
        XCTAssertEqual(o["a"]?["port"], .int(8080))
        XCTAssertEqual(o["b"]?["port"]?.stringValue, "CONSTRAINT_ERR: 'port' value 70000 exceeds max 65535")
        XCTAssertEqual(o["c"]?["port"]?.stringValue, "CONSTRAINT_ERR: 'port' expected type 'int'")
    }

    func test_registry_merge_takes_the_stricter_bounds() {
        let o = resolve("!active\na[min:1, max:100] 50\nb\n  a 5\nc[min:10, max:20] 15\nd\n  c 25\n")
        // `a` declared once; `c` once — merged registry equals the declaration.
        XCTAssertEqual(o["a"], .int(50))
        XCTAssertEqual(o["d"]?["c"]?.stringValue, "CONSTRAINT_ERR: 'c' value 25 exceeds max 20")
        let m = resolve("!active\nx[min:1, max:100] 5\ny\n  x[min:10, max:50] 7\n")
        XCTAssertEqual(m["x"]?.stringValue, "CONSTRAINT_ERR: 'x' value 5 is below min 10")
        XCTAssertEqual(m["y"]?["x"]?.stringValue, "CONSTRAINT_ERR: 'x' value 7 is below min 10")
    }

    func test_type_registry_reports_type_err() {
        let o = resolve("!active\nhp(int) 100\nmonster\n  hp 50\nboss\n  hp lots\n")
        XCTAssertEqual(o["hp"], .int(100))
        XCTAssertEqual(o["monster"]?["hp"], .int(50))
        XCTAssertEqual(o["boss"]?["hp"]?.stringValue, "TYPE_ERR: 'hp' expected int but got string")
    }

    func test_existing_error_strings_are_not_revalidated() {
        let o = resolve("!active\nage[type:int, max:5] forty\n")
        XCTAssertEqual(str(o, "age"), "CONSTRAINT_ERR: 'age' expected type 'int'")
    }

    // MARK: - ReDoS guard

    func test_nested_quantifier_pattern_is_refused() {
        XCTAssertTrue(SynxValidate.hasNestedQuantifier("^(a+)+$"))
        XCTAssertTrue(SynxValidate.hasNestedQuantifier("(a*){2,}"))
        XCTAssertFalse(SynxValidate.hasNestedQuantifier("^[a-z]+$"))
        XCTAssertFalse(SynxValidate.hasNestedQuantifier("^(ab)c+$"))
        XCTAssertFalse(SynxValidate.hasNestedQuantifier("^([+*])$"))
    }

    func test_redos_pattern_is_skipped_quickly_and_value_unchanged() {
        let value = String(repeating: "a", count: 45) + "!"
        let started = Date()
        let o = resolve("!active\nv[pattern:^(a+)+$] \(value)\n")
        XCTAssertLessThan(Date().timeIntervalSince(started), 2.0)
        XCTAssertEqual(o["v"], .string(value))
    }

    // MARK: - :calc substitutes shortest floats

    func test_calc_substitution_matches_rust_number_formatting() {
        let o = resolve("!active\na 0.1\nb:calc a * 3\nc:calc a + 0.2\nd 2.0\ne:calc d * 3\nf:calc a / 3\n")
        XCTAssertEqual(o["b"], .float(0.30000000000000004))
        XCTAssertEqual(o["c"], .float(0.30000000000000004))
        XCTAssertEqual(o["e"], .int(6))
        XCTAssertEqual(o["f"], .float(0.03333333333333333))
    }

    func test_whole_floats_interpolate_without_fraction() {
        let o = resolve("!active\nratio(float) 2.0\nmsg {ratio} x\n")
        XCTAssertEqual(o["msg"], .string("2 x"))
    }

    // MARK: - invalid patterns are skipped, never trapped

    func test_invalid_patterns_are_skipped_silently() {
        for pat in ["[", "(", "a{2,1}", "\\", "*a", "(?<", "[z-a]", ")"] {
            let o = resolve("!active\nv[pattern:\(pat)] hello\n")
            XCTAssertEqual(o["v"], .string("hello"), "pattern \(pat)")
        }
    }

    // MARK: - :calc failures carry the exact Rust CALC_ERR string

    func test_calc_failures_write_calc_err_into_the_value() {
        let dash = "\u{2014}"
        let o = resolve("!active\nbase 7\nu:calc nope + 1\nz:calc 1 / 0\nm:calc 5 % 0\n"
            + "p1:calc (1 + 2\np2:calc 1 + 2)\nstar:calc * 3\nrp:calc )\nhole:calc 2 +\n"
            + "num:calc 1.2.3 + 1\nop:calc 2 * -\nbad:calc 3 3\nuni:calc мир\n")
        XCTAssertEqual(o["u"], .string("CALC_ERR: SYNX :calc \(dash) unexpected character: 'n' in expression"))
        XCTAssertEqual(o["z"], .string("CALC_ERR: SYNX :calc \(dash) division by zero"))
        XCTAssertEqual(o["m"], .string("CALC_ERR: SYNX :calc \(dash) division by zero"))
        XCTAssertEqual(o["p1"], .string("CALC_ERR: SYNX :calc \(dash) missing closing parenthesis"))
        XCTAssertEqual(o["p2"], .string("CALC_ERR: SYNX :calc \(dash) unexpected token at position 3"))
        // 3.8.0: the token as written, not Rust's `{:?}` of it.
        XCTAssertEqual(o["star"], .string("CALC_ERR: SYNX :calc \(dash) unexpected token: '*'"))
        XCTAssertEqual(o["rp"], .string("CALC_ERR: SYNX :calc \(dash) unexpected token: ')'"))
        XCTAssertEqual(o["hole"], .string("CALC_ERR: SYNX :calc \(dash) unexpected end of expression"))
        XCTAssertEqual(o["num"], .string("CALC_ERR: SYNX :calc \(dash) invalid number: '1.2.3'"))
        XCTAssertEqual(o["op"], .string("CALC_ERR: SYNX :calc \(dash) invalid number: '-'"))
        XCTAssertEqual(o["bad"], .string("CALC_ERR: SYNX :calc \(dash) unexpected token at position 1"))
        // 3.8.0: the whole character, not its first byte read as Latin-1.
        XCTAssertEqual(o["uni"], .string("CALC_ERR: SYNX :calc \(dash) unexpected character: 'м' in expression"))
    }

    func test_calc_too_long_expression() {
        let long = String(repeating: "1+", count: 2100) + "1"
        let o = resolve("!active\nlong:calc \(long)\n")
        XCTAssertEqual(o["long"], .string("CALC_ERR: expression too long (4201 chars, max 4096)"))
    }

    func test_calc_reads_siblings_and_dotted_paths() {
        let o = resolve("!active\nbase_obj\n  hp 10\n  dmg:calc hp * 2\ndot:calc base_obj.hp + 1\n")
        XCTAssertEqual(o["base_obj"]?["dmg"], .int(20))
        XCTAssertEqual(o["dot"], .int(11))
    }
}
