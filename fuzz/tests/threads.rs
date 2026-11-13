//! Many threads resolving documents at once. The engine keeps a process-wide
//! table (`:spam`), per-thread budgets and caches, and a lock file on disk
//! (`:once`): whatever one thread does, another must get the answer it would
//! get alone.
//!
//!   cargo test --release --offline --manifest-path fuzz/Cargo.toml --test threads

use std::collections::HashMap;
use std::sync::{Arc, Barrier};
use synx_core::{Options, Synx};

const THREADS: usize = 8;
const ROUNDS: usize = 400;

fn options(base: &std::path::Path) -> Options {
    let mut o = Options::default();
    o.env = Some(HashMap::from([("PORT".to_string(), "8080".to_string())]));
    o.base_path = Some(base.to_string_lossy().to_string());
    o
}

/// A document whose answer does not depend on time, chance or other documents.
fn steady(i: usize) -> String {
    format!(
        "!active\nbase\n  port:env:default:1 PORT\n  name n{i}\nchild:inherit base\n  extra {i}\nsum:calc {i} * 2 + base.port\n\
         id[pattern:^n\\d+$] n{i}\ncode[pattern:^[a-z]+$] {i}\nwide[min:1, max:5] {i}\ntext {{base.name}}-{{sum}}\n\
         list:split:, a,b,c{i}\nup:upper x{i}\nsecret:secret s{i}\ncopy:alias secret\ninc:include part.synx\n"
    )
}

#[test]
fn threads_get_the_answer_one_thread_gets() {
    let base = std::env::temp_dir().join(format!("synx-threads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("part.synx"), "!active\nv:calc 2 + 3\n").unwrap();

    let alone: Vec<String> = (0..ROUNDS).map(|i| synx_core::to_json(&synx_core::Value::Object(Synx::parse_active(&steady(i), &options(&base))))).collect();
    let alone = Arc::new(alone);
    let start = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let alone = Arc::clone(&alone);
            let start = Arc::clone(&start);
            let base = base.clone();
            std::thread::spawn(move || {
                start.wait();
                let mut wrong = Vec::new();
                for round in 0..ROUNDS {
                    let i = (round * 7 + t * 13) % ROUNDS;
                    let got = synx_core::to_json(&synx_core::Value::Object(Synx::parse_active(&steady(i), &options(&base))));
                    if got != alone[i] && wrong.len() < 3 {
                        wrong.push(format!("document {i} in thread {t}:\n  alone  {}\n  now    {}", alone[i], got));
                    }
                    // Noise from the shared pieces: the `:spam` table, `:once`, `:random`.
                    let noisy = format!("!active\nk{t}:spam:3:60 base\nbase 1\nonce_{t}_{round}:once\nr:random\n  - a\n  - b\n");
                    let _ = Synx::parse_active(&noisy, &options(&base));
                }
                wrong
            })
        })
        .collect();
    let mut wrong = Vec::new();
    for h in handles {
        wrong.extend(h.join().expect("a thread panicked"));
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `:spam` counts per key and target across the whole process: with a limit of
/// `LIMIT` calls, exactly `LIMIT` of the calls made from all threads pass.
#[test]
fn spam_limit_holds_across_threads() {
    const LIMIT: usize = 50;
    let base = std::env::temp_dir();
    let key = format!("gate{}", std::process::id());
    let doc = Arc::new(format!("!active\ntarget 1\n{key}:spam:{LIMIT}:600 target\n"));
    let start = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let doc = Arc::clone(&doc);
            let start = Arc::clone(&start);
            let key = key.clone();
            let base = base.clone();
            std::thread::spawn(move || {
                start.wait();
                (0..100)
                    .filter(|_| {
                        let map = Synx::parse_active(&doc, &options(&base));
                        !synx_core::to_json(map.get(&key).unwrap()).contains("SPAM_ERR")
                    })
                    .count()
            })
        })
        .collect();
    let passed: usize = handles.into_iter().map(|h| h.join().expect("a thread panicked")).sum();
    assert_eq!(passed, LIMIT, "{passed} of {} calls passed a limit of {LIMIT}", THREADS * 100);
}
