// SYNX 3.8.0 — one test per defect fixed in this port (numbers refer to the
// 3.8.0 changelog items). Expected texts come from synx-core 3.8.0.
#include "test_helpers.hpp"
#include "synx/synx.hpp"
#include "internal.hpp"
#include "engine_internal.hpp"

#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

using namespace synx;

namespace {

std::string json_of(const std::string& text) {
    ParseResult r = parse(text);
    if (r.mode == Mode::Active) resolve(r, Options{});
    return to_json(r.root);
}

} // namespace

// 1 — one pass in document order: a marker sees every key above it resolved.
SYNX_TEST(v380_resolution_follows_document_order) {
    EXPECT_EQ(json_of("!active\na:calc 2 * 3\nb\n  x v={a}\n  y:calc a + 1\nprice 100\ntax:calc price * 0.2\n"
                      "total:calc price + tax\n"),
              std::string("{\"a\":6,\"b\":{\"x\":\"v=6\",\"y\":7},\"price\":100,\"tax\":20,\"total\":120}"));
}

// 2 — every list item, every dotted key and every replacement has its own declarations.
SYNX_TEST(v380_declarations_belong_to_their_object) {
    EXPECT_EQ(json_of("!active\ns\n  - n a\n    p:env:default:7 SYNX_T_UNSET\n  - n b\n    p:default:8 null\n"
                      "  - n c\n    p null\n"),
              std::string("{\"s\":[{\"n\":\"a\",\"p\":7},{\"n\":\"b\",\"p\":8},{\"n\":\"c\",\"p\":null}]}"));
    EXPECT_EQ(json_of("!active\na.b\n  x:default:1 null\na\n  b\n    x null\n"),
              std::string("{\"a\":{\"b\":{\"x\":null}},\"a.b\":{\"x\":1}}"));
    EXPECT_EQ(json_of("!active\nport\n  port[enum:a|b] x\nport\nq\n  - k[min:5] 1\nq 3\n"),
              std::string("{\"port\":{},\"q\":3}"));
    // Metadata paths: list items as `#<index>`, a dot in a key escaped.
    ParseResult r = parse("!active\ns\n  - n a\n    p:env X\na.b\n  x:env Y\n");
    EXPECT_TRUE(r.metadata.contains("s.#0"));
    EXPECT_TRUE(r.metadata.contains("a\\.b"));
}

// 3 — registries from the live tree; the first type for a name wins.
SYNX_TEST(v380_type_registry_first_declaration_wins) {
    EXPECT_EQ(json_of("!active\na\n  x(int) 1\nb\n  x(string) 2\n"),
              std::string("{\"a\":{\"x\":1},\"b\":{\"x\":\"TYPE_ERR: 'x' expected int but got string\"}}"));
}

// 4 — :inherit merges parents left to right with their markers; a list child stays.
SYNX_TEST(v380_inherit_carries_markers_and_merges_parents) {
    EXPECT_EQ(json_of("!active\nbase\n  p:env:default:5 SYNX_T_UNSET\n  sub\n    k:default:9 null\nlst:inherit:base\n"
                      "  - 1\nkid:inherit:base:other\n  z 1\nother\n  p 6\n  w 2\n"),
              std::string("{\"base\":{\"p\":5,\"sub\":{\"k\":9}},\"kid\":{\"p\":6,\"sub\":{\"k\":9},\"w\":2,\"z\":1},"
                          "\"lst\":[1],\"other\":{\"p\":6,\"w\":2}}"));
}

// 5 — :calc: local key before root key, special keys, malformed literals, whole characters.
SYNX_TEST(v380_calc_scope_and_errors) {
    const std::string err = "CALC_ERR: SYNX :calc \xE2\x80\x94 ";
    EXPECT_EQ(json_of("!active\nlimit 5\nsvc\n  limit 100\n  price:calc limit * 2\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\n"
                      "bad:calc 1.2.3 + 1\nlone:calc -\nru:calc \xD1\x86\xD0\xB5\xD0\xBD\xD0\xB0 * 2\n"),
              "{\"bad\":\"" + err + "invalid number: '1.2.3'\",\"hp\":3,\"left\":7,\"limit\":5,\"lone\":\"" + err
                  + "invalid number: '-'\",\"max-hp\":10,\"ru\":\"" + err
                  + "unexpected character: '\xD1\x86' in expression\",\"svc\":{\"limit\":100,\"price\":200}}");
}

