# Changelog

All notable changes to this repository are documented in this file.

> **Historical npm numbering note (3.6.x only):** For the 3.6.x release
> line, the `@aperturesyndicate/synx-format` npm package led by one
> patch — e.g. the "real" 3.6.2 release shipped to npm as `3.6.3` —
> because npm's `3.6.2` slot had once been taken and unpublished, and
> npm policy bans reusing a version number. All other registries
> (crates.io, NuGet, Maven Central, pub.dev, the Visual Studio
> Marketplace) and the GitHub release tag stayed on the "real" version
> number for those releases; both slots shipped identical content.
> **Starting with 3.7.0, this divergence is resolved:** the npm slot
> for `3.7.0` was free, so npm and every other registry — plus the
> git tag — now publish on the same version number.

## [3.8.0] — 2026-10-17

A speed and parity release. Every parser path was measured against 3.7.4 on
the same corpus, rewritten where the time went, and checked by differential
fuzzing — 3.7.4 against 3.8.0, Rust against TypeScript, and each writer
against its own reader. The fuzzing and review found 55 defects; all are fixed, and
synx-core and TypeScript now agree on every one of more than 70 000 generated
documents, static and active.

### Speed

Best of interleaved runs on one machine (Windows 11, system allocator unless
noted), 3.7.4 → 3.8.0, same inputs, output checked identical. `bench-config`
is a 2.5 KB service config; `static-mid` / `static-large` are generated
documents of 134 KB and 4 MB; `active-mid` / `active-large` are 36 KB and
1.2 MB with markers; `real` is the 46 production configs of the
APERTURESyndicate platform, parsed in turn.

**Parse** (`!active` documents include resolution):

| Document | synx-core | TypeScript (Node 24) | TypeScript (Bun 1.3) | Python binding |
|---|---|---|---|---|
| bench-config | 38.6 → 17.6 µs **×2.19** | 34.0 → 20.4 µs **×1.66** | 43.1 → 25.8 µs **×1.67** | 50.3 → 17.6 µs **×2.86** |
| static-mid | 1.78 → 0.97 ms **×1.84** | 1.70 → 1.24 ms **×1.37** | 2.06 → 1.33 ms **×1.55** | 2.48 → 1.16 ms **×2.13** |
| static-large | 60.1 → 35.3 ms **×1.70** | 53.9 → 38.2 ms **×1.41** | 68.3 → 42.6 ms **×1.60** | — |
| active-mid | 2.08 → 0.57 ms **×3.64** | 1.23 → 0.86 ms **×1.43** | 2.10 → 1.26 ms **×1.66** | — |
| active-large | 81.0 → 22.5 ms **×3.61** | 297.5 → 31.1 ms **×9.56** | 331.0 → 49.2 ms **×6.73** | — |
| real, 46 configs | 3.57 → 1.47 ms **×2.43** | 2.75 → 2.20 ms **×1.25** | 3.76 → 2.46 ms **×1.53** | 4.12 → 1.51 ms **×2.72** |

**SYNXL, JSON and `.synxb`** (`flat-100k` is 100 000 seven-field records,
7 MB; `blocks-20k` is 20 000 records with `[block]` fields, 2.9 MB):

| Operation | synx-core | TypeScript (Node 24) |
|---|---|---|
| SYNXL read, flat-100k | 69.6 → 74.3 ms **×0.94** · with mimalloc 37.0 → 36.0 ms **×1.03** | 130.4 → 76.3 ms **×1.71** |
| SYNXL read, blocks-20k | 60.3 → 40.8 ms **×1.48** | 84.2 → 68.0 ms **×1.24** |
| SYNXL write, flat-100k | 104.2 → 55.6 ms **×1.88** | 131.0 → 79.6 ms **×1.64** |
| SYNXL write, blocks-20k | 19.9 → 13.3 ms **×1.50** | 34.8 → 31.7 ms **×1.10** |
| SYNXL read + records, Python | 173.7 → 83.2 ms **×2.09** | — |
| JSON projection, static-large | 11.9 → 7.6 ms **×1.57** | 113.1 → 53.7 ms **×2.11** (Bun) |
| `.synxb` compile, static-large | 86.7 → 52.5 ms **×1.65** | 131.7 → 82.2 ms **×1.60** |
| `.synxb` decompile, static-large | 42.0 → 33.3 ms **×1.26** | **×0.73** — see below |
| `.synxb` size, static-large | 92.0 → 89.0 KB | 144.6 → 86.0 KB |
| `.synxb` size, active-large | 210.7 → 169.3 KB | — |

Two numbers go down. synx-core's flat SYNXL read makes one more allocation
per record — an `IndexMap` keeps its entries and its index in two buffers —
and the Windows system allocator charges ~50 ns for it; with mimalloc the
read is level with 3.7.4, and the compact strings planned for 4.0 make it
×1.49. TypeScript's `.synxb` decompile now writes through the round-trip
writer, which quotes what must be quoted; 3.7.4 was faster because it wrote
text that read back as different data.

Where it came from:

- **synx-core.** Objects are `IndexMap` with the `foldhash` hasher instead of
  `HashMap` with SipHash: faster, and in source order, which also made
  resolution deterministic (see below). The parser borrows keys and values
  from the input instead of copying each line, trims with an ASCII fast path
  and finds line ends with `memchr`; the engine classifies a marker chain once
  into a bitmask instead of ~35 string scans per key, walks with one path
  buffer, caches compiled `[pattern]`s, and substitutes `:calc` names by token
  instead of rescanning the expression per key. Validators work in place.
- **TypeScript.** Lines are walked as offsets into the text instead of split
  and trimmed copies; `:calc` substitution by token removed a quadratic pass
  (the ×8–×10 on `active-large`); integers up to 15 digits skip the BigInt
  path; the JSON writer escapes only when a string needs it.
- **SYNXL.** Rust writes floats in place and checks quoting without two
  allocations per cell; TypeScript reads cells with a cursor instead of slicing,
  builds block arrays only when a record has a block, caches the writer's
  inline text and compiles each `[pattern]` once.
- **`.synxb`.** The string table is ordered by use count, so frequent strings
  get one-byte indexes: files are 3–20 % smaller in Rust and up to 40 % in
  TypeScript, and — unlike 3.7.4, whose output depended on hash order — the
  same document always compiles to the same bytes.
- **Bindings.** The Node and Python extensions use mimalloc: allocation was
  nearly half of parse time with the Windows system allocator. Python also
  reuses key strings within one conversion.

### Security

- **`!use` could read outside the packages directory.** synx-core joined the
  package name and the manifest's `main` / `wasm` paths onto
  `synx_packages/` as written, so `!use @a/../../secret` parsed
  `<anywhere>/src/main.synx` into the document, and a published package could
  point its `main` at any file — the escape `!include` already refused. Both
  must now be relative paths with no root, drive or `..` segment; a package
  directory that is itself a symlink keeps working. TypeScript does not load
  `!use` packages.
- **A hostile document could stop synx-core:**
  - `:clamp:nan:5` panicked (`f64::clamp` with a NaN bound). A bound must now
    be a finite number in the Rust grammar, otherwise the marker is skipped —
    TypeScript skipped NaN already, but read `5abc` as 5 and `Infinity` as a
    bound; both implementations now agree.
  - A file that `:include`s itself overflowed the stack: the pre-pass that
    registers `:include` keys for `{key:alias}` resolved the file without the
    depth cap every other include path has. Each level was also resolved
    twice, so a chain of includes cost 2^depth resolutions; the marker now
    reuses the pre-pass result.
  - `format` looped forever on a document nested deeper than its 128-level
    cap (Rust and TypeScript alike): the deeper subtree was dropped without
    consuming its lines.

### Active mode

- **Markers resolve in document order, deterministically.** synx-core walked
  objects in hash order: `tax:calc price * 0.2` / `total:calc price + tax`
  failed with `CALC_ERR` in about half of runs, an `:alias` of an `:env` key
  returned either the value or the variable's name, and when one field was
  declared with two types the winner was random. Nested objects were also
  resolved before their parent's markers, so `x {a}` inside an object saw an
  unresolved `a`. Rust now follows TypeScript: one pass in document order.
- **Every object keeps its own declarations.**
  - Markers inside list items were never applied in synx-core; after that was
    fixed, all items still shared one declaration per key, so `port[max:9]` in
    the third server replaced `port:env:default:7001` of the first.
  - A key containing a dot (`a.b`) shared its markers with `b` inside `a`.
  - A key that replaced an object or a list kept the replaced subtree's
    markers and constraints.
  - Type and constraint registries counted declarations of objects that had
    since been overwritten.
