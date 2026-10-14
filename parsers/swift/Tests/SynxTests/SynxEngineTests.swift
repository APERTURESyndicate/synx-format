import XCTest
@testable import Synx

final class SynxEngineTests: XCTestCase {

    func test_env_default() {
        var opts = SynxOptions()
        opts.env = ["APP_PORT": "9090"]
        var r = SynxParser.parse("!active\nport:env:default:3000 APP_PORT\n")
        SynxEngine.resolve(&r, options: opts)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["port"], .int(9090))
    }

    func test_env_falls_back_to_default() {
        var opts = SynxOptions()
        opts.env = [:]
        var r = SynxParser.parse("!active\nport:env:default:3000 NOT_SET\n")
        SynxEngine.resolve(&r, options: opts)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["port"], .int(3000))
    }

    func test_calc_basic() {
        var r = SynxParser.parse("!active\nprice 100\ntax:calc price * 0.2\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        let n = o["tax"]?.asDouble ?? -1
        XCTAssertEqual(n, 20.0, accuracy: 0.01)
    }

    func test_secret_redacted_in_json() {
        var r = SynxParser.parse("!active\ntoken:secret abc123\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        let json = SynxJSON.encode(r.root)
        XCTAssertTrue(json.contains("[SECRET]"))
        XCTAssertFalse(json.contains("abc123"))
    }

    func test_clamp() {
        var r = SynxParser.parse("!active\nx:clamp:0:10 99\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["x"]?.asDouble, 10.0)
    }

    func test_format_padded() {
        var r = SynxParser.parse("!active\nnum:format:%05d 42\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["num"], .string("00042"))
    }

    // MARK: - 3.7.4: `[type:string]` keeps env text, exact 64-bit ids

    private func resolveEnv(_ text: String) -> SynxObject {
        var opts = SynxOptions()
        opts.env = ["ZIP": "007", "SNOW": "1550184245153112174", "Q": "\"12\""]
        var r = SynxParser.parse(text)
        SynxEngine.resolve(&r, options: opts)
        guard case .object(let o) = r.root else { return SynxObject() }
        return o
    }

    func test_env_type_string_constraint_keeps_text() {
        let o = resolveEnv("!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\n"
            + "fallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n")
        XCTAssertEqual(o["id"], .string("1550184245153112174"))
        XCTAssertEqual(o["zip"], .string("007"))
        XCTAssertEqual(o["fallback"], .string("0042"))
        XCTAssertEqual(o["plain"], .int(7))
    }

    func test_env_type_hint_string_keeps_text() {
        let o = resolveEnv("!active\nzip(string):env ZIP\n")
        XCTAssertEqual(o["zip"], .string("007"))
    }

    func test_env_cast_keeps_64bit_integers_exact() {
        let o = resolveEnv("!active\nraw:env SNOW\nq:env Q\n")
        XCTAssertEqual(o["raw"], .int(1550184245153112174))
        XCTAssertEqual(o["q"], .string("12"))
    }

    func test_default_standalone_honours_string_constraint() {
        let o = resolveEnv("!active\na[type:string]:default:007 null\nb:default:007 null\n")
        XCTAssertEqual(o["a"], .string("007"))
        XCTAssertEqual(o["b"], .int(7))
    }

    func test_type_string_constraint_accepts_secret() {
        let o = resolveEnv("!active\ntoken[type:string]:secret:env SNOW\n")
        XCTAssertEqual(o["token"], .secret("1550184245153112174"))
        XCTAssertEqual(SynxJSON.encode(.object(o)), "{\"token\":\"[SECRET]\"}")
    }

    func test_env_reference_projection() {
        let o = resolveEnv("!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\n"
            + "raw:env SNOW\nd[type:string]:env:default:007 MISSING\nsec[type:string]:secret:env SNOW\nq:env Q\n")
        XCTAssertEqual(SynxJSON.encode(.object(o)),
            "{\"d\":\"007\",\"id\":\"1550184245153112174\",\"q\":\"12\",\"raw\":1550184245153112174,"
            + "\"sec\":\"[SECRET]\",\"z\":7,\"zs\":\"007\"}")
    }

    // MARK: - 3.7.3: interpolation

    func test_interpolation_never_leaks_secret() {
        var r = SynxParser.parse("!active\ntoken:secret abc\nmsg hello {token} world\nsection\n  nested use {token} now\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["msg"], .string("hello [SECRET] world"))
        guard case .object(let section) = o["section"] ?? .null else { return XCTFail() }
        XCTAssertEqual(section["nested"], .string("use [SECRET] now"))
        XCTAssertFalse(SynxJSON.encode(r.root).contains("abc"))
    }

    func test_interpolation_preserves_non_ascii() {
        var r = SynxParser.parse("!active\nname Привет\nmsg мир {name} 東京\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["msg"], .string("мир Привет 東京"))
    }

    func test_auto_interpolation_nested_path() {
        var r = SynxParser.parse("!active\nserver\n  host localhost\n  port 8080\nurl http://{server.host}:{server.port}/api\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["url"], .string("http://localhost:8080/api"))
    }

    func test_interpolation_unresolved_stays_literal() {
        var r = SynxParser.parse("!active\nmsg a {nope} b {\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["msg"], .string("a {nope} b {"))
    }

    func test_interpolated_float_uses_shortest_digits() {
        var r = SynxParser.parse("!active\nratio(float) 3.14\nmsg r={ratio}\n")
        SynxEngine.resolve(&r, options: SynxOptions())
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["msg"], .string("r=3.14"))
    }
}
