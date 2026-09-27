//! Randomised property tests for SYNXL.
//!
//! `crates/synx-core/fuzz/fuzz_targets/fuzz_synxl.rs` asserts the same
//! invariants under libFuzzer, which explores far deeper — but it needs a
//! nightly toolchain and a sanitizer runtime, so it does not run in the ordinary
//! `cargo test` path and never gates a commit. This does.
//!
//! The generator is a deterministic LCG over the constructs the audit found
//! problems in: Unicode indentation, multiline bodies holding comment and
//! directive lines, quote fallbacks, integers around the i64 edge, mid-file
//! field-list changes, and reserved prefixes. A failure prints the seed and the
//! document, so any counterexample is one line to reproduce.

use synx_core::synxl;

/// Deterministic PRNG — a fixed seed has to give a fixed corpus, or a failure
/// cannot be reproduced from the message.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG constants.
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Fragments chosen because each one broke at least one implementation.
const VALUES: &[&str] = &[
    "plain",
    "",
    "42",
    "-0.0",
    "9223372036854775807",
    "9223372036854775808",
    "1526947345124249601",
    "true",
    "null",
    "\"quoted\"",
    "\"a\"b\"",
    "'single'",
    "a // b",
    "a # b",
    "has;semicolon",
    "\u{a0}leading-nbsp",
    "trailing-nbsp\u{a0}",
    " ascii-space ",
    "#hash-first",
    "//slash-first",
    "!bang-first",
    "-dash-first",
    "[bracket",
    "(paren",
    ":colon",
    "@at",
    "/var/log/app",
    "emoji-\u{1f600}",
    "\u{0085}nel",
    "\u{feff}bom",
];

const BLOCK_LINES: &[&str] = &[
    "  k v",
    "  k",
    "    nested 1",
    "  list",
    "    - a",
    "    - b",
    "  body |",
    "      line one",
    "      # comment-shaped",
    "      // slash-shaped",
    "      ###",
    "      !include /etc/passwd",
    "      !active",
    "  body |+",
    "      indented",
    "        deeper",
    // Indents that mix widths and Unicode whitespace. `|+` locks its strip
    // prefix to the first continuation line and slices every later line at
    // that byte count, so a line whose indent is *narrower* in characters but
    // built from multi-byte whitespace puts the cut inside a character — the
    // exact shape of the audit's first panic.
    "   \u{a0}narrower-unicode-indent",
    "     \u{a0}\u{a0}wider-unicode-indent",
    "    \u{3000}ideographic-space",
    "\t\u{a0}tab-then-nbsp",
    "\u{a0}nbsp-indent v",
    "  n(random) x",
    "  __proto__ boo",
    "  -",
    "",
];

const FIELD_DECLS: &[&str] = &[
    "id",
    "id(int)",
    "score(float)",
    "flag(bool)",
    "name[required]",
    "v[type:int]",
    "w[min:1, max:9]",
    "e[enum:a|b]",
    "p[pattern:^[a-z]+$]",
    "b[block]",
    "\u{441}\u{43b}\u{43e}\u{432}\u{43e}",
];

fn generate(rng: &mut Rng) -> String {
    let mut doc = String::from("!synxl 1\n");
    let groups = 1 + rng.below(3);
    for _ in 0..groups {
        // At least one inline field: a field list that is all `block` is a hard
        // error by §5.3.4 and would only exercise the rejection path.
        let arity = 1 + rng.below(3);
        let mut decls: Vec<String> = (0..arity)
            .map(|i| format!("f{}{}", i, if rng.below(4) == 0 { "(int)" } else { "" }))
            .collect();
        for _ in 0..rng.below(3) {
            decls.push((*rng.pick(FIELD_DECLS)).to_string());
        }
        doc.push_str("!fields ");
        doc.push_str(&decls.join(" ; "));
        doc.push('\n');

        for _ in 0..1 + rng.below(4) {
            let parts = 1 + rng.below(4);
            let mut line: Vec<&str> = (0..parts).map(|_| *rng.pick(VALUES)).collect();
            // Position matters as much as content: a value with leading Unicode
            // whitespace only breaks the §14.1 round-trip when it is written
            // *first*, because that is what gives the re-read line a non-zero
            // indent (§3.3) and makes the record vanish. Leaving that to chance
            // would mostly generate documents where it lands mid-line and does
            // nothing, so every value gets its turn at the front.
            if rng.below(3) == 0 {
                line[0] = *rng.pick(VALUES);
            }
            doc.push_str(&line.join(" ; "));
            doc.push('\n');
            for _ in 0..rng.below(5) {
                doc.push_str(*rng.pick(BLOCK_LINES));
                doc.push('\n');
            }
        }

        if rng.below(4) == 0 {
            doc.push_str("# a comment\n");
        }
        if rng.below(6) == 0 {
            doc.push_str("###\nhidden\n###\n");
        }
    }
    doc
}

