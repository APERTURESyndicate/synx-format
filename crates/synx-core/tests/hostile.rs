//! Inputs built to cost far more than their size. Each one has to end — with
//! a value or an error, never a panic — inside a small stack and a stated
//! amount of memory. The limits are the contract: a change that makes one of
//! these documents twice as expensive fails here.
//!
//! Memory is what the allocator handed out while the call ran (its peak above
//! what was live before), so the input itself is not counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use synx_core::{parse, resolve, to_json, Options, ParseResult, Synx, Value};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            let grown = new_size - layout.size();
            let now = LIVE.fetch_add(grown, Ordering::Relaxed) + grown;
            PEAK.fetch_max(now, Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The counters are the process's, so one test runs at a time — building
/// its input included, or one test's megabytes land in another's peak.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

const MIB: usize = 1024 * 1024;

/// The stack a host's worker thread may well have.
const SMALL_STACK: usize = 512 * 1024;

struct Cost<T> {
    out: T,
    peak_mib: usize,
    took: Duration,
}

/// Run `work` on a thread with a small stack and report what it cost.
fn measure<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Cost<T> {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let started = Instant::now();
    let out = std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(work)
        .expect("thread")
        .join()
        .expect("the call panicked or overflowed its stack");
    let took = started.elapsed();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    Cost { out, peak_mib: peak.div_ceil(MIB), took }
}

/// A debug build is an order of magnitude slower; the bound is on the shape
/// of the cost, not on the machine.
fn seconds(release: u64) -> Duration {
    Duration::from_secs(if cfg!(debug_assertions) { release * 20 } else { release })
}

fn active(text: String) -> Cost<ParseResult> {
    measure(move || {
        let mut r = parse(&text);
        resolve(&mut r, &Options { env: Some(HashMap::new()), ..Default::default() });
        r
    })
}

fn active_in(text: String, base: std::path::PathBuf) -> Cost<ParseResult> {
    measure(move || {
        let mut r = parse(&text);
        resolve(
            &mut r,
            &Options { env: Some(HashMap::new()), base_path: Some(base.to_string_lossy().into_owned()), ..Default::default() },
        );
        r
    })
}

fn at<'a>(r: &'a ParseResult, key: &str) -> &'a Value {
    &r.root.as_object().expect("an object")[key]
}