- **`:inherit`** carries each field's markers from the object its value came
  from (synx-core passed the parent's `:env` variable *name* as the value),
  merges several parents left to right in both implementations, copies nested
  subtrees instead of sharing them (TypeScript), and leaves a list or scalar
  child as written instead of replacing it with `{}` (synx-core).
- **`:calc`** — a name refers to the key of the same object first, then to a
  root key (synx-core looked at the root first). Errors read the same in both
  implementations; synx-core no longer garbles non-ASCII text in them, and
  TypeScript rejects malformed literals (`1.2.3`, a lone `-`) instead of
  reading a prefix.
- **Constraints on `null`** check only `[enum]` in both (synx-core reported
  `CONSTRAINT_ERR` for `[type]`, `[min]`, `[max]` and `[pattern]`).
- **Values in text** read the same everywhere: a number is plain decimal
  (TypeScript put `1e-7` and `1e+21` into interpolation, `:join`, `:calc`
  operands and enum checks where synx-core writes `0.0000001`), a group or a
  list is empty text and `null` is `null` (TypeScript wrote
  `[object Object]`).
- **`:format`** — a whole float takes an integer pattern (`12.0` under `%05d`
  is `00012`; synx-core printed `12`); TypeScript no longer formats a quoted
  number or an object, and keeps a 64-bit integer exact under `%d`.
- **`:sort`** compares text by code point in both; TypeScript used
  `localeCompare`, so the order depended on the machine's locale.
- **`:version`** reads `1.2.3-beta` as 1.2.3 (synx-core dropped the third part).
- TypeScript applies `:clamp`, `:round`, `:sum`, `:sort` and `:calc` to integers
  above 2^53, which it used to skip.

### Parsing

- **List items.** synx-core dropped the keys of an item that followed an item
  holding a nested object, and lost keys when an object opened between a list
  key and its first item. TypeScript kept a list on the frame stack, so a
  deeper non-item key became a property of the array that no serializer
  emits; a scalar item left the previous item open; and items after a
  re-declared list key went into a detached array, without closing the
  frames above them.
- **Unicode whitespace** follows SYNX §4 in TypeScript as it did in synx-core:
  indentation is measured in UTF-8 (U+00A0 counts 2, U+3000 counts 3 — a tab
  and an NBSP were the same depth in TypeScript and different depths in Rust,
  so the two built different trees), trailing Unicode whitespace is trimmed
  from keys, values and `|` lines, a line of U+00A0 is blank to look-ahead,
  and the `|+` prefix is cut by whole characters.
- **`(float)`** in TypeScript reads the whole value with the Rust float
  grammar instead of `parseFloat`'s prefix (`2 + 2` was `2`).
- synx-core set `truncated` on every document that starts with a BOM; the
  dropped U+FEFF is not lost content.

### Writers: stringify, `.synxb`, format

- **`Synx.stringify` and `.synxb` decompile now round-trip.** In 3.7.4, 1 777 of
  3 000 random documents came back different through `.synxb`: strings that
  read as other types were written bare (`"007"` → `7`, `"true"` → `true`),
  `|+` was written as `|` (code lost its indentation), and objects inside list
  items became text. Both implementations now write through the SYNXL §14.1
  writer, which quotes such strings, keeps `|+`, repeats a list item's head
  when needed, and masks secrets. Property fuzz: 0 of 3 000 differ, in both.
- **`format`** treated the lines of a `|+` body as child keys and sorted them,
  rewriting the code or prompt text inside. Both implementations now keep the
  body and its relative indentation; the result is data-preserving,
  idempotent and byte-identical between Rust and TypeScript. TypeScript sorted
  keys with `localeCompare`, overflowed the stack on 100 000+ siblings and
  had no depth cap; synx-core's sort was unstable.
- **TypeScript `.synxb`** wrote nothing for integers above 2^53, producing a
  corrupt file, and rounded them on read.

### SYNXL

- A string's `[min]`/`[max]` length counts Unicode characters in both readers
  (synx-core counted bytes, TypeScript UTF-16 units).
- The `[pattern]` length cap is 256 in TypeScript, as in synx-core and the
  active engine (it was 128).
- A typed cell keeps U+00A0 as data (§7.4): `5` followed by U+00A0 under
  `(int)` is `CastFailed` in TypeScript too.
- `UnknownBlockKey` and `BlockFieldNotDeclared` report the line inside the block
  that declares the key, as decided by the SYNX parser itself. Both readers
  approximated it from indentation and fell back to the record line for a
  deeper line after a key with a value; TypeScript also disagreed with Rust
  on U+3000 indents.
- Writer: an empty list is written as `key:join` (TypeScript wrote a bare key
  that read back as `{}`), a one-key object in a list round-trips, and `-0.0`
  keeps its sign.

### Known limitations

SYNX keeps one open list at a time (§8.5 of the frozen 3.6 core), so some
data has no text form that reads back the same. `stringify` and `.synxb`
decompile write such data, and the reader then drops or reshapes it:

- a list inside a list item, followed by another item of the outer list —
  the later items are dropped;
- an empty object or an empty list as the last entry of a list item,
  followed by another item — the next item is read into it;
- trailing whitespace on a `|+` line, and a key that ends in Unicode
  whitespace or reads as a directive (`!active`).

The port sessions measured about 5 % of random documents affected. Lifting
the first two needs a change to the frozen core and is left for 4.0.

### Native ports

Every native port was brought to 3.8.0 — most were rewritten as ports of
synx-core's parser, engine and writers — and checked against synx-core on
generated documents (parse and resolve, `format`, `stringify`, `.synxb`
payload) with no difference. All pass the 34 conformance cases byte for byte;
none reads or writes SYNXL yet.

| Port | Tests | Checked against synx-core | Notable |
|---|---|---|---|
| Go | all pass | 23 000+ documents | `!active` 1.2 MB: 329 → 23.1 ms; a 4 MB document ×73 — key lookup was linear |
| Java 17 | 102 | 10 000 documents | `!active` 1.2 MB: 179 → 28.7 ms; indentation and limits in UTF-8 bytes |
| .NET 8 | 123 | 3 400+ documents | `!active` 1.2 MB: 9.7 s → 70 ms, allocations 20.5 GB → 39 MB; `:sort`, `:sum`, `:replace` added |
| Dart 3 | 122 | 3 400+ documents | `!active` 1.2 MB: 18.4 s → 68 ms; parsing was quadratic |
| C++17 | 107 | 21 000+ documents | `!active` 1.2 MB: 88.5 s → 32.7 ms; gcc 15, clang 22, MSVC 14.44, ASan clean |
| Swift | 103 | 21 000+ documents | `!active` 1.2 MB: 46.6 s → 95 ms; rewritten to work on UTF-8 bytes |
| Godot / GDScript | 176 | 31 000 documents | did not compile under Godot 4.7; parse ×1.6–1.9; `.synxb` now exchanges with synx-core |

As in TypeScript, the writers in Java, Dart and GDScript got slower
(×0.78–0.87) because they now quote what must be quoted. `.synxb`
compression in C++ (zlib) and Swift (`Compression`) was not exercised on the
release machine; the payload layer matches synx-core byte for byte.

### Tools and bindings

- **Language server:** document symbols are built in one pass per block instead
  of one file scan per key, point at the right line (`cache.port` pointed at
  `db.port`), and no longer crash on a long value with a multi-byte character
  at the cut; same-named keys under different parents (`db.url`, `redis.url`)
  are no longer reported as duplicates.
- **Python:** dictionaries keep source order (they were in hash order).
- **Node:** the loader replaces its `.node` copy after a rebuild instead of
  loading the first build forever.

### Compatibility

- **Rust API.** `Value::Object` holds `synx_core::Map` — an
  `indexmap::IndexMap<String, Value, foldhash::fast::RandomState>` — instead of
  `HashMap<String, Value>`, and `ParseResult::metadata` uses the same map. Code
  that reads objects through `get`, iteration or indexing compiles unchanged;
  code that builds a `HashMap` and wraps it needs `Map::from_iter` or
  `.into_iter().collect()`.
- **Metadata paths** (`ParseResult::metadata`, `.synxb` with metadata) name a
  list item as `list.#0` and escape `.` and `\` inside a key (`a\.b`). Before,
  items had no path of their own.
- **Native ports.** Go: `ParseResult.Metadata` is a `*MetadataTree` (dot-path
  view as in synx-core) and `synx.Version` exists. Swift: `SynxMarkers.apply*`
  are no longer public. Dart: `markerFns` receive the chain segments after the
  marker name as `args`; `!use` finds packages as synx-core does
  (`<packages>/@scope/name/` and the manifest's `main`, else `src/main.synx`,
  where it read `<packages>/<pkg>/synx.synx`); `.synx.lock` uses the
  synx-core `key value` format.
- **Building the bindings from source** needs a C compiler for mimalloc; the
  published wheels and Node binaries are prebuilt.
- Documents: the active-mode changes above alter results only where 3.7.4 was
  wrong or nondeterministic, or where Rust and TypeScript disagreed.

### Tests

New conformance cases: SYNX 019, 106–114; SYNXL 185–186. New differential
fuzzers, run for this release: synx-core 3.8.0 against 3.7.4, synx-core
against TypeScript (static, active, structured documents with lists of
objects, dotted keys and Unicode indentation — 0 differences in 70 000+),
SYNXL Rust against TypeScript (projection and diagnostics — 0 in 8 000),
writer round-trip and format properties (0 in 3 000 and 2 000), and the Python
and Node bindings against synx-core's own JSON (0 in 12 000 and 6 000).

### Toward 4.0

Two changes were prototyped on a copy of synx-core and measured with the same
benches, allocation counter and fuzzers; both need an API break, so they wait
for 4.0.

- **Compact strings** for keys and values (inline up to 24 bytes): 41–83 %
  fewer allocations, ×1.20–1.50 faster, identical output on 30 000 fuzz
  documents. Key interning was dropped: 0.1 % of real keys exceed 24 bytes.
- **Declarations stored on their objects** instead of a map of dotted paths:
  ×1.05–1.08 in active mode, ±2 % static, identical output on 60 000 fuzz
  documents. Its value is correctness — the three path defects fixed above
  cannot occur — not speed.

## [3.7.4] — 2026-10-14

A parity release. The parser fixes of 3.7.2 and 3.7.3 reached Rust, TypeScript
and Python only: the six native parsers never received them, and nothing
noticed, because CI did not run their suites and most of their conformance
runners never compared a result with its golden. This release closes that gap,
puts every implementation under one byte-exact gate — now covering active mode
too — and carries errata for active-mode constraints.

### Erratum — `[type:string]` on `:env` and `:default`

- **`[type:string]` now keeps text taken from the environment as text.**
  `:env` and `:default` cast their value like a literal, so a value that only
  looks numeric — a 19-digit snowflake id, a zero-padded `007` — became a
  number, and `[type:string]` then rejected its own declared type with
  `CONSTRAINT_ERR`. Only the `(string)` hint escaped the cast. Both now do, in
  every implementation (`wants_string` in `crates/synx-core/src/engine.rs`).
  In synx-core and TypeScript, a document that previously produced that
  `CONSTRAINT_ERR` now produces the string; no other output changes.
  Active-mode markers sit outside the frozen core
  (`docs/spec/CORE-FREEZE.md` §1), and the rule is now written into
  `SPECIFICATION_EN.md` / `SPECIFICATION_RU.md`.
- **`[type:string]` accepts a `:secret`.** `key[type:string]:secret:env VAR` —
  the usual shape of an API key in a config — always failed the type check,
  because a secret is not a plain string value.

### Erratum — constraints

- **A string's `[min]` / `[max]` length is its count of Unicode characters.**
  synx-core counted UTF-8 bytes, so `[max:4]` rejected the three-letter `мир`
  (6 bytes) and `[min:3]` let a two-letter Cyrillic nickname through, while
  TypeScript counted UTF-16 units and split emoji in two. Every implementation
  now counts Unicode scalar values.
- **A `[pattern]` that nests one quantifier inside another (`^(a+)+$`) is
  skipped, as an invalid pattern always was.** Backtracking regex engines —
  TypeScript, Java, .NET, Dart, Swift, C++ — can spend exponential time on
  such a pattern, which a document controls. TypeScript refused it in SYNXL
  since 3.7.2 but not in active mode, where the same 88-byte document still
  hung the event loop. The `regex` crate is linear and ran it, so Rust and
  TypeScript already disagreed on SYNXL; synx-core now refuses the same shape
  (`has_nested_quantifier`), in active mode and in SYNXL, and the projection
  is identical everywhere.
- **TypeScript skipped the document-wide constraint pass.** A constraint
  declared once applies to every field of that name, merged with the
  strictest bound winning (`validate_field_constraints`); TypeScript only
  checked the line that declared it, so `port[max:65535]` under one section
  left `port 70000` under another unchecked.

### Fixed — `.synxb` decode depth

- **The 3.7.3 stack guard overflowed the stack.** The decoder refused nesting
  beyond 1024 levels, but 1024 recursive frames are enough to overflow a 2 MiB
  thread in a Windows debug build — `cargo test -p synx-core` aborted with
  `STATUS_STACK_OVERFLOW` on the very test meant to prove the guard — and sit
  close to the 1 MiB main-thread stack Windows gives a process. The cap is now
  256 in Rust and TypeScript, twice the parser's 128-level nesting limit, and a
  new test checks that the deepest tree `parse` can produce still decodes.

### Fixed — TypeScript

- **A leading BOM stuck to the first key of a static document**
  (`packages/synx-js/src/parser.ts`). `trim()` hid it in front of `!active`,
  which is why 3.7.3 recorded TypeScript as already correct, but `\uFEFFname x`
  produced the key `"\uFEFFname"`. A single leading U+FEFF is now dropped
  before parsing, as in Rust.
- **`:env` lost precision on 64-bit integers** (`packages/synx-js/src/engine.ts`).
  3.7.2 made the parser keep i64 values exact, but the active-mode engine kept
  its own `parseInt` cast, so `id:env DISCORD_APP_ID` still turned a snowflake
  into a different id. The engine cast now mirrors `cast_primitive`: quotes keep
  the literal, integers are exact i64 (`bigint` beyond 2^53), and an i64
  overflow is tried as a float. `[type:int]`, `[type:float]` and the `(int)`
  type registry accept a `bigint`, which they rejected since 3.7.2.
- **A value that began with `(word)` was taken for a type cast**
  (`packages/synx-js/src/parser.ts`). The parser glued the key-line hint onto
  the value as `(hint)value` and re-detected it later, so a value that merely
  started that way was cast too: `key (aa)` read as `""`, `key (random:bool)`
  as a random boolean, `key (int)5` as `5`. Gluing also broke the key line
  itself — `key(int)` with children or `key(string) |` lost its object or
  block. The hint now travels separately (`castTyped`) and a value is always
  text, as in synx-core. Two existing tests that relied on the old reading are
  rewritten to the documented `key(hint) value` form.
- **Importing `src/index.ts` as an ES module threw `ReferenceError: module is
  not defined`.** The CommonJS export block is now guarded; the published
  CommonJS build is unchanged.

### Fixed — native parsers

Go, .NET, Java, Dart, Swift and C++ now produce every conformance golden byte
for byte. Five of their six "conformance" runners (all but .NET) only checked
that a case parsed to an object; each is now a byte-exact comparison, and
running them for real is what surfaced most of what follows.

- **Parser parity with 3.7.2/3.7.3, in all six:** a leading BOM is dropped
  (so `\uFEFF!active` switches to active mode instead of becoming a key);
  `|` / `|+` bodies are read before comment, `###` and directive handling, so
  `#`, `//` and `!…` lines in a body are kept rather than deleted; indentation
  is measured over Unicode whitespace and `|+` bodies and the 1 MiB cap are
  cut on character boundaries. .NET also measured non-ASCII keys in UTF-8 byte
  offsets used as string indexes, and dropped an unknown `!foo bar` line that
  Rust keeps as the key `"!foo"`.
- **Floats were printed with `%.17g` or the platform default** in Go, Java,
  Swift, C++ and Dart — `3.1400000000000001`, `99.500000000000000` — which
  failed case 004 outright. JSON output now uses the shortest round-trip
  digits in the reference (`ryu`) layout; stringify and interpolation use the
  reference `Display` form.
- **Active mode:** the `[type:string]` erratum above; `:env` / `:default`
  values are cast like `cast_primitive` (exact i64, quotes keep the literal,
  the fallback is every segment after `default` rejoined with `:`); a
  `:secret` is applied after the rest of the marker chain — before, Java,
  Swift and Go masked the variable *name* of `:secret:env`, or lost the secret
  entirely — and interpolates as `[SECRET]`.
- **`{interpolation}` did not exist** in the Go, Dart, Java and Swift engines;
  it is added with the reference semantics (children first, `{a.b}`,
  `{key:alias}`, non-ASCII literal text intact, secrets masked).
- **`:env` without an env map returned the variable name** in Go, Dart, Java,
  Swift and C++. It now reads the process environment, as Rust does; an explicitly
  empty map still means "no variables".
- **.NET: list items with fields.** `- key value` followed by deeper lines
  did not become an object item, so the fields below it broke the tree; items
  now follow the reference list logic (fields, nested objects and lists, `|`
  bodies inside an item).
- **Swift and C++ `:calc`** are byte-level ports of `calc.rs`: a failing
  expression now yields the reference `CALC_ERR: …` text instead of the raw
  expression, operands are substituted in the reference number format rather
  than `%.17g` (`0.1 * 3` → `0.30000000000000004`, as in Rust), and sibling
  keys and dotted paths (`base.hp`) resolve as in Rust.
- **C++: dangling namespace pointers.** `namespaces` pointed into `root`, which
  a second `!include` or `!use` could reallocate; they are refreshed after
  every load.
- **`|+` suites for Java, Swift and C++**, closing the gap 3.7.1 left open,
  plus engine suites for the erratum, secret masking and non-ASCII
  interpolation in every port.

- **A failed constraint only reached stderr** in Go, Dart, Java, Swift and
  C++, and only in strict mode; the value went through unchecked. It now
  becomes the reference `CONSTRAINT_ERR: …` string, with the per-key pass, the
  document-wide pass and the `TYPE_ERR` pass in the reference order. A
  `strict` option still echoes each error to stderr.
- **C++: an invalid `[pattern]` aborted the process.** The library is built
  with `-fno-exceptions`, so a pattern `std::regex` rejects — `[`, `a{2,1}` —
  ended the host process. Patterns are now checked before compilation and an
  invalid one is skipped, as everywhere else.

### Fixed — CLI

- **`synx parse` printed an `!active` document unresolved when it began with a
  BOM** (`crates/synx-cli/src/main.rs`). The command scanned lines for
  `!active` itself, and `str::trim` keeps U+FEFF; it now relies on the
  parser's mode flag, which 3.7.3 already made BOM-aware.

### Tests and CI

- **Six new static conformance cases** (`tests/conformance/cases/013`–`018`):
  leading BOM, comment syntax inside `|` / `|+` bodies, directives inside
  bodies, a type hint after a space, Unicode indentation inside `|+`, and
  values that begin with parentheses.
- **The first active-mode conformance cases** (`100`–`105`): every constraint
  and its message, constraints after markers, the document-wide constraint
  pass, the `(type)` registry, interpolation with secrets and non-ASCII text,
  and pattern backtracking. A runner resolves a document the parser reports as
  `!active` before comparing; the cases never depend on the environment.
  Goldens come from `synx-core`; all eight implementations match all 24 cases
  byte for byte.
- **TypeScript now runs the SYNX conformance suite**
  (`packages/synx-js/tests/conformance.test.ts`). It ran only the SYNXL one,
  which is how the BOM case above slipped through.
- **New `native-parsers` CI job** (`.github/workflows/bindings-smoke.yml`):
  Go, .NET, Java, Dart, Swift and C++ run their full suites on every push.
- Regression tests for the erratum in Rust and TypeScript.

### Notes

- **Swift API:** `SynxMarkers.applyCalc` now also takes the key and the
  enclosing object, which sibling and dotted references need. It is an
  engine-internal entry point, but it is public, so a direct caller has to
  pass the two new arguments.
- **Behaviour change in the native packages:** `port:env PORT` is now an
  integer and `:env` without an env map reads the process environment, as the
  reference always did; constraint failures now replace the value.
- **C++ patterns:** `std::regex` speaks ECMAScript only, so a pattern that is
  valid in Rust but outside that dialect (`\p{L}`, `(?i)`, `\A`) is skipped
  rather than enforced in C++.

### Docs

- Removed links to `docs/dev/plugins-roadmap.md`, deleted in 3.6.1, from the
  root README, the VS Code and Visual Studio READMEs, `SYNX_AT_A_GLANCE.md` and
  `repository-layout.md`. The stub described `!use` as not shipped; the package
  system behind it landed in 3.6.1.
- `tests/conformance/README.md` lists every runner and how to start it.

## [3.7.3] — 2026-09-28

A security, robustness and cross-implementation-parity pass driven by a
fuzzing + differential audit of the Rust core and the TypeScript parser, with a
focus on hostile input to `.synxb` and SYNXL and on denial-of-service through
pathological-but-valid documents. Every item was reproduced before it was
fixed and carries a regression test. No conformance golden changed: the fixes
either restore the normative spec, close a security hole, or make one
implementation match the other on input the frozen suite did not cover.

### Fixed — security

- **`:secret` leaked in clear through `{interpolation}`, `:join`, `:prompt`
  and `:map`** (`crates/synx-core/src/engine.rs`). `value_to_string` returned
  the raw secret, so `msg hello {token} world` resolved the marked value into
  plain text in the output. Every string sink now sees the `[SECRET]` mask,
  matching `write_json`.
- **A `:secret` value serialised as `{}` instead of `[SECRET]` in the
  TypeScript canonical JSON writer** (`packages/synx-js/src/json.ts`). The
  hand-written writer ignored `SynxSecret.toJSON`, dropping the redaction
  marker. `SynxSecret` now carries a global-symbol brand the writer recognises.
- **`.synxb` decode was a decompression-bomb and OOM vector**
  (`crates/synx-core/src/binary.rs`, `packages/synx-js/src/index.ts`). The
  decoder trusted the declared uncompressed size (up to 4 GiB) and pre-allocated
  `Vec`/array capacities straight from untrusted counts. Decoding is now bounded
  (256 MiB cap, `decompress_to_vec_with_limit`), capacities are clamped to the
  bytes that remain, and recursion is depth-limited — hostile containers return
  a clean error instead of aborting, hanging, or overflowing the stack.
- **A `__proto__` key in a `.synxb` payload hit the prototype setter in
  TypeScript** (`packages/synx-js/src/index.ts`). Object keys from the file are
  now defined as own data properties, matching the Rust `HashMap` and blocking
  prototype pollution.

### Fixed — denial of service

- **Quadratic SYNX parse under a long ancestor key** (`crates/synx-core/src/parser.rs`).
  A multi-megabyte parent key was re-hashed on every child line, so an 8 MiB
  document (inside the 16 MiB cap) took tens of seconds. A parent-navigation
  cache, invalidated on every stack mutation, restores linear time.
- **Quadratic multiline-block accumulation in TypeScript**
  (`packages/synx-js/src/parser.ts`). Each `|`/`|+` continuation line rescanned
  the whole growing string (`utf8Length`), making a few-thousand-line block take
  tens of seconds. Lines now accumulate in an array with a running byte count.
- **`:unique` was O(n²)** (`crates/synx-core/src/engine.rs`). De-duplication
  used `Vec::contains`; it now uses a `HashSet`.

### Fixed — correctness and cross-implementation parity

- **A leading BOM disabled `!active` in Rust** (`crates/synx-core/src/parser.rs`).
  `str::trim` does not treat U+FEFF as whitespace, so a BOM-prefixed `!active`
  (as Windows editors emit) became a `"\u{feff}!active"` key and left the
  document Static — silently disabling markers, including `:secret`. A single
  leading BOM is now stripped (SYNX §2.1/§6), matching TypeScript.
- **Non-ASCII literal text was corrupted by `{interpolation}` in Rust**
  (`crates/synx-core/src/engine.rs`). A byte-wise copy (`bytes[i] as char`)
  turned `мир` into `Ð¼Ð¸Ñ`; literal text is now copied whole UTF-8 scalars.
- **`i64` values beyond 2^31 (and negatives) were corrupted in the TypeScript
  `.synxb` codec** (`packages/synx-js/src/index.ts`). Varint/zigzag used 32-bit
  bitwise ops and then rounded at 2^53; they now use BigInt end to end, exact
  across the JS-safe range and byte-compatible with Rust.
- **`!lock` / `!tool` / `!schema` / `!llm` flags were dropped when compiling
  `.synxb` in TypeScript**, `!use` was parsed as a literal `"!use"` key
  (conformance case 012), and `format()` dropped `!tool`/`!schema`/`!llm`
  (`packages/synx-js/`). All now match the Rust reference.
- **A type hint written after a space (`key (int) 5`) was applied as a cast in
  TypeScript** (`packages/synx-js/src/parser.ts`). SYNX §7 requires the hint to
  follow the key immediately; the value `(int) 5` is now kept as a string, as in
  Rust.

### Tests

New regression coverage: `.synxb` hardening (Rust + TypeScript), the linear
parse/multiline guarantees, BOM handling, secret redaction across sinks, the
non-ASCII interpolation fix, and the `(type)`-after-space case. Rust 190+ tests
and the full TypeScript suite pass; the SYNX and SYNXL conformance corpora are
unchanged and green on both implementations.

## [3.7.2] — 2026-09-27

A correctness and hardening pass over SYNXL and the SYNX parser underneath it,
from a full audit of all four implementations against
[`SYNXL-1-NORMATIVE.md`](docs/spec/SYNXL-1-NORMATIVE.md). Every item below was
reproduced before it was fixed; none of them was covered by the existing suites,
which is itself addressed at the end.

The whole conformance corpus now agrees byte-for-byte across Rust, TypeScript
and Python — 86 documents through both projections, including the audit's own
repro set.

### Fixed — secrets and active-mode parity

- **`:secret` printed in clear by `console.log`** (`packages/synx-js/src/engine.ts`).
  `SynxSecret` masked `String()` and `JSON.stringify`, but its value sat in an
  ordinary own property, so `util.inspect` — and with it `console.log(config)` —
  printed `SynxSecret { _value: '<the key>' }`. The value is now a runtime-private
  field and `util.inspect` shows `[SECRET]`; spread and `Object.entries` see nothing.
  `reveal()` is unchanged.

- **`:secret` handed unmasked to WASM marker plugins** (`crates/synx-core/src/wasm.rs`).
  The payload for `synx_apply()` passed `Value::Secret` through raw; a plugin is
  less trusted than the core and now receives `[SECRET]`, as every other output
  does. The same payload built invalid JSON for any nested string (no quotes, no
  escaping) and for non-finite floats; nested values are now real JSON.

- **`{:?}` printed secrets** (`crates/synx-core/src/value.rs`). `Value` derived
  `Debug`; it is now hand-written and shows `Secret([SECRET])`.

- **TypeScript diverged from `synx-core` in three active-mode cases**, found by a
  4 000-document differential run:
  - `(int)` on a non-integer (`-404.3`) truncated to `-404`; it now yields `0`,
    like the reference `parse::<i64>().unwrap_or(0)`.
  - `[enum:…]` accepted `null`; it now reports `CONSTRAINT_ERR` as the reference does.
  - no `TYPE_ERR` pass: a `(type)` hint the resolved value no longer matches
    now becomes `TYPE_ERR: '<key>' expected <type> but got <type>`.

### Fixed — parser crashes

- **Panic on a Unicode indent inside a `|+` body** (`crates/synx-core/src/parser.rs`).
  Indentation is measured over Unicode whitespace (SYNX §4) but the body slice
  was taken at that byte count, so a continuation line opening with U+00A0 cut
  into the middle of a character and aborted the process. 53 bytes of input were
  enough; roughly one random document in two hundred hit it. Reached from
  `synx parse`, the SYNXL block delegation (§9.3) and the Python binding alike,
  where it surfaced as a `PanicException` that `except Exception` cannot catch.

- **Panic at the multiline body cap.** The 1 MiB `MAX_MULTILINE_BLOCK_BYTES`
  cut was a raw byte slice, so a body whose limit fell inside a multi-byte
  character panicked — any long field of Cyrillic, CJK or emoji text. The
  TypeScript parser had the mirror-image bug: it measured the cap in UTF-16
  units and split surrogate pairs, emitting a lone surrogate that strict JSON
  consumers reject.

- **Unbounded recursion in the Python value converter**
  (`bindings/python/src/lib.rs`). `synxl_write({"self": itself})` overflowed the
  native stack and killed the interpreter outright — no traceback, nothing to
  catch. Conversion now refuses anything deeper than the parser's own nesting
  cap, which also covers cycles.

### Fixed — silent data loss

- **`#`, `//` and `###` were deleted from inside `|` and `|+` bodies.** A
  multiline body is raw text (SYNX §8.4.1), but the comment rules ran before the
  block-continuation branch, so every Python/C/JS comment and Markdown heading
  in a dataset disappeared on the first read, with no diagnostic and exit 0.
  Worse, a `###` pair inside a body toggled block-comment mode and swallowed the
  *following* field. Directives had the same ordering bug and are fixed with it,
  which is what SYNXL §9.4 requires. Both parsers now consult the block branch
  first, as the `!` case already did.

- **Writers dropped values that only quoting could carry.** The quoting
  predicate looked at ASCII space and tab only, so a value with a leading
  U+00A0 was written bare, read back with a non-zero indent (§3.3) and the whole
  record vanished. It also ignored the column's declared type, so `"N/A"` in an
  `int` column lost its quotes and returned as `null` plus a `CastFailed`. In
  TypeScript, a value containing ` #` or ` //` was quoted rather than promoted —
  quoting does not protect those, since SYNX strips the comment first — and a
  value containing `CR` was promoted to a `|+` body, which cannot hold one.

- **SYNX's internal limits truncated block content without a word.** The 1 MiB
  body cap and the two-million-line cap both dropped input while `validate`
  still reported `0 diagnostics`. Parse results now carry a `truncated` flag,
  which SYNXL turns into the `RecordTruncated` diagnostic §11.2 requires — the
  signal §16.4 tells consumers to rely on.

- **`synx synxl split` swallowed every diagnostic**, `RecordTruncated`
  included. It is the one subcommand after which the original is usually
  deleted.

- **64-bit integers lost precision in TypeScript.** `parseInt` rounded a
  19-digit snowflake id to a different id, with no diagnostic, and a typed
  `int` column never failed on overflow at all. Integers outside JavaScript's
  safe range but inside i64 are now exact (`bigint`), and one that overflows i64
  falls through to the string rule, matching the reference implementation.

- **The canonical JSON writer dropped fields** (`packages/synx-js/src/json.ts`).
  Sorting through a `JSON.stringify` replacer was undone on the way out —
  JavaScript emits integer-like keys first — and assigning `__proto__` onto a
  plain object hit the prototype setter, so that field never appeared. The
  writer is now hand-rolled: key order holds, `__proto__` survives as data, and
  numbers and escapes match the reference byte for byte.

### Fixed — denial of service

- **Catastrophic backtracking via `pattern:`** in TypeScript validating mode:
  `^(a+)+$` against 45 characters hung the event loop indefinitely, from an
  88-byte document in which both the pattern and the value are attacker-supplied.
  Patterns that nest one quantifier inside another are now refused rather than
  run.

- **`streamSynxlAsync` held the whole input.** With no `LF` in sight the line
  buffer grew without limit and the repeated `substring` made it quadratic:
  100 MB took 21 s and 1.5 GiB. It is now bounded by the per-record budget
  (§13, §16.3) — 60 MB in 0.2 s and 13 MB of heap.

- **Quadratic block-diagnostic attribution.** Each block diagnostic re-scanned
  the whole block, so 60 000 keys took 55 s against Rust's 1.6 s. The scan is
  now a single indexed pass.

- **Quadratic trailing-whitespace strip in the writer** (`/[ \t]+$/`): a cell of
  100 k spaces cost 7.5 s.

- **The Python binding held the GIL through every parse**, freezing all other
  threads in the process for the duration. Parsing, file reading and both
  projections now release it.

### Fixed — non-determinism and injection

- **The same document parsed differently on each read.** The §8.3 ban on
  `random` hints was enforced for `!fields` but not inherited by the delegated
  block parse, so `r1(random) x` in a record produced a fresh number every time
  — defeating hashing, de-duplication and the byte-identical projection §1.4 is
  measured on. Both parsers now disable the hints for embedded parses, alongside
  the existing directive switch.

- **CSV formula injection.** `synx synxl convert --to csv` wrote cells opening
  with `=`, `+`, `-`, `@`, TAB or CR verbatim, so exporting an untrusted dataset
  and opening it in a spreadsheet executed them. Such cells are now defused, and
  the count is reported rather than applied in silence.

- **`--prefix` was a path-traversal primitive** in `split`: it was interpolated
  straight into the shard path.

- **`__proto__` could reshape a record in TypeScript.** The key was rejected
  after the parser repaired its indentation stack rather than before, so a
  rejected key still closed a real field's subtree and the lines beneath it
  overwrote the sibling the document declared.

### Fixed — partial results and exit codes

- **A hard error left a valid-looking partial projection on stdout.** §11.1
  forbids a partial result, but the buffered writer flushed on the way out, so
  `parse -f ndjson` emitted a truncated-but-well-formed stream that a pipeline
  losing `$?` cannot tell from a complete one. stdout targets are now
  pre-flighted, and file targets go through a temporary and a rename.

- **A failed command deleted a pre-existing `--output` file.** Cleanup removed
  the path whether or not the command had created it.

- **`split` left finished shards behind on an abort**, and a re-split with a
  larger `-n` left the previous run's extra shards in place, where a glob reads
  them as duplicate records.

- **Invalid UTF-8 exited 2 (I/O) instead of 1 (specification).** A caller that
  retries on 2 would retry forever.

- **`convert jsonl→synxl` wrote `null` into fields a record never had**,
  contradicting §12.1. Field lists are now re-declared per record shape, exactly
  as a source document would (§4.2). It also produced an unreadable document
  from empty objects, and `write_lines` accepted field names containing `;`,
  newlines or brackets — which read back as a *different schema*, silently.

### Added

- `--diagnostics-out FILE` on every `synx synxl` subcommand, writing diagnostics
  as NDJSON. §11.2 forbids stderr text from being the only channel.
- `-` as an input path, reading stdin.
- `synxl→csv` now writes the header row for a dataset with a schema but no
  records.
- The Python binding accepts `os.PathLike` paths, rejects a bare string for
  `fields`, converts tuples to arrays, and `SynxlRecord` completes the mapping
  protocol (`__iter__`, `get`, `items`, `KeyError` instead of `IndexError`).
  `doc.records` returns a fresh list, so mutating it can no longer leave the
  document contradicting itself.
- **PyO3 upgraded from 0.24 to 0.29**, which is what actually fixes the Python
  binding failing to build against CPython 3.14 — 0.24 refused outright, and no
  `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` escape hatch is needed any more. It is
  gone from both workflows.

  The migration was mechanical: `PyObject` left the prelude (it is `Py<PyAny>`),
  `Python::allow_threads` became `Python::detach`, and `Bound::downcast` became
  `Bound::cast`. 0.29 supports CPython 3.8 and up, which is exactly the range
  `requires-python` already advertised, so the wheel matrix is unaffected.
  Verified on 3.14: the full binding suite passes and the GIL is still released
  for the duration of a parse.

### Added — test coverage for the gaps that let all of this through

Three holes explain why 33 defects survived a green CI, and each is now closed:

- **The conformance suite only ever exercised the parser.** §14 (writer,
  round-trip) and §15.3 (shardability) had no coverage at all. Both runners now
  check §14.1 — `parse(write(parse(D)))` against `parse(D)` — on every accepted
  case, and `synx-cli` checks §15.3 across the whole corpus at four shard sizes:
  every shard must parse standalone and the rejoined projections must equal the
  source's. Three cases failed round-trip on the first run.

- **SYNXL had no fuzz target**, though §16 names it the format that reads
  untrusted input. `fuzz_synxl` asserts crash-freedom plus the two invariants a
  reader/writer pair must hold (§1.4.3 reader agreement, §14.1 round-trip).
  Because cargo-fuzz needs a nightly toolchain and a sanitizer runtime, and so
  never gates a commit, `tests/property_synxl.rs` runs the same invariants over
  8 000 generated documents in the ordinary `cargo test` path, alongside a
  regression set of the minimised hostile inputs and two superlinearity guards.

- **Nothing compared the implementations to each other.**
  `scripts/ci/diff-implementations.mjs` runs the corpus plus the audit's
  minimised divergences through the Rust CLI and the TypeScript package and
  compares both projections byte for byte — 105 documents. A new `synxl-cross`
  CI job runs all of the above together.

Seventeen conformance cases were added for the defects a fixture can pin down.
That distinction matters and is now written into the suite's README: a
self-checking invariant like round-trip cannot catch *symmetric* loss — a parser
that drops the same content going in and coming out still round-trips perfectly
— which is exactly what comment lines vanishing from a `|+` body looked like.
Only an independently derived expectation catches that.

Regression suites were added for the two implementations whose defects lie
outside the fixtures' reach: `packages/synx-js/tests/synxl-hardening.test.ts`
(27 cases, including the denial-of-service bounds) and
`bindings/python/tests/test_synxl_hardening.py` (25 cases, covering the
Python→SYNXL direction the conformance suite cannot see).

### Notes

- `SynxPrimitive` (TypeScript) gains `bigint`, and `ParseResult` (Rust) gains
  `truncated`. Both are additive.
- The NDJSON projection now terminates each record with `LF` rather than
  separating them, matching the reference implementation and making an append a
  plain concatenation.
- One documented divergence remains: JavaScript cannot distinguish `5.0` from
  `5`, so an integral float projects as `5` there and `5.0` in Rust. §14.3
  already admits this.

## [3.7.1] — 2026-07-31

A release-integrity fix. 3.7.0 reached crates.io and NuGet but not npm, PyPI or
pub.dev, and the reason on pub.dev was a genuine defect rather than a
credential problem. This release carries the same feature set as 3.7.0 with
that defect fixed, so every registry lands on one number again.

### Fixed

- **Dart parser did not compile under `|+`.** The §8.4.1 branch called
  `codeUnitAt` and `substring` on `raw`, which is the line's byte list rather
  than a `String`. Trimming and slicing now happen on the bytes with a single
  decode at the end — which is also what the specification describes, since the
  offsets there are byte offsets. `dart analyze` rejected the package, so the
  broken parser was never published.

- **Windows wheels failed to build.** maturin stopped with *"compiling for
  Windows without an interpreter requires PyO3's `generate-import-lib`
  feature"*: the release matrix builds for CPython 3.9–3.13 from a runner that
  has only one of them installed. The feature is now enabled. Because the PyPI
  upload waits on every wheel, this single failure had skipped PyPI entirely.

### Added — `|+` test coverage for Dart and .NET

The Dart defect survived to a release because `|+` was only covered in Rust,
JS and Go. Both parsers now carry the operator's own suite: the worked example
from the specification, base-indent locking, per-line trailing-whitespace
stripping, block termination at the opener indent, plain `|` for contrast, and
a non-ASCII body — the case that fails whenever a byte offset is applied to the
wrong unit, which is exactly how the Dart parser broke. **.NET passed without
any change to the parser**, confirming the package already on NuGet is sound.

C++, Java and Swift still have no `|+` coverage; that gap is open.

## [3.7.0] — 2026-07-30

### Added — `|+` indent-preserving multiline blocks

- New multiline opener **`|+`** alongside the existing `|`. While `|` trims each
  continuation line fully (per Frozen 3.6 §8.4), `|+` keeps each line's indent
  *relative to the first non-empty continuation line*. The base prefix is
  locked on that first line and stripped from every subsequent line; anything
  beyond it is preserved verbatim. This makes it safe to embed indentation-
  sensitive content (code, SYNX examples, ASCII diagrams, AI-prompt scaffolds)
  directly inside a SYNX value.

  Example:
  ```synx
  prompt |+
    Outline:
      - step one
      - step two
        sub-step
  ```
  parses as `"Outline:\n  - step one\n  - step two\n    sub-step"`.

- **Frozen reference (3.6) is unaffected.** A 3.6 parser sees `key |+` as a
  plain string value `"|+"`. Documents that worked under 3.6 keep parsing
  identically. The only behavioural change between 3.6 and 3.7 is that the
  literal token `|+` now opens a multiline block; previously it was a
  (probably nonsensical) two-character string value.

- Specification update: see [`docs/spec/SYNX-3.7-NORMATIVE.md`](docs/spec/SYNX-3.7-NORMATIVE.md)
  §8.4.1.

### Updated parsers

`|+` is implemented in every parser shipped from this repo:

- **[`crates/synx-core/`](crates/synx-core/)** — Rust reference engine. Also
  reaches every FFI binding: `bindings/c-header`, `bindings/node`,
  `bindings/python`, `bindings/wasm`, `bindings/kotlin`, `bindings/mojo`.
- **[`packages/synx-js/`](packages/synx-js/)** — TypeScript / browser bundle
  (`@aperturesyndicate/synx-format`).
- **[`parsers/cpp/`](parsers/cpp/)** — C++17 header-only.
- **[`parsers/dart/`](parsers/dart/)** — pure Dart 3.
- **[`parsers/dotnet/`](parsers/dotnet/)** — .NET 9.
- **[`parsers/go/`](parsers/go/)** — pure Go.
- **[`parsers/java/`](parsers/java/)** — Java 17.
- **[`parsers/swift/`](parsers/swift/)** — Swift 5.

Tests added in JS, Rust core, and Go covering: indent preservation, base
indent locking, block termination at opener indent.

### Added — SYNXL (`.synxl`), the SYNX Lines record format

A **record-stream format** for homogeneous datasets: the SYNX-native
counterpart of JSONL and CSV. Aimed at AI/LLM datasets and tabular data.

```synxl
!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1

2 ; 0.74
  messages
    - role user
      content Привет
```

- **Field list declared once** (`!fields`), carrying SYNX types and
  constraints. Against JSONL this removes the dominant token cost — keys and
  structural punctuation repeated on every record. Against CSV it adds types,
  and it makes an empty field unambiguously `null` (an empty *string* is
  `""`).
- **One record starts at indent 0**; anything indented below it belongs to
  that record. No record separator is needed, appending is safe, and a record
  boundary is decidable from a single byte — which is what makes streaming and
  parallel parsing possible.
- **Fields marked `[block]` expand under the record** in full SYNX: nested
  objects, lists, and `|` / `|+` multiline text. Multi-line values stay
  readable text instead of `\n`-escaped noise, so chat-style datasets
  (`messages` as a list of role/content objects) are expressible directly.
  Block content is defined by delegation to the SYNX 3.7 parser, so it is the
  same language, not a lookalike.
- **Schema evolution mid-file:** a later `!fields` line replaces the one in
  effect and applies to following records only, so a producer that gains a
  column keeps appending without rewriting history.
- **Diagnostics instead of silent drops.** SYNX §8.10 skips malformed
  structure silently; SYNXL reports it (`MissingFields`, `ExtraFields`,
  `CastFailed`, `UnknownBlockKey`, …) with record index and line, because a
  dataset that silently loses a column is worse than one that fails loudly.
- **Per-record size limit, not per-file.** SYNX caps input at 16 MiB; that cap
  is deliberately not applied to a SYNXL document, since records are
  independent and datasets are routinely gigabytes.
- **Security:** SYNX directives inside a record block are never honored — a
  row from an untrusted corpus must not become a file-read primitive via
  `!include`. A line beginning with `!` inside a `|+` body remains ordinary
  content.

Specification: [`docs/spec/SYNXL-1-NORMATIVE.md`](docs/spec/SYNXL-1-NORMATIVE.md).
SYNXL is versioned on **its own axis** (format version `1`), independent of
the SYNX language version and of the `.synxb` container version — a SYNXL
document is not a SYNX document, since its top level is a record sequence
rather than an object (SYNX §8.1).

#### Available in this release

- **[`crates/synx-core/`](crates/synx-core/)** — reference implementation.
  Whole-document parse, an owned reader, and a true streaming reader over
  `io::BufRead` whose live memory is one record; canonical JSON and NDJSON
  projections; a writer with round-trip guarantees.
- **[`packages/synx-js/`](packages/synx-js/)** — TypeScript, including the
  browser bundle; sync generator and async iterator streaming.
- **[`bindings/python/`](bindings/python/)** — parse, off-disk streaming,
  both projections, writer.
- **[`crates/synx-cli/`](crates/synx-cli/)** — `synx synxl parse | validate |
  convert | split`. Converters both ways for JSONL and CSV; `split` writes
  shards that each carry the prologue and the field list in effect at the cut,
  so every shard is a valid document on its own.

Conformance suite: [`tests/conformance-synxl/`](tests/conformance-synxl/) —
67 cases derived from the specification text independently of any
implementation. The Rust runner additionally re-reads every accepted case
through all three readers and fails on disagreement.

The remaining parsers (C++, Dart, .NET, Go, Java, Swift) do not implement
SYNXL yet; they are unaffected and remain fully SYNX 3.7 conforming.

## [3.6.3] — 2026-05-19

A cross-implementation **parity hot-fix** release. Closes 17 divergences
uncovered by a code-review pass across all SYNX parsers (Rust canonical
+ C++, Dart, Go, Java, Swift, .NET, JS/TS). Three of the fixes are
**security-relevant** and required for any production use of the
non-Rust bindings.

Alongside the parity work, **`parsers/` is now the home for all from-scratch
language parsers** and `bindings/` is reserved for FFI / WASM bridges back
into the Rust engine. C++, Go, and Swift no longer ship as thin C-FFI
wrappers around `synx-core`; each is a native port. Dart and Java join as
fresh native parsers, and Godot 4 gets a pure-GDScript engine.

No surface-syntax change. All existing `.synx` files parse identically.

### Added — native parsers and Godot integration

- **[`parsers/cpp/`](parsers/cpp/)** — header-only C++17 parser + CMake
  project (`synx_tests.exe` 42 / 42, conformance + unit). Replaces the
  `bindings/cpp` C-FFI wrapper; no `libsynx_c` runtime dependency.
- **[`parsers/dart/`](parsers/dart/)** — pure-Dart 3 implementation (no
  FFI); `dart test` 37 / 37, `dart analyze` clean. New target — pub.dev
  package `synx` (planned).
- **[`parsers/go/`](parsers/go/)** — pure-Go module, **no cgo**. Replaces
  the `bindings/go` cgo wrapper. `go test ./...` pass, `go vet ./...`
  clean. Importable directly: `import "github.com/aperturesyndicate/synx-format/parsers/go"`.
- **[`parsers/java/`](parsers/java/)** — Java 17 implementation (Maven
  `com.aperturesyndicate:synx`). New target — Maven Central (planned).
- **[`parsers/swift/`](parsers/swift/)** — SwiftPM-only Swift 5 package
  with no C interop. Replaces the `bindings/swift` C-FFI wrapper.
- **[`integrations/godot/synx-gdscript/`](integrations/godot/synx-gdscript/)** —
  **Godot 4 editor plugin** (`addons/synx`) implementing the SYNX engine in
  pure GDScript: parser, `!active` engine, markers, binary `.synxb`,
  formatter, JSON I/O, diff, RNG, calc, options. Drop into any Godot
  project's `addons/` folder and enable in Project Settings.

### Removed — superseded FFI wrappers

- **`bindings/cpp/`** — replaced by native [`parsers/cpp/`](parsers/cpp/).
- **`bindings/go/`** — replaced by native [`parsers/go/`](parsers/go/)
  (cgo dependency gone).
- **`bindings/swift/`** — replaced by native [`parsers/swift/`](parsers/swift/)
  (no longer requires linking `libsynx_c`).

Remaining `bindings/` are the surfaces that *legitimately* need FFI or a
shared engine binary: `c-header` (reference C ABI), `node` (N-API),
`python` (PyO3), `wasm` (Rust → WebAssembly), `kotlin` (JNA), `mojo`
(CPython bridge). JVM consumers on plain Java should prefer
[`parsers/java/`](parsers/java/) over `bindings/kotlin` to avoid the
JNA / `synx-c` runtime.

### Fixed — JSON output safety

- **Rust core (`synx_core::write_json`)** — `NaN` / `±Infinity` were
  emitted as the literal `ryu` tokens `NaN` / `inf`, producing **invalid
  JSON**. Now emits `null`, matching ECMA-404 and the existing
  Java / Dart / .NET behaviour.
- **.NET `SynxJson.WriteDouble`** — same fix; `Utf8JsonWriter` throws on
  non-finite by default, we now short-circuit to `null` before it sees
  the value.
- **.NET `SynxJson.WriteJson` for `Secret`** 🔴 **(SECURITY)** — was
  emitting the **real secret value** as a regular JSON string. Now
  redacted to `"[SECRET]"` to match `synx_core::write_json` and the
  README contract (`:secret → не попадёт в логи`).

### Fixed — `:include` / `:watch` path jail

- **Go (`engine.go::jailPath`)** 🔴 **(SECURITY)** — relied on a
  `filepath.Join` plus a lexical `..` check. A symlink inside `base/`
  pointing outside the jail (e.g. `base/tmp → /etc`) bypassed the
  check entirely. Now resolves symlinks via `filepath.EvalSymlinks`
  and verifies canonical-path containment, matching
  `synx_core::engine::jail_path`.

### Fixed — `__proto__` / `constructor` / `prototype` filter

- **.NET (`SynxParser.Parse`)** 🔴 **(SECURITY for JS consumers)** —
  was missing the prototype-pollution reject list. JS / Node consumers
  of the JSON output were therefore exposed via `__proto__` injection
  in `.synx` files. Aligned with `synx_core::parser::parse`.

### Fixed — binary `.synxb` endianness

- **.NET (`SynxBinary`)** — `BitConverter.GetBytes(double)` /
  `BitConverter.ToDouble` and the matching `UInt32` variants used host
  endianness; output diverged from Rust on big-endian hosts. Now uses
  `BinaryPrimitives.WriteDoubleLittleEndian` /
  `ReadDoubleLittleEndian` (and `*UInt32LittleEndian`) to guarantee
  byte-identical output to `synx_core::binary` on every platform.

### Fixed — `:random:int` range parity

Several non-Rust parsers had constrained the random-integer range,
breaking parity with `synx_core::rng::random_i64()` which returns the
full `[i64::MIN, i64::MAX]` band:

- **.NET** — `Random.NextInt64()` (no args) only covers
  `[0, Int64.MaxValue)`. Now `NextInt64(long.MinValue, long.MaxValue)`.
- **Java** — `nextLong(Long.MAX_VALUE)` only covers
  `[0, Long.MAX_VALUE)`. Now plain `nextLong()` (full range).
- **Swift** — `Int64.random(in: 0..<Int64.max)`. Now
  `Int64.random(in: Int64.min...Int64.max)`.
- **Dart** — `Random.nextInt(1 << 32)` capped at unsigned 32-bit. Now
  combines two 32-bit draws into a full signed 64-bit value.
- **JS / TS** — `Math.random() * 2147483647` capped at 31-bit
  non-negative. Now spans `[-Number.MAX_SAFE_INTEGER,
  Number.MAX_SAFE_INTEGER)` (the precise integer band JS can
  represent; values beyond 2⁵³−1 still lose precision in any
  `.synxb` roundtrip — a fundamental `Number` limit, not a code bug).

### Fixed — C++ enum constraint parser

- **`parsers/cpp/src/parser.cpp::parse_constraints`** — the scan
  condition `p < v.size()` dropped the trailing empty segment, so
  `enum:` produced `[]` and `enum:a|` produced `["a"]`. Now uses
  `p <= v.size()`, matching Rust `str::split('|')`: `enum:` →
  `[""]`, `enum:a|` → `["a", ""]`.

### Fixed — Go enum trim

- **`parsers/go/parser.go::parseConstraints`** — `enum: a | b` was
  storing the surrounding whitespace as part of each value (`"a "`,
  `" b "`). Now each segment is `strings.TrimSpace`-ed.

### Fixed — `MAX_PARSE_NESTING_DEPTH` parity

- **.NET (`SynxParserCore`)** — was `2048`, 16× looser than every
  other parser. Lowered to `128` to match
  `synx_core::parser::MAX_PARSE_NESTING_DEPTH`. The same input now
  accepts / rejects identically across all implementations.

### Verification — what passes after this release

| Implementation | Test command | Result |
| -------------- | ------------ | ------ |
| Rust core      | `cargo test -p synx-core` | 83 / 83 |
| Go             | `go test ./...`           | pass |
| Go             | `go vet ./...`            | clean |
| C++            | `synx_tests.exe`          | 42 / 42 |
| Dart           | `dart test`               | 37 / 37 |
| Dart           | `dart analyze`            | no issues |
| .NET           | `dotnet build`            | 0 warn / 0 err |
| TS / JS        | `npx tsc --noEmit`        | clean |

### Compatibility notes

- Pre-3.6.3 `.synx` files containing **literal `__proto__` /
  `constructor` / `prototype` keys** were silently kept on the .NET
  side and silently dropped everywhere else. They are now dropped on
  every side, matching the documented behaviour.
- `.synxb` binaries produced by .NET **on big-endian hosts** in 3.6.2
  are not readable by 3.6.3 (and were never readable by Rust);
  regenerate them.

## [3.6.2] — 2026-05-07

A stability and parity release. Closes 27 categories of cross-engine
divergence between `synx-core` (Rust) and `@aperturesyndicate/synx-format`
(TypeScript), plus three new utility markers. **No surface syntax change**
to existing markers — same `.synx` file produces the same JSON in every
binding now.

### Added — three new markers (active mode)

- **`:replace:FROM:TO`** — literal substring replacement on a string value.
  `TO` defaults to `""` (deletion). `FROM`/`TO` cannot themselves contain
  `:` (chain delimiter); use `{interpolation}` for those cases.
  ```synx
  !active
  greeting:replace:l:L Hello there      # → "HeLLo there"
  ```
- **`:sort`** / **`:sort:desc`** — sort an array. Numeric items compare
  numerically, otherwise lexicographic. Default direction is ascending.
  ```synx
  !active
  ranked:sort:desc
    - 5
    - 1
    - 3
  # → [5, 3, 1]
  ```
- **`:sum`** — sum the numeric items of an array (non-numeric items are
  ignored). Returns an integer when all summands are integers, otherwise
  a float.
  ```synx
  !active
  total:sum
    - 19.99
    - 29.99
    - 5.50
  # → 55.48
  ```

### Fixed — cross-engine parity (CRITICAL)

- **`Synx.parse()` no longer silently switches engines at 5 KB.** The pure-TS
  parser is now used unconditionally. The native binding remains available
  via `bindings/node` for callers that explicitly opt in. Previously, the
  same file produced *different trees* depending on its size.
- **`Synx.parseTool()` rewritten to spec §9.** Sorts top-level keys
  lexicographically, takes the first as the tool name, returns
  `{ tool, params }` (call mode) or `{ tools: [...] }` (schema mode).
  Previously returned empty `{ tool: "", params: {} }`.
- **`Value::Secret` is now properly redacted in JSON output.** `synx-core`'s
  `write_json` emits `"[SECRET]"` for the `Value::Secret` variant; previously
  the raw secret string was leaked. **Security-relevant**.
- **`.synxb` binary format is now cross-language compatible.** JS
  `Synx.compile`/`decompile` now use the same wire format as
  `synx-core::binary` (header + raw-deflate payload via Node `zlib`).
- **List items with sub-keys parse identically in both engines.** Rust now
  supports the README pattern `- name Sword\n  damage 50` as an object,
  matching the long-standing JS behaviour. Previously Rust silently
  hoisted `damage` into the parent map.
- **`!tool` and `!schema` directives now parsed by JS parser.** Previously
  ignored — flags on `SynxParseResult` were never set.

### Fixed — semantic alignment (MAJOR)

- **`pattern` constraint is now enforced in Rust** (`regex` crate added).
- **Constraint validation runs once, not twice.** Eliminates Rust error
  messages like `value 52 exceeds max 10` (where 52 was the byte length
  of the previous error string).
- **`[constraints]` parser is now bracket-balanced.** Patterns containing
  `]` such as `[pattern:^[A-Z]{2}$]` parse correctly in both engines.
- **`:env` casts booleans/`null` from the environment in JS.** `DEBUG=true`
  now yields `true` (boolean), not `"true"` (string).
- **`:calc` supports dot-paths in JS** (`base.hp * 5`). Was Rust-only.
- **`:i18n:count_field` plural forms work in JS** (CLDR-inspired:
  `one/few/many/other`/`zero`/`two`).
- **`:watch:nested.field` resolves dot-paths** in Rust for both JSON and
  SYNX inputs (real `serde_json` parse, real SYNX parse — not substring
  search).
- **`:import` marker** is now handled in JS (was Rust-only).
- **`:include` / `:import` markers register an alias** in the includes map
  so `{leaf:db}` interpolation works for the README pattern.
- **`:inherit base` with children** — both engines now treat the value
  after `:inherit` as the parent name and open a group for the child
  block. Previously the README-documented pattern silently broke.
- **`:format:%e`** is implemented in Rust.
- **`:prompt` emits keys in sorted order** in both engines (deterministic
  for LLM caching).
- **`Synx.toJSON()` sorts keys lexicographically** (canonical JSON per
  spec §10).
- **`__proto__` / `constructor` / `prototype` keys are blocked** in both
  engines for safe consumption from JavaScript downstream code.
- **`synx parse` auto-enables `--active`** when the file declares
  `!active` or `#!mode:active`. Removes a long-standing UX papercut.

### Fixed — security (MAJOR)

- **Path-jail blocks Linux-style `/foo` and Windows-rooted `\foo`.**
  Previously, on Windows, `/etc/passwd` was not classified as absolute
  by `Path::is_absolute()` and could escape the base directory in some
  edge cases.
- **Error messages are OS-portable.** ENOENT / "system cannot find" both
  emit `file not found: <path>`; absolute/rooted blocks emit identical
  text on Linux and Windows.
- **JS parser enforces the §3 limits** (16 MiB input, 128 nesting depth,
  1 MiB multiline block, 1 M list items, 4 K includes, 4 K enum parts,
  512 marker chain segments). Previously only the Rust core had them.

### Fixed — cosmetic

- **Cycle-alias error messages are now stable.** Both keys in an
  `a → b → a` cycle produce the same message regardless of HashMap
  iteration order.
- **`(unknown_hint)value`** in JS no longer wraps the value back with
  the unknown hint; falls through to plain cast (matches Rust
  `cast_typed` fallback). A leading `(`-prefixed value with non-identifier
  contents (e.g. `(10 + 20) / 2`) is preserved as a literal string.

## [3.6.1] — 2026-04-09

### Added

- **Package registry — soft delete + yank system**
  - **Soft delete** (`DELETE /api/packages/:scope/:name`) — marks package as deleted (`deleted_at`, `deleted_by`, `deletion_reason` columns) instead of removing from DB/S3. All versions auto-yanked. Tarballs preserved for 30-day grace period.
  - **Restore endpoint** (`POST /api/packages/:scope/:name/restore`) — un-deletes a soft-deleted package within the 30-day grace period. All versions auto-unyanked.
  - **Permanent delete** (`DELETE /api/packages/:scope/:name/permanent`) — hard delete for already soft-deleted packages. Removes tarballs from S3 and rows from DB.
  - **Unyank endpoint** (`POST /api/packages/:scope/:name/:version/unyank`) — restore a single yanked version.
  - Detail endpoint returns **410 Gone** for soft-deleted packages with `deletedAt` and restoration info.
  - List endpoint filters out soft-deleted packages (`deleted_at IS NULL`).
  - DB migration: `deleted_at TIMESTAMPTZ`, `deleted_by TEXT`, `deletion_reason TEXT` on `packages` table.

- **`synx restore` CLI command** — restore a soft-deleted package within the 30-day grace period.

- **`synx create` CLI command** — interactive package scaffolder.
  - Prompts for scope, name, description, author, license, and package type.
  - **WASM Marker Package** template — generates `Cargo.toml`, `src/lib.rs` (ABI v1 boilerplate), `synx-pkg.synx`, `build.bat`, `build.sh`, `README.md`.
  - **SYNX Config Package** template — generates `synx-pkg.synx`, `src/main.synx` (with `:env` and `:default` markers), `README.md`.
  - Lists generated files on completion with next-steps guidance.

- **Packages page UI improvements**
  - Author avatar shows real profile image (from `users.avatar_url`) with fallback to initial letter.
  - Files tab populated from tarball extraction (file list + file contents viewer overlay).
  - Dependencies and Dependents tabs auto-hidden when empty (`tab-hidden` CSS class).
  - Cache-bust bumped to `?v=20260408f`.

- **Package registry pipeline — S3 storage + extraction**
  - Tarball extraction at publish time: README, file list, and file contents stored in `packages` table.
  - S3 Hetzner Object Storage for tarball persistence.
  - CLI `DEFAULT_REGISTRY` changed to `https://synx.aperturesyndicate.com/api`.
  - Forum-API accepts CLI Bearer tokens (via `cli_tokens` table fallback in `getUser()`).

### Changed

- **`synx delete`** — now performs soft delete with 30-day grace period (was permanent hard delete). Confirmation prompt updated to reflect soft delete behavior.
- **Yank route** — changed from `DELETE /api/packages/:scope/:name/:version` to `POST /api/packages/:scope/:name/:version/yank` to match CLI expectations.
- **Renamed `@assynx/synx-markers` → `@assynx/text-tools`** — better reflects the package's 8 text utility markers (`:upper`, `:lower`, `:reverse`, `:base64`, `:hash`, `:truncate`, `:pad`, `:count`).
- **Consolidated `examples/` and `synx_packages/`** — removed duplicate WASM examples that duplicated content already in `synx_packages/`.
- **Package template moved to `synx_packages/template/`** — starter WASM marker template lives next to official packages.

### Removed

- **Deleted `@assynx/synx-defaults`** — removed from `synx_packages/` and the embedded registry.
- **Deleted `@aperture/synx-defaults`** — removed the entire `@aperture` scope.
- **Deleted `examples/wasm-marker-upper/`** and **`examples/wasm-marker-template/`** — consolidated into `synx_packages/`.

### Added

- **SYNX Package System — complete pipeline** (Steps 1–12)
  - **Package manifests** (`synx-pkg.synx`) — normalized format with name, version, description, author, license, main, synx-version, keywords, files, dependencies, capabilities.
  - **Package engine** (`crates/synx-cli/src/pkg.rs`) — `Manifest` parser, `LockFile` (synx.lock), `pack()` → `.tar.gz` with SHA-256 integrity, `publish()`, `install()` (registry + local), `uninstall()`, `login()` (token-based auth).
  - **Credential storage** — `~/.synx/credentials.synx`, token read/write, registry-scoped.
  - **CLI commands** — `as init`, `as publish`, `as install`, `as uninstall`, `as login`, `as pack`, `as yank`, `as unyank` (wired in `main.rs`).
  - **`!use` directive support** in LSP (`synx-lsp`) — import packages from `synx_packages/`.
  - **Semver dependency resolution** — `parse_semver()`, `version_satisfies()` (^, ~, \*, >=, <=, >, <, exact), `check_conflicts()` integrated into `install()`.
  - **Package yank/unyank** — `yank()` / `unyank()` API calls + CLI commands.
  - **14 new CLI tests** + **12 semver tests** — all passing.

- **WASM custom markers — sandboxed WebAssembly runtime**
  - **`WasmMarkerRuntime`** (`crates/synx-core/src/wasm.rs`) — wasmi-based WASM interpreter with ABI v1 (`synx_alloc`, `synx_markers`, `synx_apply`).
  - **Capability permissions** — `WasmCapabilities { string, fs, net, env }` enforced at module load time.
  - **Ed25519 package signing** (`crates/synx-core/src/signing.rs`) — `SigningKey`, `VerifyKey`, `PackageSignature`, keygen/sign/verify.
  - **Engine integration** — WASM dispatch in `engine.rs` after built-in markers, before constraints.
  - **`@assynx/synx-markers`** — official marker pack with 8 markers: `:upper`, `:lower`, `:reverse`, `:base64`, `:hash`, `:truncate`, `:pad`, `:count`. Built as `.wasm`, installed in `synx_packages/`.
  - **WASM marker author guide** (`docs/guides/WASM_MARKERS.md`) — ABI spec, project setup, build, sign, publish, capabilities, security model. 

- **JS/TS binary format + tool parsing** (`packages/synx-js/src/index.ts`)
  - `Synx.compile(text, resolved?)` → `Uint8Array` — pure-JS `.synxb` compiler (string table, varint, zigzag, type tags 0x00–0x08).
  - `Synx.decompile(data)` → string — decompile `.synxb` back to `.synx` text.
  - `Synx.isSynxb(data)` → boolean — check for SYNXB magic header.
  - `Synx.parseTool(text, options?)` → `{ tool, params }` — parse `!tool` mode with reshaping.
  - JS/TS binding now at **full parity** with Python/C#/Go/Kotlin/Swift.
  - `LoadFileAsync<T>(path)` / `LoadFile<T>(path)` — read + deserialize a `.synx` file.
  - `LoadFileActiveAsync<T>(path, synxOptions?)` — same with `!active` engine resolution.
  - `SaveFileAsync<T>(path, obj)` / `SaveFile<T>(path, obj)` — serialize + write a `.synx` file.
  - `FromJson(string json)` — convert a JSON string to SYNX text (public API for the previously internal `JsonToSynx`).

- **JS/TS File save helpers** — symmetric counterpart to existing `load`/`loadSync`.
  - `Synx.save(filePath, obj, active?)` — async save object to `.synx` file.
  - `Synx.saveSync(filePath, obj, active?)` — sync save object to `.synx` file.

- **Python File I/O helpers** — load/save `.synx` files from Python.
  - `synx.load(file_path)` — read + parse a `.synx` file, returns dict.
  - `synx.load_active(file_path, base_path?)` — same with `!active` engine resolution.
  - `synx.save(file_path, obj)` — stringify + write a `.synx` file.
  - `synx.from_json(json_text)` — convert JSON string to SYNX text.

- **C# `SynxFormat.Stringify` / `Serialize<T>`** — object → SYNX text serialization, the missing inverse of `Parse` / `Deserialize<T>`.
  - `Stringify(SynxValue value)` — serialize a value tree to canonical SYNX text (sorted keys, 2-space indent, `- ` list items, `|` multiline strings). Matches Rust `Synx::stringify` output.
  - `Stringify(Dictionary<string, SynxValue> map)` — serialize a root map to SYNX text.
  - `Serialize<T>(T obj, JsonSerializerOptions?)` — generic object → SYNX text via JSON intermediate. The SYNX equivalent of `JsonSerializer.Serialize<T>()`. Uses `camelCase` naming by default.
  - Internal: `SynxStringify.cs` serializer, `JsonElementToSynxValue` converter.
  - 3 new tests: round-trip, nested object, generic Serialize<T>.
  - All parsers/bindings now have full stringify parity (C# was the only one missing it).

- **C# `Deserialize(text, Type)` — non-generic runtime-type deserialization.**
  - `Deserialize(string text, Type type, JsonSerializerOptions?)` → `object?` — the SYNX equivalent of `JsonSerializer.Deserialize(json, type)`. Useful when the target type is only known at runtime (plugin systems, reflection-based DI).
  - `DeserializeActive(string text, Type type, SynxOptions?, JsonSerializerOptions?)` → `object?` — same, with `!active` engine resolution.

- **C# Async stream API — `SerializeAsync<T>` / `DeserializeAsync<T>`.**
  - `DeserializeAsync<T>(Stream, JsonSerializerOptions?, CancellationToken)` → `Task<T?>` — read SYNX from a stream and deserialize. The SYNX equivalent of `JsonSerializer.DeserializeAsync<T>(stream)`.
  - `DeserializeActiveAsync<T>(Stream, SynxOptions?, JsonSerializerOptions?, CancellationToken)` → `Task<T?>` — same, with `!active` engine resolution.
  - `SerializeAsync<T>(Stream, T obj, JsonSerializerOptions?, CancellationToken)` → `Task` — serialize to SYNX and write to stream. The SYNX equivalent of `JsonSerializer.SerializeAsync<T>(stream, obj)`.
  - 4 new tests: Deserialize(Type), DeserializeAsync, SerializeAsync, async round-trip.
  - **Other parsers audited**: JS/TS (has `parse<T>` + `load()` async + `stringify`), Python (`parse` + `stringify`), Rust (`parse` + `stringify` + serde) — all already complete. C# was the only gap.

- **C# `SynxFormat.Format(text)`** — canonical SYNX reformatter. Sorts keys alphabetically (case-insensitive), normalizes 2-space indentation, strips comments, preserves directives. Matches Rust `Synx::format` / `fmt_canonical`. New file: `SynxFormatter.cs`.

- **C# `SynxFormat.Diff` / `DiffJson`** — structural diff between two parsed SYNX objects.
  - `Diff(Dictionary<string, SynxValue>, Dictionary<string, SynxValue>)` → `SynxDiffResult` — returns `Added`, `Removed`, `Changed`, `Unchanged`.
  - `Diff(string textA, string textB)` → `SynxDiffResult` — parses then diffs.
  - `DiffJson(string textA, string textB)` → JSON string — matches Rust `diff_to_value`.
  - New types: `SynxDiffResult`, `SynxDiffChange`. New file: `SynxDiff.cs`.

- **C# `SynxFormat.Compile` / `Decompile` / `IsSynxb`** — full `.synxb` binary format support.
  - `Compile(string text, bool resolved = false)` → `byte[]` — compiles to binary (deflate-compressed, string table, varint/zigzag encoding). Byte-compatible with Rust `synx_core::binary`.
  - `Decompile(byte[] data)` → SYNX text — decompiles binary back to text with directives.
  - `IsSynxb(byte[] data)` → `bool` — checks for `SYNXB` magic header.
  - New file: `SynxBinary.cs`.

- **C# `SynxParseResult.Llm`** — parser now recognizes and tracks the `!llm` directive (previously stripped but not recorded).

- **Full cross-parser feature parity audit.** All 13 parsers/bindings now cover the same core capabilities. C# was the only parser with gaps (was missing `format`, `diff`, `compile`/`decompile`/`is_synxb`). 7 new tests (30 total, all passing).

### Fixed

- **Inline comment highlighting across all editor integrations.** Comments after values (`key value // comment` or `key value # comment`) were incorrectly rendered as part of the value string. Fixed in:
  - **VSCode** (`synx.tmLanguage.json`) — added `(.+?)\\s+(//|#)(.*)$` pattern in `#values`; list items also updated.
  - **Sublime Text** (`synx.sublime-syntax`) — added capture groups for inline `//`/`#` in key and list-item patterns.
  - **Visual Studio** (`SynxClassifier.cs`) — added `InlineCommentRe` regex; checked before fallback `synx.value.string` classification.
  - **Tree-sitter** (`grammar.js`) — documented limitation (requires external scanner for inline comments; not feasible with regex-only rules).

- **False `:alias` error on `:include`'d keys** (`diagnostics.ts`) — `db_user:alias db_data.user` no longer reports `Key "db_data.user" is not defined` when `db_data:include ./db.synx` is present. The validator now checks whether the alias target's root prefix is a key with `:include`/`:import` marker and suppresses the error.

- **Duplicate method definitions in C# and JS/TS** — removed accidental duplicate `LoadFileAsync`/`SaveFileAsync`/`FromJson` in `SynxFormat.cs` and duplicate `saveSync`/`save` in `packages/synx-js/src/index.ts` (leftover from prior session).

- **`docs/dev/`** — contributor-facing stubs: [`docs/dev/plugins-roadmap.md`](docs/dev/plugins-roadmap.md) (registry / `!use` / package layout **TBD**; **no** plugin loader in `synx-core` yet).
- **Publish helper scripts** — `publish-cpp.bat`, `publish-csharp.bat`, `publish-swift.bat`, `publish-kotlin.bat`, `publish-mojo.bat` (see script headers for registry URLs and env vars). **`*.bat`** remains gitignored except **`publish-*.bat`** via `.gitignore` exception.
- **NuGet package ID (C#):** **`APERTURESyndicate.Synx`** — `Synx.Core` was already taken on nuget.org; project folder and `Synx` namespaces unchanged.

### Documentation

- **Install C# everywhere it matters:** root [`README.md`](README.md) — `dotnet add package APERTURESyndicate.Synx`, link [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx), pre-publish path (`dotnet add reference` / local `.nupkg` via `publish-csharp.bat`); maintainer notes for **`.\publish-csharp.bat`** (PowerShell), **`NUGET_API_KEY`** without angle brackets (avoids **403**), stale `artifacts/nuget/*.nupkg` cleanup; short **ready to push** checklist (verification scripts, no secrets, bump `Version` in `Synx.Core.csproj`).
- **Indexes:** [`docs/README.md`](docs/README.md), [`docs/SYNX_AT_A_GLANCE.md`](docs/SYNX_AT_A_GLANCE.md), [`docs/repository-layout.md`](docs/repository-layout.md), [`EDITING.md`](EDITING.md), [`parsers/README.md`](parsers/README.md), [`parsers/dotnet/README.md`](parsers/dotnet/README.md) — aligned on NuGet ID and publish script.
- **Guides:** EN/RU C# sections + **DE/ES/JA/ZH** installation blocks — same one-liner and package URL; full detail remains in `parsers/dotnet/README.md`.
- **Spec §5.9 (EN/RU):** C# section updated for **.NET 8**, **`SynxFormat`** API, **`SynxValue`** records, NuGet URL; removed obsolete **`SynxParser` / `packages/synx-csharp`** text; comparison table uses `SynxFormat.Parse(File.ReadAllText(...))`.
- **`docs/guides/GUIDE.md` (English):** replaced with a **from-scratch** practical manual — install matrix, static vs `!active`, syntax tutorial, per-parser sections (Rust, CLI, JS, Python, Node native, WASM, C, C++, Go, Swift, Kotlin, C#, Mojo), tools (VS Code, LSP), conformance/FAQ; normative detail remains in `SYNX-3.6-NORMATIVE.md` / `SPECIFICATION_EN.md`.
- **`docs/guides/GUIDE.md`:** **Parsers and bindings** section expanded with **call stacks**, **`Options` / key semantics**, and **function-by-function tables** per language (incl. npm `SynxOptions`, Py `synx_native`, napi exports, WASM limitations, C memory contract, FFI wrappers, C# `SynxFormat` / `SynxOptions`).

## Changes by Module

Quick reference of what was modified in recent versions:

| Version | Components Modified |
|---------|---|
| **3.6.1** | forum-api (soft delete, unyank, restore, permanent delete, DB migration), synx-cli (restore command, soft delete messaging), packages page (avatars, files tab, hidden empty tabs), S3 pipeline (tarball extraction, README/files storage) |
| **3.6.0** | synx-core (`.synxb`, `!tool`, **`!llm`**, `diff`, JSON Schema helpers, hostile-input caps), synx-cli (**`synx schema`**, **`json-validate`**), synx-lsp, VS Code (`!llm`), bindings (**C++**, **Go cgo**, **SwiftPM+`synx-c`**, **Kotlin/JNA+`synx-c`**, **Mojo↔CPython**), Python extra JSON/hex helpers for thin interop, conformance (**11** cases), tree-sitter, fuzz + **`Synx.FuzzReplay`**, **`SYNX_AT_A_GLANCE`**, SYNX-Adapter, docs: **normative spec + CORE-FREEZE (engine frozen 2026-04-01)** |
| **3.5.2** | synx-core (`:prompt` marker, `:vision`/`:audio` metadata, calc modulo-by-zero fix), synx-js (same + `Synx.diff()` + prototype pollution fix + ReDoS guard + `deepGet`/parser hardening), synx-vscode (diagnostics/completion for 3 new markers + `:template` sibling-scope fix), all 6 guides |
| **3.5.1** | synx-core (stack overflow guard, circular alias detection), synx-js (same + SynxError class + browser bundle export), synx-vscode (circular alias diagnostic), security tests |
| **3.5.0** | synx-core (path jail, depth limit, file size limit, calc length limit), synx-js (same), LICENSE (ethical use clause) |
| **3.4.0** | synx-core (`:spam` rate-limit marker), synx-js (`:spam` + strict error sync), VSCode (diagnostics/completion/navigation/preview for `:spam`), guides (all languages), version sync |
| **3.3.0** | synx-core (multi-parent inherit, calc dot-path, i18n plural, quoted strings, :import alias), VSCode (diagnostics/completion/navigation sync), documentation |
| **3.2.3** | synx-core (global [] constraints), documentation, version sync |
| **3.2.2** | synx-core (type validation), documentation |
| **3.2.1** | VSCode (diagnostics, syntax), Python binding, Node.js binding, WASM binding, C FFI, synx-core (serde), CI/CD, documentation |
| **3.2.0** | JS/TS engine, Rust engine, VSCode (completion, parser, syntax), guides (6 languages), documentation |
| **3.1.3** | VSCode extension, JS/TS API, documentation (6 guides), CLI tool, deployment examples |
| **3.1.2** | JS parser, Rust parser, VSCode extension, Node.js binding (napi), all guides |
| **3.1.0** | JS/TS API (runtime manipulation), Rust engine, VSCode extension, all guides |

---

## [3.6.0] — (SYNX Diagen) 2026-03-29

### Added

- **Rust CLI (`synx`):** New `crates/synx-cli` crate providing `synx parse`, `synx validate`, `synx convert`, `synx tool`, `synx compile`, `synx decompile`, `synx diff`, `synx query`, `synx format` commands. Built with clap v4; single static binary. Replaces the legacy Node.js CLI (`packages/synx-js/bin/synx.js` which was stuck at v3.2.0).
- **`diff` module (`synx-core`):** Structural diff between two parsed SYNX objects — `added`, `removed`, `changed`, `unchanged` keys. Public API: `Synx::diff()`, `diff_to_value()`.
- **`diff` in all bindings:** Node (`diff`, `diffJson`), Python (`diff`, `diff_json`), WASM (`diff`, `diff_object`), C FFI (`synx_diff`). Full API parity table updated.
- **`synx query` command:** dot-path query with array index support (e.g. `synx query server.host config.synx`, `synx query items.0 data.synx`).
- **Conformance test suite:** `tests/conformance/` — 11 canonical test cases (scalar types, nesting, arrays, type casting, comments, multiline, mixed structures, strings with spaces, empty values, tool mode, **`!llm` directive**). Rust runner as integration test: `cargo test -p synx-core --test conformance`.
- **Language Server (`synx-lsp`):** New `crates/synx-lsp` crate — LSP server over stdio using `tower-lsp-server`. Supports real-time diagnostics (tabs, odd indentation, unknown markers/constraints/types, duplicate keys, `!active` requirement), completion (markers, constraints, directives), and document symbols (full outline tree). Works in any LSP-capable editor: Neovim, Helix, Zed, Emacs, JetBrains (manual LSP config).
- **GitHub Action:** `.github/actions/synx/action.yml` — composite action to validate `.synx` files in CI using the Rust CLI. Inputs: `files` (glob), `strict` (bool), `version`.
- **Tree-sitter grammar:** `tree-sitter-synx/` subfolder with `grammar.js`, highlight queries (`queries/highlights.scm`), and `package.json`. Provides syntax highlighting for Neovim, Helix, Zed, Emacs, and future GitHub Linguist support.
- **Fuzz targets:** `crates/synx-core/fuzz/` with three `cargo-fuzz` targets (`fuzz_parse`, `fuzz_compile`, `fuzz_format`) exercising parser, binary codec, formatter, calc, and engine with arbitrary inputs. See `fuzz/README.md` for usage.
- **`!tool` directive (synx-core):** LLM tool call format. `!tool` reshapes output to `{ tool: "name", params: { ... } }`. Combined with `!schema`, produces `{ tools: [ { name, params } ] }` for tool definitions. Compatible with `!active` — markers resolve before reshaping. New API: `Synx::parse_tool()`, `reshape_tool_output()`.
- **Node.js binding:** `parseTool(text, options?)` for tool call parsing.
- **Python binding:** `parse_tool(text, env=None, base_path=None)` for tool call parsing.
- **WASM binding:** `parse_tool(text)` / `parse_tool_object(text)` for tool call parsing.
- **C FFI:** `synx_parse_tool()` for tool call parsing. Updated `synx.h` header.
- **C++ SDK (`bindings/cpp`):** `include/synx/synx.hpp` (C++17) — optional `std::string` / `std::vector` wrappers over **`synx-c`** / `synx.h` with **full API parity** (`parse`, `parse_active`, `stringify`, `format`, `parse_tool`, `compile`, `decompile`, `is_synxb`, `diff`). CMake example `synx_cpp_minimal` (`SYNX_C_LIBRARY`). Spec §5.7–5.8 and guides (EN/RU/DE/ES/JA/ZH), `README` binding table, and `parsers/README` updated; **still SYNX 3.6.0** (same engine as Rust).
- **Go binding (`bindings/go`):** cgo module `github.com/APERTURESyndicate/synx-format/bindings/go` — `Parse`, `ParseActive`, `Stringify`, `Format`, `ParseTool`, `Compile`, `Decompile`, `IsSynxb`, `Diff` over **`synx-c`**. Linux/macOS default `-L../../target/release -lsynx_c`; **Windows:** link `synx_c.dll.lib` via `CGO_LDFLAGS` + `CGO_LDFLAGS_ALLOW`, runtime `synx_c.dll` on `PATH`. Tests in `synx_test.go`. Spec §5.10 (EN/RU), guides, `README` parity row, repo layout — **3.6.0**.
- **Mojo (`bindings/mojo`):** `synx/interop.mojo` calls **`synx_native`** via Modular **Python from Mojo** — same engine as **`synx-core`** (not a pure Mojo grammar port). Demo `examples/demo.mojo`. **`synx-python`:** added `parse_active_to_json`, `parse_tool_to_json`, `stringify_json`, `compile_hex`, `decompile_hex`, `is_synxb_hex` for string-only boundaries; `synx-core` dependency enables **`serde`** for `stringify_json`. Spec §5.11 (EN/RU, renumbered following sections), guides (all locales), `README` table — **3.6.0**.
- **Swift (`bindings/swift`):** SwiftPM package **`Synx`** — `SynxEngine` wraps **`synx-c`** (`CSynx` + mirrored `synx.h`), `String`/`Data` API (`parse`, `parseActive`, `stringify`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb`). Unit tests under `Tests/SynxTests`. Spec §5.13 (EN/RU), comparison table, `README` parity row, repo layout, guides — **3.6.0**.
- **Kotlin/JVM (`bindings/kotlin`):** **`SynxEngine`** over **`synx-c`** via **JNA** (`com.aperturesyndicate:synx-kotlin`, `publishToMavenLocal`), Gradle **8.11** + Foojay toolchain resolver / **JDK 17**; CI **`kotlin-smoke`** in `bindings-smoke.yml`. Spec §5.12 (EN/RU), parity table, guides, `README` / layout — **3.6.0**.
- **6 new `!tool` tests** covering directive flags, call reshape, schema reshape, empty tool, and `!active` compatibility.
- **Binary format `.synxb` (synx-core):** Compact binary representation of SYNX data. Achieves 40%+ size reduction over text via string interning + deflate compression. Public API: `Synx::compile()`, `Synx::decompile()`, `Synx::is_synxb()`.
- **`binary` module (synx-core):** Wire format: 7-byte header (magic `SYNXB` + version + flags) + 4-byte uncompressed size + deflate-compressed payload. Payload uses string interning table with varint references and zigzag-encoded integers.
- **`--resolved` flag:** When `resolved=true`, `compile()` resolves all `:env`, `:ref`, `:calc` markers and strips metadata/includes — producing a fully-resolved snapshot.
- **Node.js binding:** `compile(text, resolved?)` → `Buffer`, `decompile(buf)` → `string`, `isSynxb(buf)` → `boolean`.
- **Python binding:** `compile(text, resolved=False)` → `bytes`, `decompile(data)` → `str`, `is_synxb(data)` → `bool`.
- **WASM binding:** `compile(text, resolved)` → `Uint8Array`, `decompile(data)` → `string`, `is_synxb(data)` → `boolean`.
- **C FFI:** `synx_compile()`, `synx_decompile()`, `synx_is_synxb()`, `synx_free_bytes()`. Updated `synx.h` header.
- **16 new binary tests** covering round-trip fidelity, all value types, metadata preservation, constraints, size reduction, error paths.
- **Size benchmark** in `benchmarks/rust/` comparing text vs binary on the 110-key production config.
- **`.NET` / C# parser (preview):** `parsers/dotnet/` — SDK-style library `Synx.Core` that parses static SYNX (same structural rules as `synx-core::parser::parse`), emits canonical JSON with **sorted object keys** (matching `synx_core::to_json`). `dotnet test` in `parsers/dotnet/` runs the same `tests/conformance/cases/*.synx` + `.expected.json` pairs as the Rust runner, including `!tool` reshape via `Synx.ParseTool` (no `!active` engine yet — tool cases that only need parse + reshape are covered).
- **Integrations (agents & editors):**
  - **MCP server** [`integrations/mcp/synx-mcp/`](integrations/mcp/synx-mcp/) — stdio MCP with `validate` / `parse` / `format` tools; companion docs in [`integrations/mcp/README.md`](integrations/mcp/README.md) and [`docs/claude.md`](docs/claude.md).
  - **AI adapters** [`integrations/ai/synx-adapter/`](integrations/ai/synx-adapter/) — Python and Node helpers (`pack_for_llm` / `packForLlm`, JSON size estimates); index in [`integrations/ai/README.md`](integrations/ai/README.md).
  - **Sublime Text** — [`integrations/sublime-text/Synx/`](integrations/sublime-text/Synx/).
  - **Neovim** — [`integrations/neovim/synx.nvim/`](integrations/neovim/synx.nvim/).
- **LSP crate docs:** [`crates/synx-lsp/README.md`](crates/synx-lsp/README.md) — install and editor hookup for `synx-lsp`.
- **MCP filesystem tools:** [`integrations/mcp/synx-mcp/`](integrations/mcp/synx-mcp/) — `synx_read_path`, `synx_write_path` (atomic temp + rename), `synx_apply_patch` (ordered unique substring replacements); gated by **`SYNX_MCP_ROOT`** or comma-separated **`SYNX_MCP_ROOTS`** (10 MB cap per file).
- **Docs for Claude / Anthropic:** [`docs/anthropic-system-prompt.txt`](docs/anthropic-system-prompt.txt), [`docs/anthropic-token-notes.md`](docs/anthropic-token-notes.md) (SYNX vs JSON tokenizer sample comparison), [`docs/claude.md`](docs/claude.md).
- **Benchmarks:** Node + Python runners now include **XML** (JSON-derived tree) alongside JSON/YAML/SYNX; Node uses `fast-xml-parser`.
- **`synx-adapter` CLI:** `synx-context` — Python [`synx_adapter.cli_compress`](integrations/ai/synx-adapter/python/synx_adapter/cli_compress.py), Node bin; optional **`--xml` / `--xml-tag`** wraps output in `<synx_data>` (CDATA) so Claude sees XML boundaries while the payload stays SYNX. Adapter [`README.md`](integrations/ai/synx-adapter/README.md) in English.
- **C# `Synx.Core`:** `!active` resolution engine (markers, `[]` constraints metadata, `!include` list, `:calc` / `:env` / `:ref` / `:alias` / `:random` / `:i18n` / interpolation, etc.) via `SynxFormat.ParseActive` / `ParseFullActive`; `SynxValue.Secret`; extra tests in `EngineActiveTests`.
- **C# fuzz replay (`Synx.FuzzReplay`):** [`parsers/dotnet/tools/Synx.FuzzReplay/`](parsers/dotnet/tools/Synx.FuzzReplay/) — replay corpus / `minimized-from-*` through `Parse` + `ToJson` (strict UTF-8, aligned with Rust `fuzz_parse`). Documented in [`crates/synx-core/fuzz/README.md`](crates/synx-core/fuzz/README.md).
- **Python fuzz replay:** [`crates/synx-core/fuzz/scripts/synx_fuzz_replay.py`](crates/synx-core/fuzz/scripts/synx_fuzz_replay.py) — `synx_native` corpus replay (`parse`, `parse_to_json`, `stringify`, `parse_active`, `parse_tool`); same UTF-8 filter as Rust.
- **Fuzz coverage report (llvm-cov):** `crates/synx-core/fuzz/coverage/fuzz_parse/html/` — pruned to remove 0.00% line-coverage rows/pages (unused files), keeping the report focused on executed code.
- **Expanded `fuzz_parse` corpus:** Added an additional **7,177** interesting inputs under `crates/synx-core/fuzz/corpus/fuzz_parse/` from a long fuzzing run.
- **Vendor verification scripts:** [`scripts/verify-release-quality.ps1`](scripts/verify-release-quality.ps1), [`scripts/verify-release-quality.sh`](scripts/verify-release-quality.sh) — `cargo test -p synx-core`, `dotnet test`, FuzzReplay on conformance `.synx`, optional bench build.
- **`docs/SYNX_AT_A_GLANCE.md`:** single-page map (layout, parsers, benchmarks, AI/Claude, quality gates).
- **SYNX-Adapter — long context & Anthropic:** `make_anchor_index` / `inject_section_anchors`, `pack_for_llm` / `packForLlm` anchor options; CLI `--anchor-index`, `--section-anchors`, `--anchor-prefix`; **`synx_adapter.anthropic_tools`** and **`@aperturesyndicate/synx-format-adapter/anthropic`**.
- **`docs/guides/long-context-synx.md`:** SYNX + anchors vs lost-in-the-middle; Anthropic tool-result notes.
- **Claude Artifacts:** [`integrations/artifacts/synx-visualizer/`](integrations/artifacts/synx-visualizer/) — `SynxVisualizer` React snippet.
- **`!llm` directive (synx-core):** Optional top-of-file marker for LLM-oriented envelopes; sets `ParseResult.llm`. Parsed **data tree is unchanged** vs omitting the line (conformance: `011-llm-directive`). **`.synxb`:** header flag bit 6; `Synx::decompile` / canonical `format` emit `!llm` when missing from body.
- **`synx-core` `schema_json`:** `metadata_to_json_schema()`, `value_to_json_value()`, and (with feature `jsonschema`) `validate_with_json_schema` / `validate_serde_json` (draft 2020-12).
- **`synx` CLI:** `synx schema`, `synx json-validate`, `synx validate --self-schema` / `--json-schema` (bundled JSON Schema validation).
- **JS:** `Synx.schema()` no longer emits invalid per-property `required`; `parseData()` exposes `llm` when `!llm` is present.
- **synx-lsp / VS Code:** directive completion + TextMate grammar + editor parser skip for `!llm`.

### Frozen engine (2026-04-01)

The **SYNX 3.6.0** language surface in [`docs/spec/SYNX-3.6-NORMATIVE.md`](docs/spec/SYNX-3.6-NORMATIVE.md) and **`synx-core`** parse → canonical JSON behaviour covered by **`tests/conformance/`** are **frozen**. **PATCH** (`3.6.z`) = spec restoration and compatibility fixes only; non-additive language changes require a new normative version. See [`docs/spec/CORE-FREEZE.md`](docs/spec/CORE-FREEZE.md) and the notice in the root [`README.md`](README.md).

### Changed

- **synx-core (hostile input):** Parser/engine/stringify bounds for long fuzz runs — UTF-8 prefix cap, line-table cap, indentation nesting **128**, multiline block / list / `!include` limits, JSON/stringify/format depth caps, engine scratch string cap; see [`fuzz/README.md`](crates/synx-core/fuzz/README.md). **C#** deep-nesting test aligned with parser limits.
- **synx-core (format patterns):** Clamp `%Nd/%0Nd` integer width and `%.Nf/%.Ne` float precision to prevent `std::fmt` panics like **“Formatting argument out of range”** under hostile/fuzzed inputs.
- **Repository layout:** `_guides/` moved to `docs/guides/`; normative specification moved to `docs/spec/`; Criterion crate path is `benchmarks/rust/` (still package name `synx-bench`); LLM benchmark long-form guide is `benchmarks/llm-tests/GUIDE.md`. Added `docs/README.md` and `docs/repository-layout.md`. Removed root `playground-mini-parser.js` (obsolete subset parser). `packages/synx-js` ships a short `SPECIFICATION.md` pointer; full spec files live under `docs/spec/`.

### Dependencies

- Added `miniz_oxide 0.8` (pure-Rust deflate, zero transitive deps) for payload compression.
- Added `clap 4` for CLI argument parsing (`synx-cli` crate).
- Added `tower-lsp-server 0.23` + `tokio 1` for LSP server (`synx-lsp` crate).
- Added `libfuzzer-sys` for fuzz targets (`crates/synx-core/fuzz/`).
- Added `serde_json` + optional `jsonschema` (with feature `jsonschema`) for schema export/validation in `synx-core` / `synx-cli`.

---

## [3.5.2] — 2026-03-28

### Added

- **`:prompt` marker (Rust + JS):** Formats a subtree into a labeled SYNX code fence for LLM consumption. Usage: `key:prompt:Label` produces a string like `"Label (SYNX):\n\`\`\`synx\n...\n\`\`\`"`. No network calls — purely a serialization transform.
- **`:vision` metadata marker (Rust + JS):** Marks a key as image-generation intent. Pass-through marker preserved for application-layer processing.
- **`:audio` metadata marker (Rust + JS):** Marks a key as audio-generation intent. Pass-through marker preserved for application-layer processing.
- **`Synx.diff(a, b)` (JS):** Static method that compares two parsed SYNX objects and returns a structured diff with `added`, `removed`, `changed` (with `from`/`to`), and `unchanged` fields.
- **`SynxDiff` type (JS):** TypeScript interface for the diff result, exported from the package.
- **VS Code:** IntelliSense autocomplete and documentation for `:prompt`, `:vision`, `:audio` markers.
- **VS Code:** Diagnostics recognize `:prompt`, `:vision`, `:audio` as valid markers.
- **Guides:** All 6 language guides (EN, RU, DE, ES, JA, ZH) updated with new marker documentation and `Synx.diff()` section.

### Internal

- `stringify_value()` helper added to Rust engine for `:prompt` serialization.
- `stringifyValue()` helper added to JS engine for `:prompt` serialization.
- `deepEqual()` helper added to JS for `Synx.diff()` value comparison.
- 3 new Rust engine tests: `test_prompt_marker`, `test_vision_marker_passthrough`, `test_audio_marker_passthrough`.
- Marker count updated from 21 to 24 across all documentation.

### Fixed

- **VS Code: `:template` false-positive warning for sibling keys.** The diagnostic `"Key referenced in template is not defined"` only checked root-level `keyMap` (dot-paths), so `{sibling}` inside a nested `:template` always flagged as missing. Now checks both root path and sibling scope (`parent.sibling`).
- **Python binding CI smoke tests.** `pyo3`'s `extension-module` mode is now feature-gated so binding builds keep extension semantics by default, while `cargo test` in CI can run with `--no-default-features` and avoid unresolved embedded-Python entrypoints.
- **Bindings workflow stability.** `bindings-smoke.yml` now runs the Python smoke job with `--no-default-features`, prints/verifies the built Node native artifact on Windows, and executes the Node smoke test from the binding directory directly instead of going through `npm run`.
- **JS: Prototype pollution via `Synx.set()` / `Synx.add()` / `Synx.remove()`** (Security). Path segments `__proto__`, `constructor`, `prototype` are now rejected with an error.
- **JS: `deepGet()` followed prototype chain** (Security). Lookups like `constructor` or `toString` could reach inherited properties. Now uses `hasOwnProperty` checks at every traversal step.
- **JS: `__proto__` key injection in parser** (Security). A `.synx` file with a key named `__proto__` could corrupt the parsed object's prototype. Such keys are now silently skipped.
- **JS: ReDoS via constraint `[pattern:]`** (Security). User-supplied regex patterns longer than 128 characters are now rejected to prevent catastrophic backtracking.
- **JS: SPAM_BUCKETS memory leak.** Expired bucket entries (empty timestamp arrays) are now removed from the map instead of persisting indefinitely.
- **Rust + JS: `:calc` modulo by zero.** `expr % 0` now returns `CALC_ERR: division by zero` instead of producing `NaN` (JS) or `NaN` (Rust).

---

## [3.5.1] — 2026-03-20

### Fixed

- **engine (Rust + JS):** Prevent stack overflow when parsing objects with more than 512 levels of nesting. Values beyond the limit are replaced with `NESTING_ERR: maximum object nesting depth exceeded`.
- **engine (Rust + JS):** `:alias` markers now detect self-referential and one-hop circular references. Circular aliases produce `ALIAS_ERR: circular alias detected: <key> → <target>` instead of silently returning wrong values.
- **engine (Rust + JS):** Fixed false-positive `ALIAS_ERR` when a plain string value happened to equal the aliasing key's name. Cycle detection now uses metadata to confirm the target key is itself an alias.

### Added

- **JS:** `SynxError` typed class for strict mode. When `{ strict: true }` is passed, SYNX throws a `SynxError` (extends `Error`) with a `.code` field (e.g. `"CALC_ERR"`, `"ALIAS_ERR"`) instead of a generic `Error`.
- **JS:** `ALIAS_ERR` and `NESTING_ERR` added to strict mode error prefix list.
- **VS Code:** Circular alias diagnostic — editor now shows a warning when `:alias` references form a cycle, without needing to run the engine.
- **Tests:** New `security.test.ts` with 31 edge-case and security tests covering deep nesting, circular aliases, empty input, Unicode, `:calc` limits, type casting, CRLF line endings.

### Internal

- `resolve_value` (Rust) takes `depth: usize` parameter and checks against `MAX_RESOLVE_DEPTH = 512`.
- `resolve()` (JS) takes `_resolveDepth` parameter checked against `MAX_RESOLVE_DEPTH = 512`.
- `apply_markers` (Rust) `_path` renamed to `path`, `_metadata` renamed to `metadata` for active use in alias cycle detection.

---

## [3.5.0] - (SYNX Socrate) 2026-03-08

### Security Hardening

All engines (Rust core + JS/TS) now include built-in security protections. No functionality is removed — safe defaults are applied automatically.

- **Path jail (filesystem sandbox)**: `:include`, `:import`, `:watch`, `:fallback` file paths are resolved relative to `basePath` and cannot escape it. Absolute paths and `../` traversal beyond the project root are rejected.
- **Include depth limit**: Nested `:include`/`:import`/`:watch` calls are limited to 16 levels deep (configurable via `maxIncludeDepth` / `max_include_depth`). Prevents infinite recursion and circular include DoS.
- **File size limit**: Included/watched files larger than 10 MB are rejected to prevent memory exhaustion.
- **Calc expression length limit**: `:calc` expressions longer than 4096 characters are rejected to prevent parser abuse.
- **Env isolation**: When `env` option is provided explicitly, only that map is used — no fallthrough to `process.env` / `std::env`.

### Added
- `max_include_depth` option (Rust `Options`) and `maxIncludeDepth` option (JS `SynxOptions`) — configurable max nesting depth for file operations (default: 16).
- `jail_path()` / `jailPath()` internal helpers — canonicalize and validate paths against base directory.
- `check_file_size()` / `checkFileSize()` internal helpers — reject files exceeding 10 MB.

### Changed
- **LICENSE**: Added ethical use notice — SYNX must not be used for unauthorized data exfiltration, credential theft, or causing harm.

---

## [3.4.0] - (SYNX Classic) 2026-03-08

### Added
- **`:spam` marker** (Rust engine + JS engine + VSCode preview parser): `key:spam:MAX_CALLS[:WINDOW_SEC] target` limits how often a target can be resolved inside a time window. If `WINDOW_SEC` is omitted, it defaults to `1`.
- **Rate-limit error surface**: Engines now emit `SPAM_ERR: ...` when the limit is exceeded.
- **VSCode support for `:spam`**: marker recognition in diagnostics, argument validation (`MAX_CALLS`, `WINDOW_SEC`), completion docs/snippet, and go-to-definition support for `:spam` target references.

### Changed
- **Strict runtime mode update (JS)**: `SPAM_ERR` is now treated as a strict-mode runtime error prefix (same fail-fast behavior as `INCLUDE_ERR`, `WATCH_ERR`, `CALC_ERR`, `CONSTRAINT_ERR`).
- **Version sync to `3.4.0`** across core and binding/package manifests.
- **Guide updates**: added `:spam` marker documentation to all language guides.

---

## [3.3.0] - 2026-03-08

### Added
- **Multi-parent `:inherit`** (Rust engine): A block can now inherit from multiple parents via `:inherit:_parent1:_parent2:_parent3`. Parents merge left-to-right (later parents override earlier ones), child fields override all. Enables mixin-style composition for templates.
- **`:calc` dot-path references** (Rust engine): Arithmetic expressions now support nested key references via dot-path syntax (e.g., `total:calc stats.base_hp * stats.multiplier`). Previously only flat root-level keys were resolved.
- **`:i18n` pluralization** (Rust engine): Added CLDR-based plural form selection via `:i18n:COUNT_FIELD` syntax. The language entry contains plural category keys (`one`, `few`, `many`, `other`), and the engine selects the correct form based on the count value. `{count}` placeholder is auto-replaced with the actual number. Supported languages: en, de, es, it, fr, pt, ru, uk, be, pl, cs, sk, ar, ja, zh, ko, vi, th.
- **Quoted string values** (Parser): Wrapping a value in double or single quotes preserves it as a literal string, bypassing auto-casting. `status "null"` → string `"null"` (not null), `enabled "true"` → string `"true"` (not boolean), `count "42"` → string `"42"` (not integer).
- **`:import` marker alias** (Rust engine): `:import` is now a recognized alias for `:include` (key-level file embedding). Recommended to use `:import` to avoid confusion with the `!include` directive (file-level interpolation).
- **Import comparison matrix** (Guide): Added a table in GUIDE.md comparing `!include` (directive) vs `:include`/`:import` (marker) — syntax, placement, behavior, and use cases.
- **7 new tests**: `test_multi_parent_inherit`, `test_calc_dot_path`, `test_i18n_plural_en`, `test_i18n_plural_en_one`, `test_i18n_plural_ru`, `test_quoted_null_preserved`, `test_unquoted_null_is_null`.

### Changed
- **`:inherit` engine rewrite**: `apply_inheritance()` now collects all markers after "inherit" as parent names instead of just one. Backward compatible — single-parent `:inherit:_parent` works unchanged.
- **`:calc` engine enhancement**: Variable substitution now includes a second pass for dot-path identifiers using `deep_get()` traversal after flat key substitution.
- **Guide updates**: Updated `:inherit`, `:calc`, `:i18n`, `:include` sections with new features, examples, and the import matrix table. Added quoted values documentation to Basic Syntax.
- **Version sync to `3.3.0`** across all manifests.

---

## [3.2.3] - 2026-03-08

### Added
- **Global `[]` constraint validation** (Rust engine, active mode): Constraints declared with square brackets now apply consistently across all matching field names in the resolved tree, including inherited fields from `:inherit` templates.
  - Supports global enforcement of `required`, `min`, `max`, `type`, and `enum`
  - Constraint rules are collected into a global registry and recursively applied after marker resolution
  - Violations are surfaced as `CONSTRAINT_ERR: ...` values for visibility in output and downstream checks
- **Constraint merge strategy for repeated field declarations**: When the same field is constrained in multiple places, strict merging is applied (`required`/`readonly` propagate, `min` picks higher bound, `max` picks lower bound).
- **Engine test coverage for global constraints**: Added tests for inherited range validation and required validation:
  - `test_constraint_validation_inherited_range`
  - `test_constraint_validation_required`
- **Guide update near type hints**: Added `Constraint Validation ([]) in Active Mode` section in `_guides/GUIDE.md` with examples for `[required, min:1, max:50000]`, `type`, and `enum`.

### Changed
- **Version sync to `3.2.3`** across core manifests and package manifests used by bindings/extensions.

---

## [3.2.2] - 2026-03-08

### Added
- **Global type validation** (Rust engine, active mode): When you define a field with an explicit type like `hp(int)` or `name(string)`, the engine now validates that **all uses of that field across the entire document match the declared type**. Once a type is registered (e.g., "hp is int"), any value later assigned to that field is checked against the registered type.
  - In `!active` mode, type registry is built from all `key(type)` declarations
  - All field values are validated recursively through the entire value tree
  - Type mismatches are replaced with `TYPE_ERR: 'field' expected TYPE but got ACTUAL` for visibility
  - Benefits: Ensures consistency across blocks (especially with `:inherit`), self-documenting code, early error detection
- **Type validation test coverage**: Added two tests (`test_type_validation`, `test_type_validation_error`) to the synx-core engine test suite to verify correct type matching and error reporting.
- **Type validation documentation** (GUIDE.md): Added "Type Validation (Active Mode)" section under "Type Casting" with examples of valid/invalid type usage and error handling.

---

## [3.2.1] - 2026-03-08

### Added
- **Python binding: `parse_active` now accepts options** — `env` (dict) and `base_path` (str) parameters for `:env` and `:include` marker resolution. Previously `parse_active` used only defaults.
- **Python binding: `stringify`** — converts a Python dict/list back to SYNX format text.
- **Python binding: `format`** — reformats SYNX text into canonical form (sorted keys, normalized indentation).
- **Node.js binding: `stringify`** — converts a JS object back to SYNX format text.
- **Node.js binding: `format`** — reformats SYNX text into canonical form.
- **WASM binding: `parse_object` / `parse_active_object`** — returns JS objects directly via `serde_wasm_bindgen`, eliminating the need for `JSON.parse()` on the consumer side.
- **WASM binding: `stringify`** — converts JSON string to SYNX format text.
- **WASM binding: `format`** — reformats SYNX text into canonical form.
- **C FFI binding: `synx_stringify`** — converts JSON string to SYNX format text. Caller must free with `synx_free`.
- **C FFI binding: `synx_format`** — reformats SYNX text into canonical form. Caller must free with `synx_free`.
- **`serde` feature on `synx-core`** — optional `Serialize`/`Deserialize` derives on `Value` enum (used by WASM and C FFI bindings for JSON round-tripping).
- **Binding API parity table in README** — documented function availability and behavior notes for Rust core, JS package, Python, Node native, WASM, and C FFI.
- **Bindings smoke tests** — lightweight smoke coverage for Python/C/Node/WASM binding surfaces (`parse`, `parse_active`, `stringify`, `format`) without adding runtime overhead to production code paths.
- **CI check matrix** — new GitHub Actions workflow `.github/workflows/bindings-smoke.yml` to run binding-level checks on each push/PR.
- **C header ownership docs** — `bindings/c-header/include/synx.h` now explicitly documents allocation/free contract and NULL-on-error behavior for FFI consumers.

### Fixed
- **VSCode `:inherit` validation** — diagnostics now check that the parent block key exists when using `:inherit:parent_key`. Previously, the error was only shown at parse time, not in the editor.
- **VSCode block comment `###` syntax highlighting** — entire content of block comments is now properly highlighted as comment text. Previously, the first word on a line inside the block could be highlighted as a key instead of comment text.

## [3.2.0] - 2026-03-09

### Added
- **`:ref` marker** (JS + Rust + VSCode): Value reference with marker chaining. Like `:alias` but feeds the resolved value into subsequent markers. Supports shorthand calc: `rate:ref:calc:*2 base_rate` resolves `base_rate` (50), then computes `50 * 2 = 100`.
- **`:inherit` marker** (JS + Rust + VSCode): Block-level field inheritance. Merges all fields from a parent block into the child, with child values taking priority. Use `_` prefix for private template blocks excluded from output: `_base_resource` defines defaults, `steel:inherit:_base_resource` inherits them.
- **`:i18n` marker** (JS + Rust + VSCode): Multilingual values with language selection. Nested keys are language codes (`en`, `ru`, `de`), selected via `options.lang`. Falls back to `en`, then first available value. Syntax: `title:i18n` with child keys per language.
- **Auto-`{}` interpolation** (JS + Rust): In `!active` mode, any string value containing `{key}` is automatically interpolated — no `:template` marker needed. Supports dot-path for nested access (`{server.host}`). The `:template` marker is kept as a recognized no-op for backward compatibility.
- **`!include` directive** (JS + Rust + VSCode): File-level directive `!include ./file.synx [alias]` imports another file's top-level keys for use in `{key:alias}` interpolation. Alias is auto-derived from filename if not provided. Supports `{key:alias}` for named includes and `{key:include}` shorthand when only one file is included.
- **Comment string highlighting** (VSCode extension): Double-quoted `"strings"` and single-quoted `'strings'` inside comments now have distinct colors — orange for `""`, light blue for `''`.

### Fixed
- **Block comment content highlighting** (VSCode extension): Content inside `###` block comments was not highlighted as comments — only the `###` delimiters were colored. Fixed TextMate grammar to apply comment scope to all content between fences.

## [3.1.3] - 2026-03-08

### Added
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.
- **Deployment example (Docker + Nginx + Redis)**: Added runnable stack example in `examples/docker-stack` with SYNX-driven config and generated Nginx upstream config.
- **CLI tool** (`synx`): New CLI with 4 commands — `synx convert` (export to JSON/YAML/TOML/.env), `synx validate` (strict-mode check for CI/CD), `synx watch` (live reload with `--exec` support), `synx schema` (extract constraints as JSON Schema). Installed globally via `npm install -g @aperturesyndicate/synx-format`.
- **Export formats** (JS/TS API): `Synx.toJSON()`, `Synx.toYAML()`, `Synx.toTOML()`, `Synx.toEnv()` — convert parsed SYNX config to standard formats without external dependencies.
- **File watcher** (JS/TS API): `Synx.watch(filePath, callback, options)` — monitors `.synx` files for changes and delivers hot-reloaded config via callback.
- **Schema export** (JS/TS API): `Synx.schema(text)` — extracts constraint annotations (`[required, min:N, max:N, type:T, enum:A|B, pattern:R]`) as a JSON Schema-compatible object.
- **Deployment guide** (all 6 language guides): Docker, Docker Compose, Nginx, Redis, PostgreSQL, K8s Secrets, Vault, Helm, Terraform, CI/CD validation — added to GUIDE.md and all translations (DE, ES, JA, RU, ZH).


### Changed
- **Syntax highlighting redesign** (VSCode extension): Improved TextMate grammar with semantic scopes for clarity. Parent nodes (with nesting) use `entity.name.section` (bright, bold) to highlight structural branches. Leaf nodes (with values) use `support.type.property-name` (calm, subtle) for actual properties. Markers now `keyword.control.marker.synx` (pink/red). Recursive depth coloring: level 0 `keyword.control` (pink), level 1 `entity.name.tag` (bright cyan), level 2 `entity.name.function` (yellow), level 3+ `variable.parameter` (light cyan). This creates visual hierarchy—structure jumps out, data stays quiet.
- **JS native engine parity**: Large-file native path now forwards `Synx.parse(..., options)` into `parseActive(text, options)`, so `env` and `basePath` behave the same as the pure-JS path.

### Fixed
- **Fail-fast production mode**: Added `strict` option to JS API (`Synx.parse/loadSync/load`) to throw when runtime marker resolution returns `INCLUDE_ERR`, `WATCH_ERR`, `CALC_ERR`, or `CONSTRAINT_ERR` strings.

## [3.1.2] - 2026-03-07

### Fixed
- **`:default` value truncation** (JS parser + VSCode extension): The marker regex `([\w:]+)` only allowed word characters and colons, truncating default values containing dots (IPs like `0.0.0.0`), hyphens (`dev-secret-key`), or operators (`>=`). Changed to `([^\s]+)` to capture the full marker chain up to the next whitespace. This also fixes `:version:>=:18.0` and `:clamp` markers with decimal bounds.
- **`:default` compound values** (all engines — Rust, JS, VSCode): When `:default:VALUE` contained colons (e.g. `0.0.0.0` split as `["0","0","0","0"]` after `:` split), only the first fragment was used as the fallback. Now joins all marker parts after `default` back with `:` to reconstruct the original value.
- **`(string)` type hint ignored by `:default`/`:env`** (Rust + JS engines): A key with `(string)` type hint like `host(string):env:default:0.0.0.0 HOST` would still auto-detect the default value as a number. Now respects the type hint and returns the raw string.
- **VSCode `:env` without `:default` fallback**: The VSCode parser's `:env` handler didn't check for a `:default` sibling marker when the environment variable was missing. Now correctly falls back to the default value.
- **VSCode false "duplicate key" diagnostics in lists**: Keys inside different list-of-objects items (e.g. `category`, `name`, `price` repeated in each `- item`) were flagged as duplicates because scope tracking didn't reset at list item boundaries. Now clears deeper indent scopes when a new list item is encountered.
- **`.gitignore` encoding**: File was UTF-16 (BOM `FF FE`) which git cannot read, causing all ignored files to appear as untracked. Re-encoded to UTF-8 without BOM.

### Added
- **Node native binding: `parseActive` options** (`bindings/node`): `parseActive(text, options?)` now accepts an optional options object with `env` (environment variable overrides) and `basePath` (for `:include` resolution), matching the JS package API.
- **`typeHint` in JS metadata**: The `SynxMeta` interface now includes an optional `typeHint` field, allowing engine markers to respect explicit type casts like `(string)`.
- **Nesting-level key coloring** (VSCode extension): Keys are now colored by indentation depth — each nesting level gets a distinct color (blue → teal → yellow → purple → orange → gold), with separate palettes for dark and light themes.
- **Block comments `###`** (all parsers — Rust, JS, VSCode): Multi-line comments using `###` fences. Everything between an opening `###` and closing `###` is ignored by the parser.
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.

### Changed
- Added `*.bat` and `.claudeignore` to `.gitignore`.

## [3.1.0] - 2026-03-07

### Fixed
- **Critical**: Removed `/n` → newline replacement from JS parser (`parser.ts`) and Rust core parser (`parser.rs`). The code was corrupting any value containing the two-character sequence `/n` (e.g. URLs like `/newsletter`, `/nginx`, `/node`) in multiline blocks and list items by replacing it with an actual newline character. This behavior was not part of the SYNX spec.
- **Security**: Removed `SynxSecret.valueOf()` from the JS engine. `valueOf()` returned the real secret value, meaning secrets could leak in arithmetic contexts or coercion (e.g. `secretVal + 1`). Only `.reveal()` should expose the underlying value.
- **Dead code**: Removed `SECRET_TAG = Symbol('synx:secret')` from the JS engine. The symbol was set on every `SynxSecret` instance via `Object.defineProperty` but was never read anywhere.
- **Silent coercion**: Fixed `castType` in JS parser — `parseInt(raw, 10) || 0` was silently returning `0` for any invalid or falsy numeric input (e.g. `(int)0` returned `0` but so did `(int)abc`). Changed to explicit `isNaN` check.
- **`:watch` nested key lookup**: `extractFromFileContent` in the JS engine was doing a flat line-by-line scan for `.synx` source files, so `:watch:database.host` always returned `null` for nested keys. Now uses `parseData` + `deepGet` for correct dot-path resolution.
- **`.synx.lock` format in docs**: The English guide showed a JSON block as the lock file format, but the actual implementation writes plain `key value` text (one per line). Corrected in `_guides/GUIDE.md`.
- **`:calc` ordering**: Added inline comment clarifying that `:calc` expressions only see already-resolved numeric siblings — keys that appear later in file order and still hold unresolved marker values are not available. This was a silent failure with no prior indication.

### Changed
- `Synx.format()` canonical formatter added to JS (`packages/synx-js`) and Rust (`crates/synx-core`): sorts keys alphabetically at every level, normalizes indentation to 2 spaces, strips comments, adds blank lines between top-level blocks. Useful for deterministic git diffs and pre-commit hooks.
- All language guides (`_guides`) updated with `format()` usage examples and pre-commit hook script.

## [3.1.0] - 2026-03-06

### Added
- Type-cast random generation in parsers:
- `(random)` and `(random:int)` for integer values
- `(random:float)` for float values
- `(random:bool)` for boolean values
- Runtime config manipulation API in JS/TS:
- `Synx.get(obj, keyPath)`
- `Synx.set(obj, keyPath, value)`
- `Synx.add(obj, keyPath, item)`
- `Synx.remove(obj, keyPath, item?)`
- `Synx.isLocked(obj)`
- `!lock` directive support to protect parsed configs from external runtime mutation through the JS/TS API.
- Delimiter keyword support for `slash` in marker processing (`:split` / `:join`).
- Root spelling dictionary config (`cspell.json`) for SYNX-specific terms.

### Changed
- JS and Rust parser type-hint regex now supports `:` in cast names (for example `(random:int)`).
- VS Code extension completion behavior improved:
- marker snippets no longer inject noisy placeholders by default
- `!active` completion after `!` no longer produces `!!active`
- added `!lock` completion and random cast completions
- VS Code diagnostics updated to recognize random type-casts as valid.
- VS Code parser updated to ignore `!lock` directive line as a directive, not a key.
- VS Code syntax grammar updated to highlight `!lock`.
- VS Code extension package version set to `3.1.0`.

### Fixed
- Documentation and runtime behavior aligned for `:join:slash` by adding actual `slash` delimiter support in engines.
- Type diagnostics mismatch for random casts in VS Code extension.

### Documentation
- Guides updated in all supported languages (`_guides`):
- random cast section
- lock mode section
- runtime manipulation examples
- marker compatibility section
- Python access-helper equivalents (`get_path` / `set_path` / `add_path` / `remove_path`) with note that native Python API currently exposes `parse`, `parse_active`, `parse_to_json`
- delimiter keyword lists synchronized in `split` and `join` sections
- Removed "view logo" button lines from GitHub guides while keeping GIF demos.
- VS Code README Full Specification section expanded with links to all language guides and specification files.
- Added extension-scoped changelog: `packages/synx-vscode/CHANGELOG.md`.

### Tooling and Release Scripts
- `publish-npm.bat` improved for safer execution:
- path auto-detection
- optional version bump argument
- better npm auth flow (`npm login` / `NPM_TOKEN`)
- explicit `call npm ...` usage on Windows
- clearer error output
- Added package-local publish helper: `packages/synx-js/publish-npm.bat`.

## [3.0.0] - Original

### Added
- Initial public release of SYNX format and parser/runtime ecosystem.
- Core marker system, constraints, and `!active` processing pipeline.
- Rust core crate and bindings/packages for JS/TS, Python, and VS Code tooling.

---

<div align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/asp_128.png" width="128" height="128" />
  <p>Made by <strong>APERTURESyndicate Production</strong></p>
</div>