// 6 — a null value checks only [enum].
SYNX_TEST(v380_null_checks_only_enum) {
    EXPECT_EQ(json_of("!active\np[type:int]:env SYNX_T_UNSET\nm[enum:a|b]:env SYNX_T_UNSET\nok[enum:a|]:env SYNX_T_UNSET\n"
                      "n[min:3, pattern:^x]:env SYNX_T_UNSET\n"),
              std::string("{\"m\":\"CONSTRAINT_ERR: 'm' must be one of [a|b]\",\"n\":null,\"ok\":null,\"p\":null}"));
}

// 7 — values as text: plain decimal, a group is empty text, null is `null`.
SYNX_TEST(v380_values_as_text) {
    EXPECT_EQ(json_of("!active\nf 0.0000001\nbig 1e21\nh 1000000000000000000000.0\no\n  k 1\nz null\n"
                      "t {f} {h} [{o}] {z}\nl:join\n  - 2.5\n  - 0.0000001\n  - true\n"),
              std::string("{\"big\":\"1e21\",\"f\":1e-7,\"h\":1e21,\"l\":\"2.5,0.0000001,true\",\"o\":{\"k\":1},"
                          "\"t\":\"0.0000001 1000000000000000000000 [] null\",\"z\":null}"));
    // `{name}` names are Unicode alphanumerics.
    EXPECT_EQ(json_of("!active\n\xD1\x86\xD0\xB5\xD0\xBD\xD0\xB0 5\nt {\xD1\x86\xD0\xB5\xD0\xBD\xD0\xB0} {a\xE2\x80\x94"
                      "b}\n"),
              std::string("{\"t\":\"5 {a\xE2\x80\x94" "b}\",\"\xD1\x86\xD0\xB5\xD0\xBD\xD0\xB0\":5}"));
}

// 8 — :format: whole floats take integer patterns, strings pass, 64-bit stays exact.
SYNX_TEST(v380_format_patterns) {
    EXPECT_EQ(json_of("!active\nw:format:%05d 12.0\nw2:format:%5d 3.0\ns:format:%.2f \"3.14159\"\n"
                      "big:format:%d 9007199254740993\ne:format:%e 123456.789\ne2:format:%.2e 1234.5\n"
                      "neg:format:%05d -42\n"),
              std::string("{\"big\":\"9007199254740993\",\"e\":\"1.23456789e+5\",\"e2\":\"1.23e3\",\"neg\":\"00-42\","
                          "\"s\":\"3.14159\",\"w\":\"00012\",\"w2\":\"    3\"}"));
}

// 9 — :sort: numbers numerically, other pairs by code point, stable; :sort:desc reverses.
SYNX_TEST(v380_sort_by_code_point) {
    EXPECT_EQ(json_of("!active\na:sort\n  - b\n  - A\n  - a\n  - \xC3\x84\n  - 10\n  - 9\n  - 9.5\n"
                      "d:sort:desc\n  - 1\n  - 3\n  - 2\n"),
              std::string("{\"a\":[9,9.5,10,\"A\",\"a\",\"b\",\"\xC3\x84\"],\"d\":[3,2,1]}"));
}

// 10 — :version reads leading integers positionally.
SYNX_TEST(v380_version_parts_are_positional) {
    EXPECT_EQ(json_of("!active\nv1:version:>=:1.2.3 1.2.3-beta\nv2:version:>:1.2 1.10\nv3:version:<:1.x.3 1.0.9\n"),
              std::string("{\"v1\":true,\"v2\":1.1,\"v3\":false}"));
}

// 11 — [min]/[max] on strings count Unicode scalar values.
SYNX_TEST(v380_string_length_in_characters) {
    EXPECT_EQ(json_of("!active\nw[max:3] \xD0\xBC\xD0\xB8\xD1\x80\nv[min:4] \xD0\xBC\xD0\xB8\xD1\x80\n"),
              std::string("{\"v\":\"CONSTRAINT_ERR: 'v' value 3 is below min 4\",\"w\":\"\xD0\xBC\xD0\xB8\xD1\x80\"}"));
}

