//! SYNXL conformance runner (SYNXL §18).
//!
//! For each `tests/conformance-synxl/cases/NNN-name.synxl` the suite carries
//! **exactly one** of:
//!
//! * `.expected.json`  — the canonical array projection (§12.1), compared
//!   byte-for-byte (trailing newline stripped);
//! * `.expected.error` — a hard-error condition token on its first line (§11.1),
//!   with an informative explanation on the second.
//!
//! A case MAY additionally carry `.expected.diagnostics`, one `Kind index line`
//! triple per line, in the order §11.2 records them.
//!
//! The suite is derived from the normative text, not from this implementation:
//! where the two disagree, the fixture is the thing to argue with, and every
//! mismatch is listed at once rather than aborting on the first.

use std::fs;
use std::path::{Path, PathBuf};

use synx_core::synxl::{self, SynxlErrorKind};

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("conformance-synxl")
        .join("cases")
}

/// Condition tokens a `.expected.error` file may use for a given error kind.
///
/// The suite README fixes the spelling for the conditions it exercises; the
/// remaining aliases cover the plausible spellings for conditions added to
/// §11.1 after the suite was first written, so a naming difference is not
/// reported as a semantic failure.
fn error_tokens(kind: SynxlErrorKind) -> &'static [&'static str] {
    match kind {
        SynxlErrorKind::MissingPrologue => {
            &["MissingOrMalformedPrologue", "MissingPrologue", "MalformedPrologue"]
        }
        SynxlErrorKind::UnsupportedVersion => &[
            "UnsupportedFormatVersion",
            "UnsupportedVersion",
            "RepeatedPrologueVersionMismatch",
            "RepeatedPrologueDifferentVersion",
        ],
        SynxlErrorKind::UnknownDirective => &[
            "UnknownDirectiveLine",
            "UnknownDirective",
            "UnknownBangLine",
            "UnknownBangLineAtIndentZero",
            "UnknownExclamationLine",
        ],
        SynxlErrorKind::NoFieldList => &["RecordWithoutFieldList", "NoFieldList"],
        SynxlErrorKind::MalformedFieldList => &["MalformedFieldList"],
        SynxlErrorKind::DuplicateField => &["DuplicateFieldName", "DuplicateField"],
        SynxlErrorKind::MarkerChain => {
            &["MarkerChainInFieldDecl", "MarkerInFieldDecl", "MarkerChain"]
        }
        SynxlErrorKind::NonDeterministicHint => {
            &["NonDeterministicTypeHint", "NonDeterministicHint"]
        }
        SynxlErrorKind::BlockWithType => &["BlockCombinedWithType", "BlockWithType"],
        SynxlErrorKind::ZeroArity => {
            &["ZeroArityFieldList", "FieldListArityZero", "ZeroArity", "ArityZero"]
        }
        SynxlErrorKind::LimitExceeded => &["LimitExceeded"],
        // Writer-only (§14.3); no parse can produce it, so no fixture can
        // expect it.
        SynxlErrorKind::Unwritable => &["Unwritable"],
    }
}

/// Re-read a case through the `io::BufRead` reader (§15.1) and describe any
/// disagreement with the in-memory result.
///
/// The suite is the shared contract, so the three readers have to agree on it
/// record for record and diagnostic for diagnostic — that is cheaper to check
/// here than to discover from a bug report about one code path.
fn cross_check_streaming(name: &str, input: &str, doc: &synxl::SynxlDocument) -> Option<String> {
    let reader = match synxl::SynxlStreamReader::new(std::io::Cursor::new(input)) {
        Ok(r) => r,
        Err(err) => {
            return Some(format!(
                "{name}: the in-memory readers accepted this document but the streaming reader failed: {err}"
            ))
        }
    };

    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for item in reader {
        match item {
            Ok(mut rec) => {
                records.push(rec.value);
                diagnostics.append(&mut rec.diagnostics);
            }
            Err(err) => {
                return Some(format!(
                    "{name}: streaming reader failed mid-document where the in-memory readers did not: {err}"
                ))
            }
        }
    }

    let streamed_json = synxl::records_to_json_array(&records);
    if streamed_json != doc.to_json() {
        return Some(format!(
            "{name}: streaming reader disagrees on the projection\n       streamed: {streamed_json}\n       in-memory: {}",
            doc.to_json()
        ));
    }
    // Trailing orphan diagnostics are appended by `parse_lines`, so compare the
    // prefix the streaming loop can see.
    if diagnostics != doc.diagnostics[..diagnostics.len().min(doc.diagnostics.len())] {
        return Some(format!("{name}: streaming reader disagrees on diagnostics"));
    }
    None
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
}

