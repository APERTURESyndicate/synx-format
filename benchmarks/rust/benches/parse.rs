use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use synx_core::{Synx, parse, to_json};

// ── Config text constants ─────────────────────────────────────────────────────

const SYNX_SMALL: &str = "name TotalWario\nversion 3.0.0\nport 8080\ndebug false";

const SYNX_CONFIG: &str = include_str!("../../config.synx");

const JSON_CONFIG: &str = include_str!("../../config.json");

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("synx_parse");

    // Small payload — 4 keys
    group.bench_function("small/4-keys", |b| {
        b.iter(|| {
            let r = parse(black_box(SYNX_SMALL));
            black_box(r)
        })
    });

    // Full config (~110 keys)
    group.bench_function("full/110-keys", |b| {
        b.iter(|| {
            let r = parse(black_box(SYNX_CONFIG));
            black_box(r)
        })
    });

    group.finish();
}

fn bench_synx_api(c: &mut Criterion) {
    let mut group = c.benchmark_group("synx_api");

    // Synx::parse (HashMap output)
    group.bench_function("Synx::parse", |b| {
        b.iter(|| {
            let map = Synx::parse(black_box(SYNX_CONFIG));
            black_box(map)
        })
    });

    // parse + to_json (serialize back to JSON string)
    group.bench_function("parse_to_json", |b| {
        b.iter(|| {
            let r = parse(black_box(SYNX_CONFIG));
            let s = to_json(black_box(&r.root));
            black_box(s)
        })
    });

    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(criterion::Throughput::Bytes(SYNX_CONFIG.len() as u64));

    group.bench_function("SYNX_parse_bytes/s", |b| {
        b.iter(|| {
            let r = parse(black_box(SYNX_CONFIG));
            black_box(r)
        })
    });

    group.finish();
}

fn bench_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("synx_binary");

    // Compile text → binary
    group.bench_function("compile/110-keys", |b| {
        b.iter(|| {
            let bin = Synx::compile(black_box(SYNX_CONFIG), false);
            black_box(bin)
        })
    });

    // Decompile binary → text
    let binary = Synx::compile(SYNX_CONFIG, false);
    group.bench_function("decompile/110-keys", |b| {
        b.iter(|| {
            let txt = Synx::decompile(black_box(&binary)).unwrap();
            black_box(txt)
        })
    });

    // Size comparison (not timed, just report)
    group.bench_function("size_text_bytes", |b| {
        b.iter(|| black_box(SYNX_CONFIG.len()))
    });
    group.bench_function("size_binary_bytes", |b| {
        b.iter(|| black_box(binary.len()))
    });

    group.finish();
}

criterion_group!(benches, bench_parse, bench_synx_api, bench_throughput, bench_binary);
criterion_main!(benches);
