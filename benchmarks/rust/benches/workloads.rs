//! Workloads for the core: the paths a host runs per request or per start.
//!
//! ```bash
//! cargo bench -p synx-bench --bench workloads                       # everything
//! cargo bench -p synx-bench --bench workloads -- parse/             # one group
//! SYNX_BENCH_CORPUS=/path/to/configs cargo bench -p synx-bench --bench workloads -- corpus/
//! ```
//!
//! Inputs are built here, so two checkouts measure the same bytes:
//!
//! * `config`  — `benchmarks/config.synx`, the 2.5 KB sample every port is timed on;
//! * `service` — an `!active` service config: environment, defaults, secrets, constraints;
//! * `balance` — a game balance sheet of about 1 MB: comments, groups, lists of records;
//! * `flat`    — 20 000 `key value` lines;
//! * `worst/*` — small documents that used to cost far more than their size.
//!
//! `SYNX_BENCH_CORPUS` adds a directory of real `.synx` files (walked
//! recursively): every file parsed, resolved and printed as JSON.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use synx_core::{parse, resolve, to_json, Options, Synx};

const CONFIG: &str = include_str!("../../config.synx");

/// An `!active` config of the shape services keep in `config/app.synx`.
const SERVICE: &str = "\
!active

# Service identity
service_name example-api
version 1.4.2
port[type:int, min:1, max:65535]:env:default:7001 PORT
grpc_port[type:int]:env:default:48001 GRPC_PORT
log_level[enum:debug|info|warn|error]:env:default:info LOG_LEVEL
public_url:env:default:http://localhost:7001 PUBLIC_URL

database
  url[required]:secret:env DATABASE_URL
  pool_min[type:int, min:1]:env:default:2 DB_POOL_MIN
  pool_max[type:int, min:1, max:200]:env:default:20 DB_POOL_MAX
  statement_timeout_ms 15000
  ssl:env:default:false DB_SSL

redis
  url[required]:env REDIS_URL
  key_prefix example:
  ttl_sec 3600

nats
  url:env:default:nats://localhost:4222 NATS_URL
  user:env NATS_USER
  password:secret:env NATS_PASSWORD
  streams
    - user-events
    - billing-events
    - audit-log

jwt
  public_key[required]:secret:env JWT_PUBLIC_KEY
  issuer example-auth
  access_ttl_sec 900
  refresh_ttl_sec 2592000
  leeway_sec:calc access_ttl_sec / 30

limits
  body_bytes 1048576
  rate_per_min 600
  burst:calc rate_per_min / 10
  uploads
    max_bytes 26214400
    types
      - image/png
      - image/jpeg
      - image/webp

mail
  host:env:default:smtp.example.test SMTP_HOST
  port[type:int]:env:default:587 SMTP_PORT
  user:env SMTP_USER
  pass:secret:env SMTP_PASS
  from Example <no-reply@example.test>

features
  signup true
  invites false
  beta_users
    - alice
    - bob
banner Welcome to {service_name} {version}
health_url {public_url}/healthz
";

