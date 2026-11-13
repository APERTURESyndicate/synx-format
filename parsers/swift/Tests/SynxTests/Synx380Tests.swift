// SYNX 3.8.0 — one test per defect fixed in this port (numbers refer to the
// 3.8.0 changelog items). Expected texts come from synx-core 3.8.0.
import XCTest
@testable import Synx

final class Synx380Tests: XCTestCase {

    private func json(_ text: String) -> String { Synx.toJSON(Synx.parseActive(text)) }

    // 1 — one pass in document order: a marker sees every key above it resolved.
    func test_resolution_follows_document_order() {
        XCTAssertEqual(json("!active\na:calc 2 * 3\nb\n  x v={a}\n  y:calc a + 1\nprice 100\ntax:calc price * 0.2\ntotal:calc price + tax\n"),
                       "{\"a\":6,\"b\":{\"x\":\"v=6\",\"y\":7},\"price\":100,\"tax\":20,\"total\":120}")
    }

    // 2 — every list item, every dotted key and every replacement has its own declarations.
    func test_declarations_belong_to_their_object() {
        XCTAssertEqual(json("!active\ns\n  - n a\n    p:env:default:7 SYNX_T_UNSET\n  - n b\n    p:default:8 null\n  - n c\n    p null\n"),
                       "{\"s\":[{\"n\":\"a\",\"p\":7},{\"n\":\"b\",\"p\":8},{\"n\":\"c\",\"p\":null}]}")
        XCTAssertEqual(json("!active\na.b\n  x:default:1 null\na\n  b\n    x null\n"),
                       "{\"a\":{\"b\":{\"x\":null}},\"a.b\":{\"x\":1}}")
        XCTAssertEqual(json("!active\nport\n  port[enum:a|b] x\nport\nq\n  - k[min:5] 1\nq 3\n"), "{\"port\":{},\"q\":3}")
        // Metadata paths: list items as `#<index>`, a dot in a key escaped.
        let r = SynxParser.parse("!active\ns\n  - n a\n    p:env X\na.b\n  x:env Y\n")
        XCTAssertNotNil(r.metadata["s.#0"])
        XCTAssertNotNil(r.metadata["a\\.b"])
    }

    // 3 — registries from the live tree; the first type for a name wins.
    func test_type_registry_first_declaration_wins() {
        XCTAssertEqual(json("!active\na\n  x(int) 1\nb\n  x(string) 2\n"),
                       "{\"a\":{\"x\":1},\"b\":{\"x\":\"TYPE_ERR: 'x' expected int but got string\"}}")
    }

    // 4 — :inherit merges parents left to right with their markers; a list child stays.
    func test_inherit_carries_markers_and_merges_parents() {
        XCTAssertEqual(json("!active\nbase\n  p:env:default:5 SYNX_T_UNSET\n  sub\n    k:default:9 null\nlst:inherit:base\n  - 1\n"
                            + "kid:inherit:base:other\n  z 1\nother\n  p 6\n  w 2\n"),
                       "{\"base\":{\"p\":5,\"sub\":{\"k\":9}},\"kid\":{\"p\":6,\"sub\":{\"k\":9},\"w\":2,\"z\":1},\"lst\":[1],\"other\":{\"p\":6,\"w\":2}}")
    }

    // 5 — :calc: local key before root key, special keys, malformed literals, whole characters.
    func test_calc_scope_and_errors() {
        let e = "CALC_ERR: SYNX :calc \u{2014} "
        XCTAssertEqual(json("!active\nlimit 5\nsvc\n  limit 100\n  price:calc limit * 2\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\n"
                            + "bad:calc 1.2.3 + 1\nlone:calc -\nru:calc цена * 2\n"),
                       "{\"bad\":\"\(e)invalid number: '1.2.3'\",\"hp\":3,\"left\":7,\"limit\":5,\"lone\":\"\(e)invalid number: '-'\","
                       + "\"max-hp\":10,\"ru\":\"\(e)unexpected character: 'ц' in expression\",\"svc\":{\"limit\":100,\"price\":200}}")
    }