fn is_text(v: &Value, text: &str) -> bool {
    matches!(v, Value::String(s) if s == text)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("synx-hostile-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const LIMIT: &str = "LIMIT_ERR: resolution budget exceeded";

// ─── Expressions ────────────────────────────────────────────────────────────

#[test]
fn calc_with_a_hundred_thousand_parentheses() {
    let _alone = alone();
    let n = 100_000;
    let expr = format!("{}1{}", "(".repeat(n), ")".repeat(n));
    let cost = measure(move || synx_core::safe_calc(&expr));
    assert!(cost.out.is_ok() || cost.out.is_err());
    assert!(cost.peak_mib <= 32, "{} MiB", cost.peak_mib);

    let n = 2000;
    let cost = active(format!("!active\nbase 2\nv:ref:calc:{}+1{} base\n", "(".repeat(n), ")".repeat(n)));
    assert!(matches!(at(&cost.out, "v"), Value::Int(_) | Value::Float(_) | Value::String(_)));
}

// ─── Markers that multiply ──────────────────────────────────────────────────

#[test]
fn aliases_that_double_stop_at_the_budget() {
    let _alone = alone();
    let mut doc = String::from("!active\na0\n  x 1\n");
    for i in 1..=30 {
        let _ = write!(doc, "a{}\n  l:alias a{}\n  r:ref a{}\n", i, i - 1, i - 1);
    }
    let cost = active(doc);
    assert!(is_text(&at(&cost.out, "a30").as_object().unwrap()["l"], LIMIT));
    assert!(cost.peak_mib <= 160, "{} MiB", cost.peak_mib);
    assert!(cost.took < seconds(3), "{:?}", cost.took);
}

#[test]
fn inheritance_stops_at_the_budget() {
    let _alone = alone();
    // 2 000 children of a 2 000-field parent: four million fields from 80 KB.
    let n = 2000;
    let mut doc = String::from("!active\np\n");
    for i in 0..n {
        let _ = writeln!(doc, "  f{} {}", i, i);
    }
    for i in 0..n {
        let _ = write!(doc, "c{}:inherit:p\n  own {}\n", i, i);
    }
    let cost = active(doc);
    assert_eq!(at(&cost.out, "c0").as_object().unwrap().len(), n + 1);
    assert!(is_text(at(&cost.out, &format!("c{}", n - 1)), LIMIT));
    assert!(cost.peak_mib <= 160, "{} MiB", cost.peak_mib);
    assert!(cost.took < seconds(3), "{:?}", cost.took);

    // The same with a marker on every field of the parent: the declarations
    // are copied with the fields, and are charged with them.
    let n = 1200;
    let mut doc = String::from("!active\np\n");
    for i in 0..n {
        let _ = writeln!(doc, "  f{}:env:default:1 V", i);
    }
    for i in 0..n {
        let _ = write!(doc, "c{}:inherit:p\n  own {}\n", i, i);
    }
    let cost = active(doc);
    assert!(is_text(at(&cost.out, &format!("c{}", n - 1)), LIMIT));
    assert!(cost.peak_mib <= 160, "{} MiB", cost.peak_mib);
    assert!(cost.took < seconds(3), "{:?}", cost.took);
}

#[test]
fn text_built_by_markers_stops_at_the_budget() {
    let _alone = alone();
    // One 30 KB value named from 3 000 keys.
    let mut doc = format!("!active\nbig {}\n", "v".repeat(30_000));
    for i in 0..3000 {
        let _ = writeln!(doc, "k{} {{big}}", i);
    }
    let cost = active(doc);
    assert_eq!(at(&cost.out, "k0").as_str().map(str::len), Some(30_000));
    assert!(is_text(at(&cost.out, "k2999"), LIMIT));
    assert!(cost.peak_mib <= 120, "{} MiB", cost.peak_mib);

    // Each line twice the line before, 28 times.
    let mut doc = String::from("!active\na0 0123456789\n");
    for i in 0..28 {
        let _ = writeln!(doc, "a{} {{a{}}}{{a{}}}", i + 1, i, i);
    }
    let cost = active(doc);
    assert!(cost.peak_mib <= 120, "{} MiB", cost.peak_mib);
    assert!(cost.took < seconds(2), "{:?}", cost.took);

    // `:replace` and `:join`: the size is known before the text is built.
    let cost = active(format!("!active\nk:replace:a:{} {}\n", "B".repeat(40_000), "a".repeat(40_000)));
    assert!(is_text(at(&cost.out, "k"), LIMIT));
    assert!(cost.peak_mib <= 8, "{} MiB", cost.peak_mib);

    let mut doc = format!("!active\nk:join:{}\n", "S".repeat(40_000));
    for i in 0..40_000 {
        let _ = writeln!(doc, "  - {}", i);
    }
    let cost = active(doc);
    assert!(is_text(at(&cost.out, "k"), LIMIT));
    assert!(cost.peak_mib <= 32, "{} MiB", cost.peak_mib);

    // What fits the budget is built once, at its size.
    let cost = active(format!("!active\nk:replace:a:{} {}\n", "B".repeat(4000), "a".repeat(4000)));
    assert_eq!(at(&cost.out, "k").as_str().map(str::len), Some(16_000_000));
    assert!(cost.peak_mib <= 20, "{} MiB", cost.peak_mib);
}

#[test]
fn a_megabyte_of_braces_is_text() {
    let _alone = alone();
    for filler in ["{", "{}", "{a", "{a:"] {
        let cost = active(format!("!active\nv {}\n", filler.repeat(MIB / filler.len())));
        assert!(cost.took < seconds(1), "{:?} for {:?}", cost.took, filler);
        assert!(cost.peak_mib <= 16, "{} MiB", cost.peak_mib);
    }
}

#[test]
fn calc_over_many_numbers_is_linear() {
    let _alone = alone();
    let n = 8000;
    let mut doc = String::from("!active\nmax-hp 5\n");
    for i in 0..n {
        let _ = writeln!(doc, "v{} {}", i, i);
    }
    for i in 0..n {
        let _ = writeln!(doc, "c{}:calc v{} + v{} * 2", i, i, (i * 7) % n);
    }
    let cost = active(doc);
    assert_eq!(at(&cost.out, "c10"), &Value::Int(10 + 70 * 2));
    assert!(cost.took < seconds(2), "{:?}", cost.took);
}

// ─── Parsing ────────────────────────────────────────────────────────────────

#[test]
fn declarations_under_a_long_key_have_a_budget() {
    let _alone = alone();
    let key = "K".repeat(MIB);
    let mut doc = format!("!active\n{}\n", key);
    for i in 0..2000 {
        let _ = write!(doc, "  o{}\n    m:env X\n", i);
    }
    let cost = measure(move || parse(&doc));
    assert!(cost.out.truncated);
    assert!(cost.peak_mib <= 160, "{} MiB", cost.peak_mib);
}

#[test]
fn a_long_ancestor_key_is_not_hashed_per_line() {
    let _alone = alone();
    let n = 60_000;
    let mut doc = format!("{}\n", "k".repeat(12 * n));
    for _ in 0..n {
        doc.push_str("  a\n    x 1\n");
    }
    let cost = measure(move || parse(&doc));
    assert_eq!(cost.out.root.as_object().unwrap().len(), 1);
    assert!(cost.took < seconds(1), "{:?}", cost.took);
}

#[test]
fn a_group_declared_again_and_again_is_linear() {
    let _alone = alone();
    let mut doc = String::from("!active\n");
    for i in 0..3000 {
        let _ = write!(doc, "g{}\n  x[min:1] 1\n", i);
    }
    for _ in 0..12_000 {
        doc.push_str("a\n  y 1\n");
    }
    let cost = measure(move || parse(&doc));
    assert_eq!(cost.out.metadata.len(), 3000);
    assert!(cost.took < seconds(1), "{:?}", cost.took);
}

#[test]
fn three_thousand_levels_of_indentation() {
    let _alone = alone();
    let mut doc = String::new();
    for i in 0..3000 {
        let _ = writeln!(doc, "{}k{}", " ".repeat(i), i);
    }
    let text = doc.clone();
    let cost = measure(move || {
        let r = parse(&text);
        (to_json(&r.root).len(), Synx::stringify(&r.root).len(), Synx::format(&text).len(), Synx::compile(&text, false).len())
    });
    assert!(cost.out.0 > 0);
    assert!(cost.took < seconds(2), "{:?}", cost.took);
}

// ─── Files ──────────────────────────────────────────────────────────────────

#[test]
fn a_file_that_includes_itself_twice_ends() {
    let _alone = alone();
    let d = scratch("self-include");
    std::fs::write(d.join("self.synx"), "!active\na:include self.synx\nb:include self.synx\nc:include self.synx\n").unwrap();
    let cost = active_in("!active\na:include self.synx\nb:include self.synx\n".to_string(), d.clone());
    let a = at(&cost.out, "a").as_object().expect("the file, once");
    assert!(is_text(&a["a"], "INCLUDE_ERR: circular include: 'self.synx'"));
    assert!(cost.took < seconds(1), "{:?}", cost.took);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn packages_that_use_each_other_end() {
    let _alone = alone();
    let d = scratch("use-cycle");
    for (a, b) in [("a", "b"), ("b", "c"), ("c", "a")] {
        let p = d.join(format!("synx_packages/@x/{}/src", a));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("main.synx"), format!("!active\n!use @x/{}\nv {}\n", b, a)).unwrap();
    }
    let base = d.clone();
    let cost = measure(move || {
        let mut r = parse("!active\n!use @x/a\nk 1\n");
        resolve(
            &mut r,
            &Options {
                env: Some(HashMap::new()),
                base_path: Some(base.to_string_lossy().into_owned()),
                packages_path: Some(base.join("synx_packages").to_string_lossy().into_owned()),
                ..Default::default()
            },
        );
        r
    });
    assert!(at(&cost.out, "a").as_object().is_some());
    assert!(cost.took < seconds(2), "{:?}", cost.took);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn once_for_three_thousand_keys_writes_three_thousand_lines() {
    let _alone = alone();
    let d = scratch("once");
    let mut doc = String::from("!active\n");
    for i in 0..3000 {
        let _ = writeln!(doc, "id{}:once", i);
    }
    let cost = active_in(doc, d.clone());
    assert!(matches!(at(&cost.out, "id2999"), Value::String(s) if s.len() == 36));
    assert_eq!(std::fs::read_to_string(d.join(".synx.lock")).unwrap().lines().count(), 3000);
    assert!(cost.took < seconds(2), "{:?}", cost.took);
    let _ = std::fs::remove_dir_all(&d);
}

// ─── `.synxb` ───────────────────────────────────────────────────────────────

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn container(payload: &[u8], flags: u8) -> Vec<u8> {
    let mut out = b"SYNXB\x01".to_vec();
    out.push(flags);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(payload, 1));
    out
}

fn decode(file: Vec<u8>) -> Cost<Result<usize, String>> {
    measure(move || synx_core::binary::decompile(&file).map(|r| r.metadata.len()))
}

#[test]
fn a_header_that_declares_256_mib_allocates_none_of_it() {
    let _alone = alone();
    let mut file = b"SYNXB\x01\x00".to_vec();
    file.extend_from_slice(&(256u32 * 1024 * 1024).to_le_bytes());
    file.extend_from_slice(&[0x03, 0x00]);
    assert_eq!(file.len(), 13);
    let cost = decode(file);
    assert!(cost.out.is_err());
    assert!(cost.peak_mib <= 2, "{} MiB", cost.peak_mib);
}

#[test]
fn counts_in_a_container_are_paid_for_before_they_are_reserved() {
    let _alone = alone();
    const ARRAY: u8 = 0x06;
    const OBJECT: u8 = 0x07;
    const STRING: u8 = 0x05;
    let n = 8 * MIB;

    // Eight million nulls behind a few kilobytes of deflate.
    let mut p = Vec::new();
    varint(&mut p, 0);
    p.push(ARRAY);
    varint(&mut p, n as u64);
    p.resize(p.len() + n, 0);
    let cost = decode(container(&p, 0));
    assert!(cost.out.unwrap_err().contains("refusing to decode"));
    assert!(cost.peak_mib <= 40, "{} MiB", cost.peak_mib);

    // An object that claims eight million entries and has room for half.
    let mut p = Vec::new();
    varint(&mut p, 0);
    p.push(OBJECT);
    varint(&mut p, n as u64);
    p.resize(p.len() + n, 0);
    let cost = decode(container(&p, 0));
    assert!(cost.out.is_err());
    assert!(cost.peak_mib <= 40, "{} MiB", cost.peak_mib);

    // A hundred lists inside each other, each claiming two million items of
    // the four megabytes that follow: the first is within the budget and is
    // reserved, the second is not. The budget is the most a count can cost.
    let mut p = Vec::new();
    varint(&mut p, 0);
    for _ in 0..100 {
        p.push(ARRAY);
        varint(&mut p, 2 * MIB as u64);
    }
    p.resize(p.len() + 4 * MIB, 0);
    let cost = decode(container(&p, 0));
    assert!(cost.out.unwrap_err().contains("refusing to decode"));
    assert!(cost.peak_mib <= 200, "{} MiB", cost.peak_mib);

    // One 120 KB string, six thousand references to it: 720 MiB of copies.
    let mut p = Vec::new();
    varint(&mut p, 1);
    varint(&mut p, 120_000);
    p.resize(p.len() + 120_000, b'S');
    p.push(ARRAY);
    varint(&mut p, 6000);
    for _ in 0..6000 {
        p.extend_from_slice(&[STRING, 0]);
    }
    let cost = decode(container(&p, 0));
    assert!(cost.out.unwrap_err().contains("refusing to decode"));
    assert!(cost.peak_mib <= 200, "{} MiB", cost.peak_mib);

    // A table of sixteen million empty strings.
    let mut p = Vec::new();
    varint(&mut p, 16 * MIB as u64);
    p.resize(p.len() + 16 * MIB, 0);
    p.push(0x00);
    let cost = decode(container(&p, 0));
    assert!(cost.out.unwrap_err().contains("refusing to decode"));
    assert!(cost.peak_mib <= 64, "{} MiB", cost.peak_mib);

    // Declarations: four million objects with none.
    let mut p = Vec::new();
    varint(&mut p, 1);
    varint(&mut p, 1);
    p.push(b'k');
    p.push(0x00);
    varint(&mut p, 4_000_000);
    for _ in 0..4_000_000 {
        p.extend_from_slice(&[0, 0]);
    }
    varint(&mut p, 0);
    let cost = decode(container(&p, 0x05));
    assert!(cost.out.is_err());
    assert!(cost.peak_mib <= 40, "{} MiB", cost.peak_mib);
}

#[test]
fn a_container_written_by_compile_decodes_at_its_size() {
    let _alone = alone();
    // A million list items: what it holds is what it costs, with no slack
    // from a list grown by doubling.
    let mut doc = String::with_capacity(8 * MIB);
    doc.push_str("l\n");
    for i in 0..1_000_000 {
        let _ = writeln!(doc, " - {}", i % 10);
    }
    let file = Synx::compile(&doc, false);
    drop(doc);
    let cost = measure(move || synx_core::binary::decompile(&file).map(|r| r.root.as_object().unwrap()["l"].as_array().unwrap().len()));
    assert_eq!(cost.out, Ok(1_000_000));
    assert!(cost.peak_mib <= 80, "{} MiB", cost.peak_mib);
}

// ─── SYNXL ──────────────────────────────────────────────────────────────────

#[test]
fn many_wide_field_lists_are_linear() {
    let _alone = alone();
    let fields: Vec<String> = (0..4096).map(|i| format!("f{}", i)).collect();
    let header = format!("!fields {}\n", fields.join(" ; "));
    let row = format!("{}\n", vec!["1"; 4096].join(" ; "));
    let mut doc = String::from("!synxl 1\n");
    for _ in 0..100 {
        doc.push_str(&header);
        doc.push_str(&row);
    }
    let cost = measure(move || synx_core::parse_lines(&doc).map(|d| d.records.len()));
    assert_eq!(cost.out.ok(), Some(100));
    assert!(cost.took < seconds(2), "{:?}", cost.took);
}

// ─── Lists ──────────────────────────────────────────────────────────────────

#[test]
fn sorting_numbers_and_text_together_does_not_panic() {
    let _alone = alone();
    let mut doc = String::from("!active\nl:sort\n");
    for i in 0..5000u32 {
        match i % 3 {
            0 => {
                let _ = writeln!(doc, "  - {}", i.wrapping_mul(2654435761) % 100);
            }
            1 => {
                let _ = writeln!(doc, "  - {}a", i.wrapping_mul(40503) % 100);
            }
            _ => {
                let _ = writeln!(doc, "  - {}.5", i % 50);
            }
        }
    }
    let cost = active(doc);
    assert_eq!(at(&cost.out, "l").as_array().unwrap().len(), 5000);
}