// 12 — indentation in UTF-8 bytes (U+00A0 is 2, U+3000 is 3); `|+` strips whole characters.
SYNX_TEST(v380_unicode_indentation) {
    EXPECT_EQ(json_of("a\n\tb\n\xC2\xA0" "c 1\nx\n\xE3\x80\x80" "y\n   z 1\nw\n\xE3\x80\x80" "y\n    z 1\ncode |+\n"
                      "\xC2\xA0\xC2\xA0" "if x\n\xC2\xA0\xC2\xA0\xC2\xA0\xC2\xA0" "y\n \xC2\xA0\n"),
              std::string("{\"a\":{\"b\":{\"c\":1}},\"code\":\"if x\\n\xC2\xA0\xC2\xA0" "y\",\"w\":{\"y\":{\"z\":1}},"
                          "\"x\":{\"y\":{},\"z\":1}}"));
}

// 13 — list items: a new item pops the previous one's objects; an object opened
// between a list key and an item is not the item's ancestor; a re-declared list
// starts a new array.
SYNX_TEST(v380_list_item_structure) {
    EXPECT_EQ(json_of("items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\nafter 1\n"),
              std::string("{\"after\":1,\"items\":[{\"name\":\"a\",\"sub\":{\"k\":1}},{\"name\":\"b\",\"v\":2}]}"));
    EXPECT_EQ(json_of("items:join\n  x_1\n    a 1\n    - k v\n      m 2\nr\n  - 1\nr\n  - 2\n"),
              std::string("{\"items\":[{\"k\":\"v\",\"m\":2}],\"r\":[2],\"x_1\":{\"a\":1}}"));
    // An item of a list whose key was re-declared is dropped — after the frames
    // at or deeper than its indent are popped, so `name` lands at the root.
    EXPECT_EQ(json_of("a\n    - base_hp 's'\n  a\n      x_1 [x]\n  - {cfg.v}\n   name\n"),
              std::string("{\"a\":{\"x_1\":\"[x]\"},\"name\":{}}"));
    EXPECT_EQ(json_of("a\n    - x 1\n  a 5\n  - y\n   name\n"), std::string("{\"a\":5,\"name\":{}}"));
}

// 14, 19 — stringify writes text that reads back as the same data.
SYNX_TEST(v380_stringify_round_trips) {
    Object obj;
    obj.push_back(Pair{"a", Value::make_string("007")});
    obj.push_back(Pair{"b", Value::make_string("true")});
    obj.push_back(Pair{"c", Value::make_string("x # y")});
    obj.push_back(Pair{"d", Value::make_string("line1\n  line2")});
    obj.push_back(Pair{"n", Value::make_float(-0.0)});
    obj.push_back(Pair{"e", Value::make_array()});
    Object one;
    one.push_back(Pair{"k", Value::make_string("v")});
    Array l;
    l.push_back(Value::make_object(one));
    obj.push_back(Pair{"l", Value::make_array(l)});
    obj.push_back(Pair{"t", Value::make_string("  pad")});
    obj.push_back(Pair{"u", Value::make_string("|+")});
    obj.push_back(Pair{"w", Value::make_string("say \"hi\" it's")});
    obj.push_back(Pair{"s", Value::make_secret("hunter2")});
    const Value v = Value::make_object(obj);
    const std::string text = stringify(v);
    EXPECT_EQ(text, std::string("a \"007\"\nb \"true\"\nc |+\n  x # y\nd |+\n  line1\n    line2\ne:join\n"
                                "l\n  - k v\n    k v\nn -0.0\ns [SECRET]\nt \"  pad\"\nu \"|+\"\nw say \"hi\" it's\n"));
    ParseResult back = parse(text);
    Object expected = obj;
    expected.back().value = Value::make_string("[SECRET]");  // a secret is written as its mask
    EXPECT_TRUE(back.root.equals(Value::make_object(expected)));
}

// 15 — format: `|+` bodies are content, keys by code point of their lowercase, stable.
SYNX_TEST(v380_format_keeps_blocks_and_sorts_by_code_point) {
    const std::string src = "zeta 1\n\xD0\x98\xD0\xBC\xD1\x8F 2\nalpha 3\n\xD0\xB8\xD0\xBC\xD1\x8F" "2 4\ncode |+\n"
                            "  zeta()\n    alpha()\n  beta\nb 1\nB 2\nb 3\n";
    const std::string out = format(src);
    EXPECT_EQ(out, std::string("alpha 3\nb 1\nB 2\nb 3\ncode |+\n  zeta()\n    alpha()\n  beta\n\nzeta 1\n"
                               "\xD0\x98\xD0\xBC\xD1\x8F 2\n\xD0\xB8\xD0\xBC\xD1\x8F" "2 4\n"));
    EXPECT_EQ(format(out), out);
}