    // 6 — a null value checks only [enum].
    func test_null_checks_only_enum() {
        XCTAssertEqual(json("!active\np[type:int]:env SYNX_T_UNSET\nm[enum:a|b]:env SYNX_T_UNSET\nok[enum:a|]:env SYNX_T_UNSET\n"
                            + "n[min:3, pattern:^x]:env SYNX_T_UNSET\n"),
                       "{\"m\":\"CONSTRAINT_ERR: 'm' must be one of [a|b]\",\"n\":null,\"ok\":null,\"p\":null}")
    }

    // 7 — values as text: plain decimal, a group is empty text, null is `null`.
    func test_values_as_text() {
        XCTAssertEqual(json("!active\nf 0.0000001\nbig 1e21\nh 1000000000000000000000.0\no\n  k 1\nz null\nt {f} {h} [{o}] {z}\n"
                            + "l:join\n  - 2.5\n  - 0.0000001\n  - true\n"),
                       "{\"big\":\"1e21\",\"f\":1e-7,\"h\":1e21,\"l\":\"2.5,0.0000001,true\",\"o\":{\"k\":1},"
                       + "\"t\":\"0.0000001 1000000000000000000000 [] null\",\"z\":null}")
        // `{name}` names are Unicode alphanumerics.
        XCTAssertEqual(json("!active\nцена 5\nt {цена} {a\u{2014}b}\n"), "{\"t\":\"5 {a\u{2014}b}\",\"цена\":5}")
    }

    // 8 — :format: whole floats take integer patterns, strings pass, 64-bit stays exact.
    func test_format_patterns() {
        XCTAssertEqual(json("!active\nw:format:%05d 12.0\nw2:format:%5d 3.0\ns:format:%.2f \"3.14159\"\nbig:format:%d 9007199254740993\n"
                            + "e:format:%e 123456.789\ne2:format:%.2e 1234.5\nneg:format:%05d -42\n"),
                       "{\"big\":\"9007199254740993\",\"e\":\"1.23456789e+5\",\"e2\":\"1.23e3\",\"neg\":\"00-42\",\"s\":\"3.14159\","
                       + "\"w\":\"00012\",\"w2\":\"    3\"}")
    }

    // 9 — :sort: numbers numerically, other pairs by code point, stable; :sort:desc reverses.
    func test_sort_by_code_point() {
        XCTAssertEqual(json("!active\na:sort\n  - b\n  - A\n  - a\n  - Ä\n  - 10\n  - 9\n  - 9.5\nd:sort:desc\n  - 1\n  - 3\n  - 2\n"),
                       "{\"a\":[9,9.5,10,\"A\",\"a\",\"b\",\"Ä\"],\"d\":[3,2,1]}")
    }

    // 10 — :version reads leading integers positionally.
    func test_version_parts_are_positional() {
        XCTAssertEqual(json("!active\nv1:version:>=:1.2.3 1.2.3-beta\nv2:version:>:1.2 1.10\nv3:version:<:1.x.3 1.0.9\n"),
                       "{\"v1\":true,\"v2\":1.1,\"v3\":false}")
    }

    // 11 — [min]/[max] on strings count Unicode scalar values.
    func test_string_length_in_characters() {
        XCTAssertEqual(json("!active\nw[max:3] мир\nv[min:4] мир\n"),
                       "{\"v\":\"CONSTRAINT_ERR: 'v' value 3 is below min 4\",\"w\":\"мир\"}")
    }

    // 12 — indentation in UTF-8 bytes (U+00A0 is 2, U+3000 is 3); `|+` strips whole characters.
    func test_unicode_indentation() {
        XCTAssertEqual(json("a\n\tb\n\u{A0}c 1\nx\n\u{3000}y\n   z 1\nw\n\u{3000}y\n    z 1\ncode |+\n\u{A0}\u{A0}if x\n"
                            + "\u{A0}\u{A0}\u{A0}\u{A0}y\n \u{A0}\n"),
                       "{\"a\":{\"b\":{\"c\":1}},\"code\":\"if x\\n\u{A0}\u{A0}y\",\"w\":{\"y\":{\"z\":1}},\"x\":{\"y\":{},\"z\":1}}")
    }

