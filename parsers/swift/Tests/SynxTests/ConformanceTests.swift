// Conformance corpus runner — replays every `NNN-*.synx` case in the shared
// corpus through the Swift parser and compares the canonical JSON projection
// byte for byte against the sibling `.expected.json`. The corpus lives at
// `tests/conformance/cases` in the repo root; the test skips silently when
// running outside that checkout.
import XCTest
@testable import Synx

final class ConformanceTests: XCTestCase {

    func test_corpus_matches_expected_json() throws {
        guard let dir = findCorpus() else {
            // No corpus on this run — that's fine.
            return
        }
        let fm = FileManager.default
        let items = try fm.contentsOfDirectory(atPath: dir).sorted()
        var checked = 0
        var mismatches: [String] = []
        for name in items where name.hasSuffix(".synx") {
            let base = String(name.dropLast(".synx".count))
            let inputPath = (dir as NSString).appendingPathComponent(name)
            let expectedPath = (dir as NSString).appendingPathComponent(base + ".expected.json")
            guard fm.fileExists(atPath: expectedPath),
                  let inputData = fm.contents(atPath: inputPath),
                  let expectedData = fm.contents(atPath: expectedPath) else { continue }
            // Decode without touching a BOM: stripping it is the parser's job.
            let input = String(decoding: inputData, as: UTF8.self)
            let expected = String(decoding: expectedData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)

            let obj: SynxObject
            if input.drop(while: { $0.isWhitespace }).hasPrefix("!tool") {
                obj = Synx.parseTool(input)
            } else {
                obj = Synx.parseActive(input)
            }
            let got = Synx.toJSON(obj)
            checked += 1
            if got != expected {
                mismatches.append("\(base)\n  got:      \(got)\n  expected: \(expected)")
            }
        }
        print("[corpus] checked \(checked) cases, \(mismatches.count) mismatched")
        XCTAssertGreaterThan(checked, 0, "no conformance cases with .expected.json found in \(dir)")
        XCTAssertTrue(mismatches.isEmpty,
                      "\(mismatches.count) of \(checked) conformance cases differ:\n" + mismatches.joined(separator: "\n"))
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