/// Every invariant that must hold for one document.
fn check(doc_text: &str) -> Result<(), String> {
    let Ok(doc) = synxl::parse_lines(doc_text) else {
        // §11.1 — a hard error is a valid outcome, but every reader has to
        // agree it is one.
        let streamed = synxl::SynxlStreamReader::new(std::io::Cursor::new(doc_text))
            .map(|r| r.into_iter().all(|item| item.is_ok()));
        if matches!(streamed, Ok(true)) {
            return Err("streaming reader accepted what the in-memory reader rejected".into());
        }
        return Ok(());
    };

    let json = doc.to_json();

    // §12.2 — same records in both projections.
    if doc.to_ndjson().lines().count() != doc.records.len() {
        return Err("NDJSON projection lost or gained a record".into());
    }

    // §15.1 — the streaming reader sees the same document.
    if let Ok(reader) = synxl::SynxlStreamReader::new(std::io::Cursor::new(doc_text)) {
        let mut streamed = Vec::new();
        let mut complete = true;
        for item in reader {
            match item {
                Ok(rec) => streamed.push(rec.value),
                Err(_) => {
                    complete = false;
                    break;
                }
            }
        }
        if complete && synxl::records_to_json_array(&streamed) != json {
            return Err("streaming and in-memory readers disagree on the projection".into());
        }
    }

    // §14.1 — round-trip, written per field-list group (§4.2).
    let mut written = String::from("!synxl 1\n");
    let mut start = 0usize;
    while start < doc.records.len() {
        let fl_idx = doc.record_field_lists.get(start).copied().unwrap_or(0);
        let mut end = start + 1;
        while end < doc.records.len()
            && doc.record_field_lists.get(end).copied().unwrap_or(0) == fl_idx
        {
            end += 1;
        }
        let Some(fl) = doc.field_lists.get(fl_idx) else {
            return Err(format!("record {start} names a field list that does not exist"));
        };
        match synxl::write_lines(fl.fields(), &doc.records[start..end]) {
            Ok(group) => written.push_str(group.strip_prefix("!synxl 1\n").unwrap_or(&group)),
            // §14.3 permits a loud refusal.
            Err(_) => return Ok(()),
        }
        start = end;
    }

    match synxl::parse_lines(&written) {
        Ok(reparsed) if reparsed.to_json() == json => Ok(()),
        Ok(reparsed) => Err(format!(
            "§14.1 round-trip changed the projection\n  written:  {written:?}\n  got:      {}\n  expected: {json}",
            reparsed.to_json()
        )),
        Err(err) => Err(format!(
            "§14.1 round-trip produced a document the parser rejects: {err}\n  written: {written:?}"
        )),
    }
}

