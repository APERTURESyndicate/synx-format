# SYNX — native Java parser

`parsers/java` is the **native** SYNX parser for the JVM, written in pure Java 17.
No FFI to the Rust `synx-core` crate, no `synx-c` cdylib at runtime. Mirrors the
canonical Rust engine 1:1 (parse / `!active` / `.synxb` / canonical JSON / diff
/ format / `!tool`).

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
| Wire-compat with Rust | yes (canonical JSON + `.synxb` raw DEFLATE) |

## Quick start

```bash
mvn -f parsers/java/pom.xml test
mvn -f parsers/java/pom.xml package  # builds target/synx-3.6.2.jar
```

Maven coordinate:

```xml
<dependency>
    <groupId>com.aperturesyndicate</groupId>
    <artifactId>synx</artifactId>
    <version>3.6.2</version>
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
│   ├── SynxEngine.java         # !active resolver + all 27 markers
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

## What's unsupported (versus Rust core)

* **WASM custom markers** — replaced by `SynxMarkerFn` functional interface.
  Builtin markers always win over custom ones with the same name.
* **`:signing`** — Ed25519 signature verification not ported.
* `:vision` / `:audio` are passthrough envelopes; consumers attach actual
  media bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output. Floats use
  `%.17g` with a mandatory `.0` suffix.
* **`.synxb`**: identical magic / version / flags / string table / DEFLATE
  payload as `crates/synx-core`. Files written here decompile under
  Rust / C# / Swift / GDScript / C++ and vice-versa. Compression goes through
  `java.util.zip.Deflater(level=9, nowrap=true)` to match Rust
  `miniz_oxide::deflate::compress_to_vec(_, 9)`.
* **Canonical text format**: same sort key and indentation rules as the Rust
  `fmt_canonical`.