    // 13 — list items: a new item pops the previous one's objects; an object opened
    // between a list key and an item is not the item's ancestor.
    func test_list_item_structure() {
        XCTAssertEqual(json("items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\nafter 1\n"),
                       "{\"after\":1,\"items\":[{\"name\":\"a\",\"sub\":{\"k\":1}},{\"name\":\"b\",\"v\":2}]}")
        XCTAssertEqual(json("items:join\n  x_1\n    a 1\n    - k v\n      m 2\nr\n  - 1\nr\n  - 2\n"),
                       "{\"items\":[{\"k\":\"v\",\"m\":2}],\"r\":[2],\"x_1\":{\"a\":1}}")
        // An item of a list whose key was re-declared is dropped — after the frames
        // at or deeper than its indent are popped, so `name` lands at the root.
        XCTAssertEqual(json("a\n    - base_hp 's'\n  a\n      x_1 [x]\n  - {cfg.v}\n   name\n"),
                       "{\"a\":{\"x_1\":\"[x]\"},\"name\":{}}")
        XCTAssertEqual(json("a\n    - x 1\n  a 5\n  - y\n   name\n"), "{\"a\":5,\"name\":{}}")
    }

    // 14, 19 — stringify writes text that reads back as the same data.
    func test_stringify_round_trips() {
        var o = SynxObject()
        o.set("a", to: .string("007"))
        o.set("b", to: .string("true"))
        o.set("c", to: .string("x # y"))
        o.set("d", to: .string("line1\n  line2"))
        o.set("n", to: .float(-0.0))
        o.set("e", to: .array([]))
        var one = SynxObject()
        one.set("k", to: .string("v"))
        o.set("l", to: .array([.object(one)]))
        o.set("t", to: .string("  pad"))
        o.set("u", to: .string("|+"))
        o.set("w", to: .string("say \"hi\" it's"))
        o.set("s", to: .secret("hunter2"))
        let text = Synx.stringify(o)
        XCTAssertEqual(text, "a \"007\"\nb \"true\"\nc |+\n  x # y\nd |+\n  line1\n    line2\ne:join\nl\n  - k v\n    k v\n"
                       + "n -0.0\ns [SECRET]\nt \"  pad\"\nu \"|+\"\nw say \"hi\" it's\n")
        var expected = o
        expected.set("s", to: .string("[SECRET]"))  // a secret is written as its mask
        XCTAssertEqual(Synx.parse(text), expected)
    }

    // 15 — format: `|+` bodies are content, keys by code point of their lowercase, stable.
    func test_format_keeps_blocks_and_sorts_by_code_point() {
        let src = "zeta 1\nИмя 2\nalpha 3\nимя2 4\ncode |+\n  zeta()\n    alpha()\n  beta\nb 1\nB 2\nb 3\n"
        let out = Synx.format(src)
        XCTAssertEqual(out, "alpha 3\nb 1\nB 2\nb 3\ncode |+\n  zeta()\n    alpha()\n  beta\n\nzeta 1\nИмя 2\nимя2 4\n")
        XCTAssertEqual(Synx.format(out), out)
        // CRLF lines are lines (Swift reads "\r\n" as one Character).
        XCTAssertEqual(Synx.format("b 1\r\na 2\r\n"), "a 2\nb 1\n")
    }