fn service_env() -> HashMap<String, String> {
    [
        ("PORT", "7101"),
        ("DATABASE_URL", "postgres://app:pw@10.0.0.3:5432/app"),
        ("REDIS_URL", "redis://10.0.0.3:6379/0"),
        ("NATS_USER", "app"),
        ("NATS_PASSWORD", "nats-password"),
        ("JWT_PUBLIC_KEY", "-----BEGIN PUBLIC KEY-----MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE-----END PUBLIC KEY-----"),
        ("SMTP_USER", "mailer"),
        ("SMTP_PASS", "smtp-password"),
        ("LOG_LEVEL", "warn"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// A balance sheet: a commented header, tuning groups, lists of records.
fn balance(records: usize) -> String {
    let mut s = String::with_capacity(records * 420);
    for i in 0..40 {
        let _ = writeln!(s, "# note {}: values below are tuned together with the difficulty curve", i);
    }
    s.push_str("meta\n  schema 7\n  title Balance sheet\n  curves\n    damage_step 0.08\n    health_step 0.11\n    price_step 1.35\n\n");
    s.push_str("weapons\n");
    for i in 0..records {
        let _ = write!(
            s,
            "  - id weapon_{i:05}\n    name Weapon number {i}\n    rarity {rarity}\n    side {side}\n    damage {dmg}.{frac}\n    cooldown_sec 0.{cd:02}\n    crit_chance 0.{crit:02}\n    price {price}\n    two_handed {two}\n    alt\n      kind wave\n      windup_sec 0.35\n      damage_mult 1.{mult}\n      arc_deg {arc}\n    desc A long description of weapon {i} that explains what it does in play\n",
            i = i,
            rarity = ["common", "rare", "super_rare", "legendary", "mythic"][i % 5],
            side = ["saint", "demon", "none"][i % 3],
            dmg = 5 + i % 90,
            frac = i % 10,
            cd = 20 + i % 70,
            crit = 5 + i % 30,
            price = 100 + (i * 37) % 9000,
            two = i % 4 == 0,
            mult = 2 + i % 7,
            arc = 60 + i % 120,
        );
    }
    s.push_str("\ndrop_tables\n");
    for i in 0..records / 4 {
        let _ = write!(s, "  table_{i:04}\n    weight {w}\n    rolls {r}\n    items\n      - weapon_{a:05}\n      - weapon_{b:05}\n      - weapon_{c:05}\n", i = i, w = 1 + i % 50, r = 1 + i % 3, a = i, b = i * 2 % records.max(1), c = i * 3 % records.max(1));
    }
    s
}

fn flat(keys: usize) -> String {
    let mut s = String::with_capacity(keys * 24);
    for i in 0..keys {
        let _ = writeln!(s, "setting_{:05} value number {}", i, i);
    }
    s
}

/// SYNXL: one record per line, the shape of an event log.
fn log_lines(records: usize) -> String {
    let mut s = String::from("!synxl 1\n!fields id[type:int] ; ts ; level ; service ; message\n");
    for i in 0..records {
        let _ = writeln!(s, "{} ; 2026-11-12T10:{:02}:{:02}Z ; {} ; api ; request {} served in {} ms", i, i / 60 % 60, i % 60, ["info", "warn", "error"][i % 3], i, i % 250);
    }
    s
}

fn active(text: &str, env: &HashMap<String, String>) -> synx_core::ParseResult {
    let mut r = parse(text);
    let opts = Options { env: Some(env.clone()), ..Default::default() };
    resolve(&mut r, &opts);
    r
}

fn bench_parse(c: &mut Criterion) {
    let balance = balance(3000);
    let flat = flat(20_000);
    let mut g = c.benchmark_group("parse");
    for (name, text) in [("config", CONFIG), ("service", SERVICE), ("balance", balance.as_str()), ("flat", flat.as_str())] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_function(name, |b| b.iter(|| black_box(parse(black_box(text)))));
    }
    g.finish();
}

fn bench_active(c: &mut Criterion) {
    let env = service_env();
    let empty = HashMap::new();
    let mut g = c.benchmark_group("active");
    g.throughput(Throughput::Bytes(SERVICE.len() as u64));
    g.bench_function("service", |b| b.iter(|| black_box(active(black_box(SERVICE), &env))));
    g.bench_function("service/unset-env", |b| b.iter(|| black_box(active(black_box(SERVICE), &empty))));
    // The resolve step alone: parsing is the setup and is not timed.
    let opts = Options { env: Some(env.clone()), ..Default::default() };
    g.bench_function("service/resolve-only", |b| {
        b.iter_batched(
            || parse(SERVICE),
            |mut r| {
                resolve(&mut r, &opts);
                black_box(r)
            },
            BatchSize::SmallInput,
        )
    });
    g.finish();
}

fn bench_output(c: &mut Criterion) {
    let balance = balance(3000);
    let inputs = [("config", parse(CONFIG)), ("service", active(SERVICE, &service_env())), ("balance", parse(&balance))];
    let mut g = c.benchmark_group("to_json");
    for (name, r) in &inputs {
        g.bench_function(*name, |b| b.iter(|| black_box(to_json(black_box(&r.root)))));
    }
    g.finish();
    let mut g = c.benchmark_group("stringify");
    for (name, r) in &inputs {
        g.bench_function(*name, |b| b.iter(|| black_box(Synx::stringify(black_box(&r.root)))));
    }
    g.finish();
    let mut g = c.benchmark_group("format");
    for (name, text) in [("config", CONFIG), ("service", SERVICE), ("balance", balance.as_str())] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_function(name, |b| b.iter(|| black_box(Synx::format(black_box(text)))));
    }
    g.finish();
}

fn bench_binary(c: &mut Criterion) {
    let balance = balance(3000);
    let mut g = c.benchmark_group("synxb");
    for (name, text) in [("config", CONFIG), ("service", SERVICE), ("balance", balance.as_str())] {
        let parsed = parse(text);
        let file = synx_core::binary::compile(&parsed, false);
        g.bench_function(format!("compile/{}", name), |b| b.iter(|| black_box(synx_core::binary::compile(black_box(&parsed), false))));
        g.bench_function(format!("decode/{}", name), |b| b.iter(|| black_box(synx_core::binary::decompile(black_box(&file)).unwrap())));
        g.bench_function(format!("decompile-to-text/{}", name), |b| b.iter(|| black_box(Synx::decompile(black_box(&file)).unwrap())));
    }
    g.finish();
}

fn bench_synxl(c: &mut Criterion) {
    let text = log_lines(20_000);
    let doc = synx_core::parse_lines(&text).unwrap();
    let mut g = c.benchmark_group("synxl");
    g.throughput(Throughput::Bytes(text.len() as u64));
    g.bench_function("read/20k-records", |b| b.iter(|| black_box(synx_core::parse_lines(black_box(&text)).unwrap())));
    g.bench_function("write/20k-records", |b| b.iter(|| black_box(doc.to_synxl().unwrap())));
    g.bench_function("to_json/20k-records", |b| b.iter(|| black_box(doc.to_json())));
    g.finish();
}

/// Small inputs whose cost is the point: each has to stay in step with its size.
fn bench_worst(c: &mut Criterion) {
    let empty = HashMap::new();
    let mut cases: Vec<(&str, String, bool)> = Vec::new();

    // 128 nested groups, each with a key.
    let mut deep = String::new();
    for i in 0..128 {
        let _ = writeln!(deep, "{}g{}\n{}  leaf {}", " ".repeat(i * 2), i, " ".repeat(i * 2), i);
    }
    cases.push(("deep-128", deep, false));

    // One 64 KB key, 2 000 groups under it.
    let mut long_key = format!("{}\n", "k".repeat(64 * 1024));
    for i in 0..2000 {
        let _ = writeln!(long_key, "  g{}\n    x {}", i, i);
    }
    cases.push(("long-ancestor-key", long_key, false));

    // 64 KiB of braces in one value.
    cases.push(("braces-64KiB", format!("!active\nv {}\n", "{".repeat(64 * 1024)), true));

    // 4 000 expressions over 4 000 numbers, one of them under a dashed key.
    let mut calc = String::from("!active\nmax-hp 5\n");
    for i in 0..4000 {
        let _ = writeln!(calc, "v{} {}", i, i);
    }
    for i in 0..4000 {
        let _ = writeln!(calc, "c{}:calc v{} + v{} * 2", i, i, (i * 7) % 4000);
    }
    cases.push(("calc-4000", calc, true));

    // 300 children of a 300-field parent.
    let mut inherit = String::from("!active\nbase\n");
    for i in 0..300 {
        let _ = writeln!(inherit, "  f{} {}", i, i);
    }
    for i in 0..300 {
        let _ = writeln!(inherit, "c{}:inherit:base\n  own {}", i, i);
    }
    cases.push(("inherit-300x300", inherit, true));

    // A group redeclared 2 000 times in a document with 2 000 declarations.
    let mut redeclare = String::from("!active\n");
    for i in 0..2000 {
        let _ = writeln!(redeclare, "g{}\n  x[min:1] 1", i);
    }
    for _ in 0..2000 {
        redeclare.push_str("a\n  y 1\n");
    }
    cases.push(("redeclare-2000", redeclare, true));

    let mut g = c.benchmark_group("worst");
    g.sample_size(20);
    for (name, text, is_active) in &cases {
        g.throughput(Throughput::Bytes(text.len() as u64));
        if *is_active {
            g.bench_function(*name, |b| b.iter(|| black_box(active(black_box(text), &empty))));
        } else {
            g.bench_function(*name, |b| b.iter(|| black_box(parse(black_box(text)))));
        }
    }
    g.finish();
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "synx") {
            out.push(path);
        }
    }
}

