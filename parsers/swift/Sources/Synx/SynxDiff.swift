// Structural diff between two SYNX values. Mirrors crates/synx-core/src/diff.rs.
import Foundation

public struct SynxDiffChange: Equatable, Sendable {
    public var from: SynxValue
    public var to: SynxValue
}

public struct SynxDiffResult: Sendable {
    public var added: SynxObject
    public var removed: SynxObject
    public var changed: [(key: String, change: SynxDiffChange)]
    public var unchanged: [String]
}

public enum SynxDiff {

    public static func diff(_ a: SynxObject, _ b: SynxObject) -> SynxDiffResult {
        var added = SynxObject()
        var removed = SynxObject()
        var changed: [(String, SynxDiffChange)] = []
        var unchanged: [String] = []

        for entry in a.entries {
            if let bv = b[entry.key] {
                if entry.value == bv {
                    unchanged.append(entry.key)
                } else {
                    changed.append((entry.key, SynxDiffChange(from: entry.value, to: bv)))
                }
            } else {
                removed.set(entry.key, to: entry.value)
            }
        }
        for entry in b.entries where a[entry.key] == nil {
            added.set(entry.key, to: entry.value)
        }
        unchanged.sort()
        return SynxDiffResult(added: added, removed: removed, changed: changed, unchanged: unchanged)
    }

    public static func toValue(_ d: SynxDiffResult) -> SynxValue {
        var root = SynxObject()
        root.set("added", to: .object(d.added))
        root.set("removed", to: .object(d.removed))

        var changedObj = SynxObject()
        for (k, c) in d.changed {
            var inner = SynxObject()
            inner.set("from", to: c.from)
            inner.set("to",   to: c.to)
            changedObj.set(k, to: .object(inner))
        }
        root.set("changed", to: .object(changedObj))

        let arr = d.unchanged.map { SynxValue.string($0) }
        root.set("unchanged", to: .array(arr))
        return .object(root)
    }
}