/// Run one case, returning a failure description if it does not conform.
fn run_case(input_path: &Path) -> Result<(), String> {
    let name = input_path.file_stem().unwrap().to_string_lossy().to_string();
    let json_path = input_path.with_extension("expected.json");
    let error_path = input_path.with_extension("expected.error");
    let diag_path = input_path.with_extension("expected.diagnostics");

    let input = read(input_path);

    match (json_path.exists(), error_path.exists()) {
        (false, false) => {
            return Err(format!(
                "{name}: no .expected.json and no .expected.error — the case states no expectation"
            ))
        }
        (true, true) => {
            return Err(format!(
                "{name}: has BOTH .expected.json and .expected.error; the suite allows exactly one"
            ))
        }
        _ => {}
    }

    let parsed = synxl::parse_lines(&input);

    // ── Hard-error case (§11.1) ──────────────────────────────
    if error_path.exists() {
        let body = read(&error_path);
        let expected_token = body.lines().next().unwrap_or("").trim().to_string();
        return match parsed {
            Ok(doc) => Err(format!(
                "{name}: expected hard error `{expected_token}`, but the parse succeeded with {} record(s): {}",
                doc.len(),
                doc.to_json()
            )),
            Err(err) => {
                if error_tokens(err.kind).contains(&expected_token.as_str()) {
                    Ok(())
                } else {
                    Err(format!(
                        "{name}: expected hard error `{expected_token}`, got `{}` (line {}: {})",
                        err.kind, err.line, err.message
                    ))
                }
            }
        };
    }

    // ── Accepted case (§12.1) ────────────────────────────────
    let doc = match parsed {
        Ok(doc) => doc,
        Err(err) => {
            return Err(format!(
                "{name}: expected a successful parse, got hard error `{}` (line {}: {})",
                err.kind, err.line, err.message
            ))
        }
    };

    let expected_json = read(&json_path).trim().to_string();
    let got_json = doc.to_json();
    if got_json != expected_json {
        return Err(format!(
            "{name}: JSON projection mismatch\n       got:      {got_json}\n       expected: {expected_json}"
        ));
    }

    // ── Diagnostics (§11.2) ──────────────────────────────────
    let expected_diags: Vec<String> = if diag_path.exists() {
        read(&diag_path)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let got_diags: Vec<String> = doc
        .diagnostics
        .iter()
        .map(|d| format!("{} {} {}", d.kind, d.record_index, d.line))
        .collect();

    if let Some(msg) = cross_check_streaming(&name, &input, &doc) {
        return Err(msg);
    }

    if got_diags != expected_diags {
        let label = if diag_path.exists() {
            "diagnostics mismatch"
        } else {
            "produced diagnostics but the case declares none"
        };
        return Err(format!(
            "{name}: {label}\n       got:      [{}]\n       expected: [{}]",
            got_diags.join(" | "),
            expected_diags.join(" | ")
        ));
    }

    Ok(())
}

#[test]
fn conformance_synxl_suite() {
    let dir = cases_dir();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synxl"))
        .collect();
    entries.sort();

    assert!(!entries.is_empty(), "no .synxl cases found in {}", dir.display());

    let mut failures: Vec<String> = Vec::new();
    for path in &entries {
        if let Err(msg) = run_case(path) {
            failures.push(msg);
        }
    }

    eprintln!(
        "conformance-synxl: {}/{} cases passed",
        entries.len() - failures.len(),
        entries.len()
    );

    if !failures.is_empty() {
        panic!(
            "\n\n{} of {} SYNXL conformance case(s) failed:\n  - {}\n",
            failures.len(),
            entries.len(),
            failures.join("\n  - ")
        );
    }
}