/// Real files, when the host has some: `SYNX_BENCH_CORPUS=<dir>`.
fn bench_corpus(c: &mut Criterion) {
    let Some(dir) = std::env::var_os("SYNX_BENCH_CORPUS") else { return };
    let mut paths = Vec::new();
    collect(Path::new(&dir), &mut paths);
    paths.sort();
    let texts: Vec<String> = paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    if texts.is_empty() {
        eprintln!("SYNX_BENCH_CORPUS: no .synx files under {:?}", dir);
        return;
    }
    let bytes: usize = texts.iter().map(|t| t.len()).sum();
    eprintln!("corpus: {} files, {} bytes", texts.len(), bytes);
    // Every variable a document asks for is set, so each marker does its work.
    let env: HashMap<String, String> = texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| l.contains(":env"))
        .filter_map(|l| l.split_whitespace().last())
        .map(|name| (name.to_string(), "4242".to_string()))
        .collect();
    let largest = texts.iter().max_by_key(|t| t.len()).unwrap();

    let mut g = c.benchmark_group("corpus");
    g.sample_size(20);
    g.throughput(Throughput::Bytes(bytes as u64));
    g.bench_function("parse-all", |b| {
        b.iter(|| {
            for t in &texts {
                black_box(parse(black_box(t)));
            }
        })
    });
    g.bench_function("parse+resolve-all", |b| {
        b.iter(|| {
            for t in &texts {
                black_box(active(black_box(t), &env));
            }
        })
    });
    g.bench_function("parse+to_json-all", |b| {
        b.iter(|| {
            for t in &texts {
                black_box(to_json(&parse(black_box(t)).root));
            }
        })
    });
    g.bench_function("format-all", |b| {
        b.iter(|| {
            for t in &texts {
                black_box(Synx::format(black_box(t)));
            }
        })
    });
    g.throughput(Throughput::Bytes(largest.len() as u64));
    g.bench_function("parse-largest", |b| b.iter(|| black_box(parse(black_box(largest)))));
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_millis(800)).measurement_time(Duration::from_secs(2));
    targets = bench_parse, bench_active, bench_output, bench_binary, bench_synxl, bench_worst, bench_corpus
}
criterion_main!(benches);