// 16 — .synxb: string table by use count (deterministic), 64-bit integers exact.
SYNX_TEST(v380_synxb_payload_order_and_wide_integers) {
    ParseResult r = parse("x a\ny b\nz b\nw b\nbig 9007199254740993\nmin -9223372036854775808\n");
    const std::vector<uint8_t> p1 = detail::encode_synxb_payload(r, false);
    const std::vector<uint8_t> p2 = detail::encode_synxb_payload(parse("x a\ny b\nz b\nw b\nbig 9007199254740993\n"
                                                                       "min -9223372036854775808\n"), false);
    EXPECT_TRUE(p1 == p2);
    // String table: count 8, then "b" (3 uses) first.
    EXPECT_TRUE(p1.size() > 3 && p1[0] == 8 && p1[1] == 1 && p1[2] == 'b');
    auto back = detail::decode_synxb_payload(detail::synxb_flags(r, false), p1.data(), p1.size());
    EXPECT_TRUE(back.ok());
    const Value* big = back.value().root.get("big");
    const Value* min = back.value().root.get("min");
    EXPECT_TRUE(big && big->is_int() && *big->as_int() == 9007199254740993LL);
    EXPECT_TRUE(min && min->is_int() && *min->as_int() == INT64_MIN);
}

// Objects keep source order; JSON sorts on output.
SYNX_TEST(v380_objects_keep_source_order) {
    ParseResult r = parse("zeta 1\nalpha 2\nmid\n  y 1\n  b 2\n");
    const Object& o = *r.root.as_object();
    EXPECT_EQ(o[0].key, std::string("zeta"));
    EXPECT_EQ(o[1].key, std::string("alpha"));
    EXPECT_EQ((*o[2].value.as_object())[0].key, std::string("y"));
    MetadataTree m;
    m["b"];
    m["a"];
    m["b"].insert_or_assign("k", Meta{});
    EXPECT_EQ(m.begin()->first, std::string("b"));
    EXPECT_TRUE(m.erase("b"));
    EXPECT_EQ(m.begin()->first, std::string("a"));
}

// ── synx-core fixes after 3.8.0 review ─────────────────────────────────────

namespace {

namespace fs = std::filesystem;

// A fresh directory under the system temp dir, removed again on scope exit.
struct TempTree {
    fs::path root;
    explicit TempTree(const char* name) : root(fs::temp_directory_path() / name) {
        std::error_code ec;
        fs::remove_all(root, ec);
        fs::create_directories(root);
    }
    ~TempTree() {
        std::error_code ec;
        fs::remove_all(root, ec);
    }
    void put(const std::string& rel, const std::string& text) const {
        fs::path p = root / detail::path_from_utf8(rel);
        fs::create_directories(p.parent_path());
        std::ofstream(p, std::ios::binary) << text;
    }
    std::string json(const std::string& rel_dir, const std::string& text, Options o = Options{}) const {
        o.base_path = detail::path_to_utf8(root / detail::path_from_utf8(rel_dir));
        ParseResult r = parse(text);
        resolve(r, o);
        return to_json(r.root);
    }
};

} // namespace

// `!use` stays inside synx_packages: no `..`, root, drive or `:` in the name or the manifest `main`.
SYNX_TEST(v380_use_package_paths_are_contained) {
    TempTree t("synx-cpp-use-contained");
    t.put("outside/src/main.synx", "leaked outside\n");
    t.put("secret.synx", "leaked secret\n");
    t.put("base/synx_packages/@a/evil/synx-pkg.synx", "main ../../../../secret.synx\n");
    t.put("base/synx_packages/@a/evil2/synx-pkg.synx", "main ../../../../secret.synx\n");
    t.put("base/synx_packages/@a/evil2/src/main.synx", "fallback src-main\n");
    t.put("base/synx_packages/@a/ok/synx-pkg.synx", "main ../x.synx\nentry lib\\entry.synx\n");
    t.put("base/synx_packages/@a/ok/lib/entry.synx", "good lib-entry\n");
    t.put("base/synx_packages/@a/abs/synx-pkg.synx",
          "main " + detail::path_to_utf8(t.root / "secret.synx") + "\n");
    t.put("base/synx_packages/@a/abs/src/main.synx", "fallback abs-src\n");
    const std::string skipped = "{\"x\":1}";
    EXPECT_EQ(t.json("base", "!active\n!use @a/../../../outside as p\nx 1\n"), skipped);
    EXPECT_EQ(t.json("base", "!active\n!use @a\\..\\..\\..\\outside as p\nx 1\n"), skipped);
    EXPECT_EQ(t.json("base", "!active\n!use @a:b as p\nx 1\n"), skipped);
    EXPECT_EQ(t.json("base", "!active\n!use @a/evil as p\nx 1\n"), skipped);
    EXPECT_EQ(t.json("base", "!active\n!use @a/evil2 as p\nx 1\n"),
              std::string("{\"p\":{\"fallback\":\"src-main\"},\"x\":1}"));
    EXPECT_EQ(t.json("base", "!active\n!use @a/abs as p\nx 1\n"),
              std::string("{\"p\":{\"fallback\":\"abs-src\"},\"x\":1}"));
#if defined(_WIN32)
    // A backslash is a separator only on Windows.
    EXPECT_EQ(t.json("base", "!active\n!use @a/ok as p\nx 1\n"),
              std::string("{\"p\":{\"good\":\"lib-entry\"},\"x\":1}"));
#endif
}

