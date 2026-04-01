import Foundation
import XCTest
@testable import Synx

final class SynxEngineTests: XCTestCase {
    func testParse() throws {
        let j = try SynxEngine.parse("name Test\ncount 3\n")
        XCTAssertTrue(j.contains("name"))
        XCTAssertTrue(j.contains("Test"))
    }

    func testRoundTripCompile() throws {
        let text = "x 1\ny 2\n"
        let bin = try SynxEngine.compile(text, resolved: false)
        XCTAssertTrue(SynxEngine.isSynxb(bin))
        let back = try SynxEngine.decompile(bin)
        XCTAssertTrue(back.contains("x"))
    }

    func testParseTool() throws {
        let j = try SynxEngine.parseTool("!tool\nweb_search\n  query hi\n")
        XCTAssertTrue(j.contains("web_search"))
    }
}