#[test]
fn synxl_invariants_hold_over_generated_documents() {
    const SEEDS: [u64; 4] = [0x5117_1, 20260822, 1, u64::MAX / 3];
    const PER_SEED: usize = 2_000;

    let mut failures: Vec<String> = Vec::new();
    for seed in SEEDS {
        let mut rng = Rng(seed);
        for i in 0..PER_SEED {
            let doc = generate(&mut rng);
            if let Err(msg) = check(&doc) {
                failures.push(format!("seed {seed}, document {i}: {msg}\n  input: {doc:?}"));
                if failures.len() >= 5 {
                    break;
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "\n\n{} generated document(s) violated a SYNXL invariant:\n\n{}\n",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Inputs that previously panicked or corrupted data, kept as a regression set.
///
/// Each entry is the minimised form of an audit finding. They are cheap to run
/// and they fail loudly if the ordering rules they depend on are ever undone.
#[test]
fn known_hostile_inputs_are_handled() {
    let cases: &[(&str, &str)] = &[
        (
            "unicode indent inside a |+ body",
            "!synxl 1\n!fields id ; m[block]\n1\n  m |+\n    a\n   \u{a0}b\n",
        ),
        (
            "comment-shaped lines inside a body",
            "!synxl 1\n!fields id ; m[block]\n1\n  m |+\n      A\n      # hash\n      // slash\n      B\n",
        ),
        (
            "### inside a body must not swallow the next field",
            "!synxl 1\n!fields id ; a[block] ; b[block]\n1\n  a |+\n      x\n      ###\n      y\n  b 42\n",
        ),
        (
            "directive inside a body stays data",
            "!synxl 1\n!fields id ; m[block]\n1\n  m |+\n      one\n      !include secret\n      two\n",
        ),
        (
            "random hint inside a block",
            "!synxl 1\n!fields id ; b[block]\n1\n  b\n    n(random) x\n",
        ),
        (
            "leading unicode whitespace in an inline value",
            "!synxl 1\n!fields a ; b\n\"\u{a0}x\" ; 2\n",
        ),
        (
            "quoted value in a typed column",
            "!synxl 1\n!fields id[type:int] ; code[type:int]\n1 ; \"N/A\"\n",
        ),
        ("all-null form with a trailing NBSP", "!synxl 1\n!fields a ; b\n;\u{a0}\n"),
        ("indented line inside a block comment", "!synxl 1\n!fields a\n###\n  x\n###\n1\n"),
        ("block whose root is not an object", "!synxl 1\n!fields id\n1\n  -\n"),
        (
            "integers around the i64 edge",
            "!synxl 1\n!fields a ; b\n9223372036854775807 ; 9223372036854775808\n",
        ),
    ];

    let mut failures = Vec::new();
    for (label, input) in cases {
        if let Err(msg) = check(input) {
            failures.push(format!("{label}: {msg}"));
        }
    }
    assert!(failures.is_empty(), "\n\n{}\n", failures.join("\n\n"));
}

/// §16.3 — a hostile document must not cost time out of proportion to its size.
///
/// Deliberately loose: this catches an accidental quadratic (the audit found
/// two), not a constant-factor regression, so it does not turn into a flaky
/// timing test on a loaded machine.
#[test]
fn pathological_documents_stay_linear() {
    fn parse_ms(text: &str) -> u128 {
        let start = std::time::Instant::now();
        let _ = synxl::parse_lines(text);
        start.elapsed().as_millis()
    }

    // Many undeclared block keys: each one used to re-scan the whole block.
    let build_keys = |n: usize| {
        let mut doc = String::from("!synxl 1\n!fields id ; m[block]\n1\n");
        for i in 0..n {
            doc.push_str(&format!("  k{i} v\n"));
        }
        doc
    };
    let small = parse_ms(&build_keys(4_000));
    let large = parse_ms(&build_keys(16_000));
    // 4x the input; anything beyond ~16x the time is superlinear growth, not noise.
    assert!(
        large <= (small + 5) * 16,
        "block-key diagnostics look superlinear: 4k keys took {small} ms, 16k took {large} ms"
    );

    // One record line with very many parts.
    let semis = |n: usize| {
        let mut doc = String::from("!synxl 1\n!fields a\n");
        doc.push_str(&"x ; ".repeat(n));
        doc.push('\n');
        doc
    };
    let small = parse_ms(&semis(25_000));
    let large = parse_ms(&semis(100_000));
    assert!(
        large <= (small + 5) * 16,
        "the splitter looks superlinear: 25k parts took {small} ms, 100k took {large} ms"
    );
}