// A leading BOM is dropped, not lost content.
SYNX_TEST(v380_bom_alone_is_not_truncation) {
    EXPECT_FALSE(parse("\xEF\xBB\xBFname x\n").truncated);
    EXPECT_FALSE(parse("\xEF\xBB\xBF").truncated);
    EXPECT_FALSE(parse("name x\n").truncated);
}

// `:clamp` bounds must be finite numbers; anything else skips the marker.
SYNX_TEST(v380_clamp_bounds_must_be_finite) {
    EXPECT_EQ(json_of("!active\na:clamp:nan:5 3\nb:clamp:inf:5 3\nc:clamp:1:Infinity 3\nd:clamp:1e400:5 3\n"
                      "e:clamp:5abc:9 3\nf:clamp:1:5 9\ng:clamp:-inf:5 3\nh:clamp:9:1 3\ni:clamp:1.5:2.5 x\n"),
              std::string("{\"a\":3,\"b\":3,\"c\":3,\"d\":3,\"e\":3,\"f\":5,\"g\":3,"
                          "\"h\":\"CONSTRAINT_ERR: clamp min (9) > max (1)\",\"i\":\"x\"}"));
}

// `format` past depth 128 drops the deeper subtree and goes on at the next shallower line.
SYNX_TEST(v380_format_drops_subtree_past_depth_cap) {
    std::string deep, kept;
    for (int i = 0; i < 140; ++i) deep += std::string(static_cast<size_t>(i), ' ') + "k" + std::to_string(i) + "\n";
    for (int i = 0; i <= 128; ++i) kept += std::string(static_cast<size_t>(i) * 2, ' ') + "k" + std::to_string(i) + "\n";
    EXPECT_EQ(format(deep), kept);
    // The lines after the dropped subtree are formatted as usual.
    std::string more;
    for (int i = 0; i < 135; ++i) more += std::string(static_cast<size_t>(i), ' ') + "k" + std::to_string(i) + "\n";
    more += "  back 1\nzz 2\n  child 3\n";
    const std::string out = format(more);
    const std::string tail = "\nzz 2\n  child 3\n";
    EXPECT_TRUE(out.size() > tail.size() && out.compare(out.size() - tail.size(), tail.size(), tail) == 0);
    EXPECT_TRUE(out.find("\n    back 1\n") != std::string::npos);
}

// A file that includes itself stops at the first repeat: the text handed to
// resolve has no path, the file it includes is open once, and including it
// again from inside itself is the cycle (synx-core: `circular include`).
SYNX_TEST(v380_self_include_stops_at_the_first_repeat) {
    TempTree t("synx-cpp-self-include");
    t.put("self.synx", "!active\nme:include self.synx\nc:tick 1\n");
    int ticks = 0;
    Options o;
    o.marker_fns["tick"] = [&ticks](const std::string&, const std::vector<std::string>&, const Value& v) {
        ++ticks;
        return v;
    };
    EXPECT_EQ(t.json(".", "!active\nme:include self.synx\nc:tick 1\n", o),
              std::string("{\"c\":1,\"me\":{\"c\":1,\"me\":\"INCLUDE_ERR: circular include: 'self.synx'\"}}"));
    EXPECT_EQ(ticks, 2);
    // `{key:include}` still reads the only include when the pre-pass resolved it.
    t.put("common.synx", "name Common\n");
    EXPECT_EQ(t.json(".", "!active\ndb:include common.synx\ng {name:include} {name:db}\n"),
              std::string("{\"db\":{\"name\":\"Common\"},\"g\":\"Common Common\"}"));
}