    // 16 — .synxb: string table by use count (deterministic), 64-bit integers exact.
    func test_synxb_payload_order_and_wide_integers() {
        let src = "x a\ny b\nz b\nw b\nbig 9007199254740993\nmin -9223372036854775808\n"
        let r = SynxParser.parse(src)
        let p1 = SynxBinary.encodePayload(r, resolved: false)
        XCTAssertEqual(p1, SynxBinary.encodePayload(SynxParser.parse(src), resolved: false))
        // String table: count 8, then "b" (3 uses) first.
        XCTAssertEqual(Array(p1.prefix(3)), [8, 1, 0x62])
        guard case .success(let back) = SynxBinary.decodePayload(flags: SynxBinary.flags(r, resolved: false), p1) else {
            return XCTFail("payload does not decode")
        }
        XCTAssertEqual(back.root["big"], .int(9007199254740993))
        XCTAssertEqual(back.root["min"], .int(Int64.min))
    }

    // Keys and strings compare as UTF-8 bytes, not by canonical equivalence.
    func test_keys_compare_as_bytes() {
        let o = Synx.parse("e\u{301} 1\n\u{E9} 2\n")
        XCTAssertEqual(o.count, 2)
        XCTAssertEqual(o["\u{E9}"], .int(2))
    }

    // Objects keep source order; JSON sorts on output.
    func test_objects_keep_source_order() {
        let o = Synx.parse("zeta 1\nalpha 2\nmid\n  y 1\n  b 2\n")
        XCTAssertEqual(o.keys, ["zeta", "alpha", "mid"])
        XCTAssertEqual(o["mid"]?.objectValue?.keys, ["y", "b"])
        var m = SynxMetadataTree()
        m["b"] = SynxMetaMap()
        m["a"] = SynxMetaMap()
        XCTAssertEqual(m.keys, ["b", "a"])
        m["b"] = nil
        XCTAssertEqual(m.keys, ["a"])
    }

    func test_version_constant() {
        XCTAssertEqual(Synx.version, "3.8.1")
    }

    // ── synx-core fixes after the 3.8.0 review ──────────────────────────────

