# SYNX — native Java parser

`parsers/java` is the **native** SYNX parser for the JVM, written in pure Java 17.
No FFI to the Rust `synx-core` crate, no `synx-c` cdylib at runtime. Implements
**SYNX 3.8.0** as `crates/synx-core` does it: parse, `!active` resolution,
`.synxb`, canonical JSON, stringify, diff, format and `!tool` give the
reference's output byte for byte (`ConformanceTest` replays
`tests/conformance`).

Targets:

* **JVM 17+** (LTS — supported through 2029). Kotlin, Scala, Groovy, Clojure
  consumers reuse this jar directly — no separate per-language port needed.
* **Android** API level 26+ (when paired with desugaring for records/sealed),
  but the recommended Android path is still `bindings/kotlin` over this jar.

| Property | Value |
|---|---|
| Source | Java 17 (sealed interfaces + records) |
| Build | Maven 3.6+ |
| Tests | JUnit 5 |
| Public deps | None (only `java.base`, `java.util.zip`) |
| Wire-compat with Rust | yes (canonical JSON, `.synxb` payload) |
| Version | `Synx.VERSION` = `3.8.0` |

## Quick start

```bash
mvn -f parsers/java/pom.xml test
mvn -f parsers/java/pom.xml package  # builds target/synx-3.8.0.jar
```

Maven coordinate:

```xml
<dependency>
    <groupId>com.aperturesyndicate</groupId>
    <artifactId>synx</artifactId>
    <version>3.8.0</version>
</dependency>
```

## API at a glance

```java
import com.aperturesyndicate.synx.*;

// Static parse
SynxObject root = Synx.parse("name App\nport 8080\n");

// Active engine with env vars
SynxOptions opts = new SynxOptions();
opts.env = Map.of("PORT", "9090");
SynxObject active = Synx.parseActive("!active\nport:env:default:3000 PORT\n", opts);

// Canonical JSON
String json = Synx.toJson(active);

// Binary
var compiled = Synx.compile("name App\n", false);
if (compiled.ok) {
    var decompiled = Synx.decompile(compiled.value);
}
```

### Custom markers

```java
SynxOptions opts = new SynxOptions();
opts.markerFns.put("upper", (key, args, value) -> {
    if (value instanceof SynxValue.Str s) {
        return SynxValue.ofString(s.value().toUpperCase());
    }
    return value;
});
SynxObject r = Synx.parseActive("!active\nslug:upper hello", opts);
```

## Layout

```
parsers/java/
├── pom.xml
├── src/main/java/com/aperturesyndicate/synx/
│   ├── Synx.java               # top-level facade
│   ├── SynxValue.java          # sealed interface + records
│   ├── SynxObject.java         # insertion-ordered map
│   ├── SynxMeta.java           # markers / args / type hint / constraints
│   ├── SynxConstraints.java
│   ├── SynxOptions.java
│   ├── SynxMarkerFn.java       # functional interface
│   ├── SynxMode.java
│   ├── SynxParseResult.java
│   ├── SynxIncludeDirective.java
│   ├── SynxUseDirective.java
│   ├── SynxParser.java         # text → tree
│   ├── SynxEngine.java         # !active resolver: document-order walk, markers, inheritance
│   ├── SynxNumbers.java        # Rust number grammar and number text (Display, ryu)
│   ├── SynxCalc.java
│   ├── SynxJson.java
│   ├── SynxStringify.java
│   ├── SynxFormatter.java
│   ├── SynxDiff.java
│   └── SynxBinary.java
└── src/test/java/com/aperturesyndicate/synx/
    ├── SynxValueTest.java
    ├── SynxParserTest.java
    ├── SynxCalcTest.java
    ├── SynxJsonTest.java
    ├── SynxStringifyTest.java
    ├── SynxDiffTest.java
    ├── SynxBinaryTest.java
    ├── SynxEngineTest.java
    ├── Release380Test.java     # the 3.8.0 defects
    └── ConformanceTest.java
```

## Limits (parity with Rust)

| Cap | Value |
|---|---|
| Input bytes | 16 MiB |
| Indexed line starts | 2 million |
| Parse nesting depth | 128 |
| Multiline block | 1 MiB |
| List items per list | 1,048,576 |
| `!include` directives | 4096 |
| Marker chain segments | 512 |
| Engine resolve depth | 512 |
| Serialize / JSON depth | 128 |
| Constraint enum parts | 4096 |

## Active mode

Resolution follows synx-core: one pass per object in document order — for
each key, first its child object (or each object item of its list), then the
key's own markers, then `{…}` interpolation over the object — so a marker sees
every key above it already resolved, at any depth. `SynxParseResult.metadata`
keeps both levels in declaration order and files declarations by the object
that holds the key: a list item is `list.#0`, and a `.` or `\` inside a key is
escaped (`a\.b`). Indentation and the byte caps are measured in UTF-8 bytes
(U+00A0 counts 2, U+3000 counts 3), not UTF-16 units.

## What's unsupported (versus Rust core)

* **SYNXL** (`.synxl`) is not implemented — see `../README.md`.
* **WASM custom markers** — replaced by `SynxMarkerFn` functional interface,
  applied after the built-in markers, before the key's constraints. Builtin
  markers always win over custom ones with the same name.
* **`[pattern]`** runs on `java.util.regex` with `UNICODE_CHARACTER_CLASS`.
  Constructs the reference's `regex` crate refuses (look-around,
  back-references, atomic groups, possessive quantifiers, nested quantifiers)
  are skipped as there; other dialect edges (`$` before a final line break,
  some class syntax) can still differ.
* **`:signing`** — Ed25519 signature verification not ported.
* `:vision` / `:audio` are passthrough envelopes; consumers attach actual
  media bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output (shortest
  round-trip floats in ryu's layout, keys in code point order). Shortest
  digits come from `Double.toString` on JDK 19+ and from an exact search on
  JDK 17/18.
* **`.synxb`**: identical magic / version / flags and an identical
  uncompressed payload — the string table is ordered by use count as in
  synx-core, so one document always compiles to the same bytes. Files written
  here decompile under Rust / C# / Swift / GDScript / C++ and vice-versa.
  Compression goes through `java.util.zip.Deflater(level=9, nowrap=true)`; its
  compressed bytes differ from `miniz_oxide`'s, the decoded payload does not.
* **Stringify / decompile**: the SYNXL §14.1 block writer of synx-core —
  strings that would read back as another type or a comment are quoted, `|+`
  bodies are kept, secrets are written as `[SECRET]`.
* **Canonical text format**: same sort key (code point order of the
  lowercased key, stable) and indentation rules as the Rust `fmt_canonical`;
  a `|` / `|+` body is content and is never sorted.
