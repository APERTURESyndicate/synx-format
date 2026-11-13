// Port worker for the Swift implementation: one request per line on stdin,
//   id \t op \t base64(text) \t base64(KEY=VALUE lines) \t base directory
// and one answer per line on stdout,
//   id \t base64(canonical JSON, or "ERROR: …") \t base64(default string form of the tree)
import Foundation
import Synx

while let line = readLine(strippingNewline: true) {
    let f = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
    if f.count < 5 { continue }
    if f[1] == "decompile" {
        // The third field is the .synxb container itself.
        var answer: String
        switch Synx.decompile(Data(base64Encoded: f[2]) ?? Data()) {
        case .success(let text): answer = text
        case .failure(let error): answer = "ERROR: \(error)"
        }
        print("\(f[0])\t\(Data(answer.utf8).base64EncodedString())\t")
        fflush(stdout)
        continue
    }
    let text = String(decoding: Data(base64Encoded: f[2]) ?? Data(), as: UTF8.self)
    let envRaw = String(decoding: Data(base64Encoded: f[3]) ?? Data(), as: UTF8.self)
    var env: [String: String] = [:]
    for pair in envRaw.split(separator: "\n") {
        if let eq = pair.firstIndex(of: "="), eq != pair.startIndex {
            env[String(pair[..<eq])] = String(pair[pair.index(after: eq)...])
        }
    }
    let options = SynxOptions(env: env, basePath: f[4].trimmingCharacters(in: .newlines))
    let result = f[1] == "active" ? Synx.parseFullActive(text, options: options) : Synx.parseFull(text)
    let json = Synx.toJSON(result.root)
    let shown = "\(result.root) \(String(describing: result.root)) \(String(reflecting: result.root))"
    print("\(f[0])\t\(Data(json.utf8).base64EncodedString())\t\(Data(shown.utf8).base64EncodedString())")
    fflush(stdout)
}
