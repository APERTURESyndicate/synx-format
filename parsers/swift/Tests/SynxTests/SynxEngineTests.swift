import XCTest
@testable import Synx

final class SynxEngineTests: XCTestCase {

    func test_env_default() {
        var opts = SynxOptions()
        opts.env = ["APP_PORT": "9090"]
        var r = SynxParser.parse("!active\nport:env:default:3000 APP_PORT\n")
        SynxEngine.resolve(&r, options: opts)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["port"], .string("9090"))
    }

    func test_env_falls_back_to_default() {
        var opts = SynxOptions()
        opts.env = [:]
        var r = SynxParser.parse("!active\nport:env:default:3000 NOT_SET\n")
        SynxEngine.resolve(&r, options: opts)
        guard case .object(let o) = r.root else { return XCTFail() }
        XCTAssertEqual(o["port"], .string("3000"))
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
}