    /// A fresh directory under the system temp dir.
    private func tempTree(_ name: String) throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(name)
        try? FileManager.default.removeItem(at: root)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        return root
    }

    private func put(_ root: URL, _ rel: String, _ text: String) throws {
        let url = root.appendingPathComponent(rel)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(text.utf8).write(to: url)
    }

    private func json(_ text: String, in dir: URL, _ options: SynxOptions = SynxOptions()) -> String {
        var o = options
        o.basePath = dir.path
        return Synx.toJSON(Synx.parseActive(text, options: o))
    }

    // `!use` stays inside synx_packages: no `..`, root, drive or `:` in the name or the manifest `main`.
    func test_use_package_paths_are_contained() throws {
        let t = try tempTree("synx-swift-use-contained")
        let base = t.appendingPathComponent("base")
        try put(t, "outside/src/main.synx", "leaked outside\n")
        try put(t, "secret.synx", "leaked secret\n")
        try put(base, "synx_packages/@a/evil/synx-pkg.synx", "main ../../../../secret.synx\n")
        try put(base, "synx_packages/@a/evil2/synx-pkg.synx", "main ../../../../secret.synx\n")
        try put(base, "synx_packages/@a/evil2/src/main.synx", "fallback src-main\n")
        try put(base, "synx_packages/@a/ok/synx-pkg.synx", "main ../x.synx\nentry lib\\entry.synx\n")
        try put(base, "synx_packages/@a/ok/lib/entry.synx", "good lib-entry\n")
        try put(base, "synx_packages/@a/abs/synx-pkg.synx", "main \(t.appendingPathComponent("secret.synx").path)\n")
        try put(base, "synx_packages/@a/abs/src/main.synx", "fallback abs-src\n")
        let skipped = "{\"x\":1}"
        XCTAssertEqual(json("!active\n!use @a/../../../outside as p\nx 1\n", in: base), skipped)
        XCTAssertEqual(json("!active\n!use @a\\..\\..\\..\\outside as p\nx 1\n", in: base), skipped)
        XCTAssertEqual(json("!active\n!use @a:b as p\nx 1\n", in: base), skipped)
        XCTAssertEqual(json("!active\n!use @a/evil as p\nx 1\n", in: base), skipped)
        XCTAssertEqual(json("!active\n!use @a/evil2 as p\nx 1\n", in: base), "{\"p\":{\"fallback\":\"src-main\"},\"x\":1}")
        XCTAssertEqual(json("!active\n!use @a/abs as p\nx 1\n", in: base), "{\"p\":{\"fallback\":\"abs-src\"},\"x\":1}")
        #if os(Windows)
        // A backslash is a separator only on Windows.
        XCTAssertEqual(json("!active\n!use @a/ok as p\nx 1\n", in: base), "{\"p\":{\"good\":\"lib-entry\"},\"x\":1}")
        #endif
    }

    // A leading BOM is dropped, not lost content.
    func test_bom_alone_is_not_truncation() {
        XCTAssertFalse(SynxParser.parse("\u{FEFF}name x\n").truncated)
        XCTAssertFalse(SynxParser.parse("\u{FEFF}").truncated)
        XCTAssertFalse(SynxParser.parse("name x\n").truncated)
    }

    // `:clamp` bounds must be finite numbers; anything else skips the marker.
    func test_clamp_bounds_must_be_finite() {
        XCTAssertEqual(json("!active\na:clamp:nan:5 3\nb:clamp:inf:5 3\nc:clamp:1:Infinity 3\nd:clamp:1e400:5 3\n"
                            + "e:clamp:5abc:9 3\nf:clamp:1:5 9\ng:clamp:-inf:5 3\nh:clamp:9:1 3\ni:clamp:1.5:2.5 x\n"),
                       "{\"a\":3,\"b\":3,\"c\":3,\"d\":3,\"e\":3,\"f\":5,\"g\":3,"
                       + "\"h\":\"CONSTRAINT_ERR: clamp min (9) > max (1)\",\"i\":\"x\"}")
    }

    // `format` past depth 128 drops the deeper subtree and goes on at the next shallower line.
    func test_format_drops_subtree_past_depth_cap() {
        var deep = "", kept = ""
        for i in 0..<140 { deep += String(repeating: " ", count: i) + "k\(i)\n" }
        for i in 0...128 { kept += String(repeating: " ", count: i * 2) + "k\(i)\n" }
        XCTAssertEqual(Synx.format(deep), kept)
        // The lines after the dropped subtree are formatted as usual.
        var more = ""
        for i in 0..<135 { more += String(repeating: " ", count: i) + "k\(i)\n" }
        let out = Synx.format(more + "  back 1\nzz 2\n  child 3\n")
        XCTAssertTrue(out.hasSuffix("\nzz 2\n  child 3\n"))
        XCTAssertTrue(out.contains("\n    back 1\n"))
    }

    // A file that includes itself stops at the first repeat: the text handed to
    // resolve has no path, the file it includes is open once, and including it
    // again from inside itself is the cycle (synx-core: `circular include`).
    func test_self_include_stops_at_the_first_repeat() throws {
        final class Counter: @unchecked Sendable {
            private let lock = NSLock()
            private var n = 0
            func tick() { lock.lock(); n += 1; lock.unlock() }
            var value: Int { lock.lock(); defer { lock.unlock() }; return n }
        }
        let t = try tempTree("synx-swift-self-include")
        try put(t, "self.synx", "!active\nme:include self.synx\nc:tick 1\n")
        let ticks = Counter()
        let options = SynxOptions(markerFns: ["tick": { _, _, v in ticks.tick(); return v }])
        XCTAssertEqual(json("!active\nme:include self.synx\nc:tick 1\n", in: t, options),
                       "{\"c\":1,\"me\":{\"c\":1,\"me\":\"INCLUDE_ERR: circular include: 'self.synx'\"}}")
        XCTAssertEqual(ticks.value, 2)
        // `{key:include}` still reads the only include when the pre-pass resolved it.
        try put(t, "common.synx", "name Common\n")
        XCTAssertEqual(json("!active\ndb:include common.synx\ng {name:include} {name:db}\n", in: t),
                       "{\"db\":{\"name\":\"Common\"},\"g\":\"Common Common\"}")
    }
}
