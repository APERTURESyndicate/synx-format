import XCTest
@testable import Synx

final class SynxBinaryTests: XCTestCase {

    func test_static_roundtrip() {
        let r = SynxParser.parse("name App\nport 8080\n")
        let compiled = SynxBinary.compile(r, resolved: false)
        switch compiled {
        case .failure(let e):
            // Skip silently on Linux without Compression framework.
            if case .unsupportedPlatform = e { return }
            return XCTFail("compile failed: \(e)")
        case .success(let bytes):
            XCTAssertTrue(SynxBinary.isSynxb(bytes))
            switch SynxBinary.decompile(bytes) {
            case .success(let restored):
                XCTAssertEqual(restored.root, r.root)
            case .failure(let e):
                XCTFail("decompile failed: \(e)")
            }
        }
    }

    func test_magic_check() {
        XCTAssertTrue(SynxBinary.isSynxb(Data([0x53, 0x59, 0x4E, 0x58, 0x42, 1, 0])))
        XCTAssertFalse(SynxBinary.isSynxb(Data([0x4A, 0x53, 0x4F, 0x4E])))
    }

    func test_invalid_magic_rejected() {
        let bad = Data([0x57, 0x52, 0x4F, 0x4E, 0x47] + [UInt8](repeating: 0, count: 6))
        switch SynxBinary.decompile(bad) {
        case .success: XCTFail("expected failure")
        case .failure: break
        }
    }
}
