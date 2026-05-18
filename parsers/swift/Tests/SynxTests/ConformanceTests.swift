// Conformance corpus runner — replays every `.synx` file in the shared corpus
// through the Swift parser. The corpus lives at `tests/conformance/cases` in
// the repo root; tests skip silently when running outside that checkout.
import XCTest
@testable import Synx

final class ConformanceTests: XCTestCase {

    func test_corpus_parses_without_error() {
        let candidate = findCorpus()
        guard let dir = candidate else {
            // No corpus on this run — that's fine.
            return
        }
        let fm = FileManager.default
        guard let items = try? fm.contentsOfDirectory(atPath: dir) else { return }
        var parsed = 0
        var failed = 0
        for name in items where name.hasSuffix(".synx") {
            let path = (dir as NSString).appendingPathComponent(name)
            guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { continue }
            let r = SynxParser.parse(text)
            if case .object = r.root {
                parsed += 1
            } else {
                failed += 1
            }
        }
        XCTAssertEqual(failed, 0, "corpus: \(failed) files did not yield an object")
        print("[corpus] parsed \(parsed) files, \(failed) failed")
    }

    private func findCorpus() -> String? {
        let candidates = [
            "tests/conformance/cases",
            "../tests/conformance/cases",
            "../../tests/conformance/cases",
            "../../../tests/conformance/cases",
            "../../../../tests/conformance/cases",
        ]
        let cwd = FileManager.default.currentDirectoryPath
        for c in candidates {
            let abs = (cwd as NSString).appendingPathComponent(c)
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: abs, isDirectory: &isDir), isDir.boolValue {
                return abs
            }
        }
        return nil
    }
}
