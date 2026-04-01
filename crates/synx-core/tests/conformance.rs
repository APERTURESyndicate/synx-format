//! Conformance test runner.
//!
//! For each `tests/conformance/cases/NNN-name.synx` + `.expected.json` pair,
//! parse the SYNX input and compare the JSON output byte-for-byte with the
//! expected file.  Keys are sorted deterministically by `synx_core::to_json`.
//!
//! Tool-mode cases (files starting with `!tool`) are routed through
//! `Synx::parse_tool` instead of `Synx::parse`.

use std::fs;
use std::path::Path;

fn cases_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("conformance")
        .join("cases")
        .leak()
}

fn run_case(synx_path: &Path) {
    let expected_path = synx_path.with_extension("expected.json");
    if !expected_path.exists() {
        return;
    }

    let input = fs::read_to_string(synx_path)
        .unwrap_or_else(|e| panic!("{}: {}", synx_path.display(), e));
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("{}: {}", expected_path.display(), e))
        .trim()
        .to_string();

    let is_tool = input.trim_start().starts_with("!tool");

    let json = if is_tool {
        let opts = synx_core::Options::default();
        let map = synx_core::Synx::parse_tool(&input, &opts);
        synx_core::to_json(&synx_core::Value::Object(map))
    } else {
        let result = synx_core::parse(&input);
        synx_core::to_json(&result.root)
    };

    assert_eq!(
        json, expected,
        "\n\nConformance FAIL: {}\n  got:      {}\n  expected: {}\n",
        synx_path.file_name().unwrap().to_string_lossy(),
        json,
        expected,
    );
}

#[test]
fn conformance_suite() {
    let dir = cases_dir();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "synx"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    assert!(!entries.is_empty(), "no .synx cases found in {}", dir.display());

    let mut passed = 0;
    for entry in &entries {
        run_case(&entry.path());
        passed += 1;
    }
    eprintln!("conformance: {}/{} cases passed", passed, entries.len());
}
