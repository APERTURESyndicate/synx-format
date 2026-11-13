//! Worker for the adversarial suite in `tests/adversarial/`.
//!
//! Two ways to run it:
//!
//! * `advharness` — reads one JSON request per line on stdin and answers with
//!   one JSON line on stdout. A panic is caught and reported; a stack overflow
//!   or an allocation failure kills the process, which is how the driver
//!   learns about those.
//! * `advharness once <request.json>` — one request from a file, with wall
//!   time and peak memory of this process in the answer. Used for the
//!   size-versus-cost curves, where every measurement needs a fresh process.
//!
//! Request fields: `id`, `op`, `b64` (input bytes), `b64b` (second input),
//! `env` (object; absent means an empty environment — the host environment is
//! never read unless `"env": null` is sent on purpose), `base`, `lang`,
//! `region`, `resolved`, `validate`, `needle`, `depth`.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value as J};
use synx_core::{Mode, Options, Synx, Value};

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

fn main() {
    panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file().replace('\\', "/"), l.line(), l.column()))
            .unwrap_or_default();
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "panic".to_string()
        };
        if let Ok(mut slot) = LAST_PANIC.lock() {
            *slot = Some(format!("{} @ {}", msg, loc));
        }
    }));

    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "once" {
        let raw = std::fs::read_to_string(&args[2]).expect("request file");
        let req: J = serde_json::from_str(&raw).expect("request json");
        let resp = handle(&req);
        println!("{}", resp);
        return;
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<J>(&line) {
            Ok(req) => handle(&req),
            Err(e) => json!({"ok": false, "err": format!("bad request: {}", e)}),
        };
        let mut out = stdout.lock();
        let _ = writeln!(out, "{}", resp);
        let _ = out.flush();
    }
}

fn handle(req: &J) -> J {
    let id = req.get("id").cloned().unwrap_or(J::Null);
    let op = req.get("op").and_then(J::as_str).unwrap_or("").to_string();
    let started = Instant::now();
    if let Ok(mut slot) = LAST_PANIC.lock() {
        *slot = None;
    }
    let result = panic::catch_unwind(AssertUnwindSafe(|| run(&op, req)));
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    // `brief`: only the size of each long string comes back. A cost
    // measurement must not ship a gigabyte answer through a pipe.
    let brief = flag(req, "brief");
    let mut resp = match result {
        Ok(Ok(out)) => json!({"id": id, "ok": true, "out": if brief { shorten(out) } else { out }}),
        Ok(Err(e)) if brief && e.len() > 300 => {
            json!({"id": id, "ok": false, "err": format!("{}… ({} bytes)", &e[..e.floor_char_boundary(200)], e.len())})
        }
        Ok(Err(e)) => json!({"id": id, "ok": false, "err": e}),
        Err(_) => {
            let msg = LAST_PANIC.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
            json!({"id": id, "ok": false, "panic": msg})
        }
    };
    resp["ms"] = json!((ms * 1000.0).round() / 1000.0);
    resp["peak_mb"] = json!(peak_commit_mb());
    resp
}

/// Replace every string longer than 200 bytes by `{"len": n}`.
fn shorten(v: J) -> J {
    match v {
        J::String(s) if s.len() > 200 => json!({"len": s.len()}),
        J::Array(a) => J::Array(a.into_iter().map(shorten).collect()),
        J::Object(m) => J::Object(m.into_iter().map(|(k, v)| (k, shorten(v))).collect()),
        other => other,
    }
}

// ─── Request helpers ─────────────────────────────────────────

fn bytes_of(req: &J, field: &str) -> Vec<u8> {
    match req.get(field).and_then(J::as_str) {
        Some(s) => b64_decode(s),
        None => Vec::new(),
    }
}

fn text_of(req: &J, field: &str) -> Result<String, String> {
    if field == "b64" {
        if let Some(t) = req.get("text").and_then(J::as_str) {
            return Ok(t.to_string());
        }
    }
    String::from_utf8(bytes_of(req, field)).map_err(|_| "invalid-utf8".to_string())
}

fn options_of(req: &J) -> Options {
    let env = match req.get("env") {
        Some(J::Null) => None,
        Some(J::Object(m)) => Some(
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect::<HashMap<String, String>>(),
        ),
        _ => Some(HashMap::new()),
    };
    Options {
        env,
        base_path: req.get("base").and_then(J::as_str).map(str::to_string),
        lang: req.get("lang").and_then(J::as_str).map(str::to_string),
        region: req.get("region").and_then(J::as_str).map(str::to_string),
        max_include_depth: req.get("depth").and_then(J::as_u64).map(|d| d as usize),
        ..Default::default()
    }
}

fn flag(req: &J, name: &str) -> bool {
    req.get(name).and_then(J::as_bool).unwrap_or(false)
}

fn resolved_root(text: &str, opts: &Options) -> (Value, Mode) {
    let mut r = synx_core::parse(text);
    let mode = r.mode;
    if r.mode == Mode::Active {
        synx_core::resolve(&mut r, opts);
    }
    (r.root, mode)
}

// ─── Operations ──────────────────────────────────────────────

fn run(op: &str, req: &J) -> Result<J, String> {
    match op {
        "ping" => Ok(json!("pong")),

        // Static parse: the tree as written, markers untouched.
        "parse" => {
            let text = text_of(req, "b64")?;
            let r = synx_core::parse(&text);
            // `brief` is the cost mode: one rendering, as a caller would make.
            let ordered = if flag(req, "brief") { String::new() } else { ordered_json(&r.root) };
            Ok(json!({
                "json": synx_core::to_json(&r.root),
                "ordered": ordered,
                "keys": r.root.as_object().map_or(0, |m| m.len()),
                "active": r.mode == Mode::Active,
                "locked": r.locked,
                "tool": r.tool,
                "schema": r.schema,
                "llm": r.llm,
                "truncated": r.truncated,
                "includes": r.includes.iter().map(|i| json!([i.path, i.alias])).collect::<Vec<_>>(),
                "uses": r.uses.iter().map(|u| json!([u.package, u.alias])).collect::<Vec<_>>(),
            }))
        }

        // What `synx parse` and `Synx.parse` give: resolved when `!active`.
        "active" => {
            let text = text_of(req, "b64")?;
            let (root, mode) = resolved_root(&text, &options_of(req));
            let ordered = if flag(req, "brief") { String::new() } else { ordered_json(&root) };
            Ok(json!({
                "json": synx_core::to_json(&root),
                "ordered": ordered,
                "active": mode == Mode::Active,
            }))
        }

        "tool" => {
            let text = text_of(req, "b64")?;
            let map = Synx::parse_tool(&text, &options_of(req));
            Ok(json!({"json": synx_core::to_json(&Value::Object(map))}))
        }

        "format" => Ok(json!({"text": Synx::format(&text_of(req, "b64")?)})),

        // format twice, and what each stage parses to.
        "format_check" => {
            let text = text_of(req, "b64")?;
            let opts = options_of(req);
            let f1 = Synx::format(&text);
            let f2 = Synx::format(&f1);
            let (r0, m0) = resolved_root(&text, &opts);
            let (r1, m1) = resolved_root(&f1, &opts);
            Ok(json!({
                "f1": f1,
                "idempotent": f1 == f2,
                "f2": if f1 == f2 { J::Null } else { json!(f2) },
                "p0": synx_core::to_json(&r0),
                "p1": synx_core::to_json(&r1),
                "m0": m0 == Mode::Active,
                "m1": m1 == Mode::Active,
            }))
        }

        "stringify" => {
            let text = text_of(req, "b64")?;
            let r = synx_core::parse(&text);
            Ok(json!({"text": Synx::stringify(&r.root)}))
        }

        // parse → stringify → parse.
        "stringify_check" => {
            let text = text_of(req, "b64")?;
            let r = synx_core::parse(&text);
            let s = Synx::stringify(&r.root);
            let back = synx_core::parse(&s);
            Ok(json!({
                "p0": synx_core::to_json(&r.root),
                "p1": synx_core::to_json(&back.root),
                "text": s,
                "active1": back.mode == Mode::Active,
            }))
        }

        // JSON → Value → SYNX text → parse.
        "json2synx" => {
            let text = text_of(req, "b64")?;
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("json: {}", e))?;
            let s = Synx::stringify(&v);
            let back = synx_core::parse(&s);
            Ok(json!({
                "p0": synx_core::to_json(&v),
                "p1": synx_core::to_json(&back.root),
                "text": s,
                "active1": back.mode == Mode::Active,
                "includes1": back.includes.len(),
            }))
        }

        "compile" => {
            let text = text_of(req, "b64")?;
            let data = Synx::compile(&text, flag(req, "resolved"));
            Ok(json!({"b64": b64_encode(&data), "len": data.len()}))
        }

        "decompile" => {
            let data = bytes_of(req, "b64");
            let text = Synx::decompile(&data)?;
            Ok(json!({"text": text}))
        }

        // The typed decoder: what a host gets before any text is written.
        "decompile_tree" => {
            let data = bytes_of(req, "b64");
            let r = synx_core::binary::decompile(&data)?;
            Ok(json!({"json": synx_core::to_json(&r.root), "meta_paths": r.metadata.len()}))
        }

        // A container read both ways: the typed tree, the text `decompile`
        // writes, and what that text reads back as.
        "decompile_full" => {
            let data = bytes_of(req, "b64");
            let typed = synx_core::binary::decompile(&data)?;
            let text = Synx::decompile(&data)?;
            let back = synx_core::parse(&text);
            Ok(json!({
                "tree": synx_core::to_json(&typed.root),
                "active": typed.mode == Mode::Active,
                "includes": typed.includes.len(),
                "text_tree": synx_core::to_json(&back.root),
                "text_active": back.mode == Mode::Active,
                "text_includes": back.includes.len(),
                "text": text,
            }))
        }

        // text → .synxb → tree, and → text → tree.
        "compile_check" => {
            let text = text_of(req, "b64")?;
            let r0 = synx_core::parse(&text);
            let data = synx_core::binary::compile(&r0, false);
            let r1 = synx_core::binary::decompile(&data)?;
            let back_text = Synx::decompile(&data)?;
            let r2 = synx_core::parse(&back_text);
            Ok(json!({
                "p0": synx_core::to_json(&r0.root),
                "p1": synx_core::to_json(&r1.root),
                "p2": synx_core::to_json(&r2.root),
                "meta_equal": r0.metadata == r1.metadata,
                "mode_equal": r0.mode == r1.mode && r0.mode == r2.mode,
                "text": back_text,
            }))
        }

        "synxl" => {
            let text = text_of(req, "b64")?;
            let opts = synx_core::SynxlOptions { validate: flag(req, "validate") };
            match synx_core::parse_lines_with(&text, &opts) {
                Ok(doc) => Ok(json!({
                    "json": doc.to_json(),
                    "diagnostics": doc.diagnostics.iter().map(|d| json!([d.kind.as_str(), d.record_index, d.line])).collect::<Vec<_>>(),
                })),
                Err(e) => Ok(json!({"hard": e.kind.as_str(), "line": e.line})),
            }
        }

        // parse → write → parse, and the streaming reader against the batch one.
        "synxl_check" => {
            let text = text_of(req, "b64")?;
            let doc = match synx_core::parse_lines(&text) {
                Ok(d) => d,
                Err(e) => return Ok(json!({"hard": e.kind.as_str()})),
            };
            let j0 = doc.to_json();
            let written = doc.to_synxl().map_err(|e| format!("write: {}", e))?;
            let j1 = match synx_core::parse_lines(&written) {
                Ok(d) => d.to_json(),
                Err(e) => format!("HARD:{}", e.kind.as_str()),
            };
            let mut stream_json = String::from("[");
            let mut stream_err = J::Null;
            match synx_core::SynxlStreamReader::new(std::io::Cursor::new(text.as_bytes())) {
                Ok(reader) => {
                    let mut first = true;
                    for rec in reader {
                        match rec {
                            Ok(rec) => {
                                if !first {
                                    stream_json.push(',');
                                }
                                first = false;
                                synx_core::write_json(&mut stream_json, &rec.value);
                            }
                            Err(e) => {
                                stream_err = json!(e.to_string());
                                break;
                            }
                        }
                    }
                }
                Err(e) => stream_err = json!(e.to_string()),
            }
            stream_json.push(']');
            Ok(json!({"j0": j0, "j1": j1, "written": written, "stream": stream_json, "stream_err": stream_err}))
        }

        "calc" => {
            let text = text_of(req, "b64")?;
            match synx_core::safe_calc(&text) {
                Ok(f) if f.is_nan() => Ok(json!({"num": "NaN"})),
                Ok(f) if f.is_infinite() => Ok(json!({"num": if f > 0.0 { "Infinity" } else { "-Infinity" }})),
                Ok(f) => Ok(json!({"num": format!("{:?}", f)})),
                Err(e) => Ok(json!({"calc_err": e})),
            }
        }

        "diff" => {
            let a = text_of(req, "b64")?;
            let b = String::from_utf8(bytes_of(req, "b64b")).map_err(|_| "invalid-utf8".to_string())?;
            let d = Synx::diff(&Synx::parse(&a), &Synx::parse(&b));
            Ok(json!({"json": synx_core::to_json(&synx_core::diff_to_value(&d))}))
        }

        "schema" => {
            let text = text_of(req, "b64")?;
            let r = synx_core::parse(&text);
            Ok(json!({"json": synx_core::metadata_to_json_schema(&r.metadata).to_string()}))
        }

        // One `[pattern:…]` against several values with the crate the core
        // compiles patterns with: does it compile, and which values match.
        "regex" => {
            let pat = req.get("pattern").and_then(J::as_str).unwrap_or("");
            let values: Vec<&str> = req
                .get("values")
                .and_then(J::as_array)
                .map(|a| a.iter().filter_map(J::as_str).collect())
                .unwrap_or_default();
            match regex::Regex::new(pat) {
                Ok(re) => Ok(json!({"compiled": true, "m": values.iter().map(|v| re.is_match(v)).collect::<Vec<_>>()})),
                Err(e) => {
                    let why: String = e.to_string().lines().last().unwrap_or("").chars().take(120).collect();
                    Ok(json!({"compiled": false, "m": [], "why": why}))
                }
            }
        }

        #[cfg(feature = "jsonschema")]
        "self_schema" => {
            let text = text_of(req, "b64")?;
            let mut r = synx_core::parse(&text);
            synx_core::resolve(&mut r, &options_of(req));
            let sch = synx_core::metadata_to_json_schema(&r.metadata);
            match synx_core::validate_with_json_schema(&r.root, &sch) {
                Ok(()) => Ok(json!({"errors": []})),
                Err(errs) => Ok(json!({"errors": errs})),
            }
        }

        // Every string a resolved document can be turned into, searched for `needle`.
        "sinks" => {
            let text = text_of(req, "b64")?;
            let needle = req.get("needle").and_then(J::as_str).unwrap_or("").to_string();
            let opts = options_of(req);
            let mut r = synx_core::parse(&text);
            synx_core::resolve(&mut r, &opts);
            let root = r.root.clone();
            let mut sinks: Vec<(&str, String)> = Vec::new();
            sinks.push(("to_json", synx_core::to_json(&root)));
            sinks.push(("stringify", Synx::stringify(&root)));
            sinks.push(("debug", format!("{:?}", root)));
            sinks.push(("debug_result", format!("{:?}", r)));
            let mut shown = String::new();
            collect_display(&root, &mut shown);
            sinks.push(("display", shown));
            sinks.push(("serde_json", serde_json::to_string(&root).unwrap_or_default()));
            sinks.push(("value_to_json_value", synx_core::value_to_json_value(&root).to_string()));
            if let Value::Object(ref m) = root {
                let d = Synx::diff(m, &synx_core::Map::default());
                sinks.push(("diff", synx_core::to_json(&synx_core::diff_to_value(&d))));
                sinks.push(("diff_debug", format!("{:?}", d)));
            }
            let bin = synx_core::binary::compile(&r, true);
            let payload = miniz_oxide::inflate::decompress_to_vec(&bin[11.min(bin.len())..]).unwrap_or_default();
            sinks.push(("synxb_resolved", String::from_utf8_lossy(&payload).into_owned()));
            sinks.push(("decompile", Synx::decompile(&bin).unwrap_or_default()));
            let hits: Vec<&str> = sinks
                .iter()
                .filter(|(_, s)| !needle.is_empty() && s.contains(&needle))
                .map(|(n, _)| *n)
                .collect();
            Ok(json!({"hits": hits, "json": synx_core::to_json(&root)}))
        }

        "sig" => {
            let text = text_of(req, "b64")?;
            let data = bytes_of(req, "b64b");
            let sig = synx_core::signing::PackageSignature::from_hex(&text)?;
            Ok(json!({"verified": sig.verify(&data)?}))
        }

        "pubkey" => {
            let text = text_of(req, "b64")?;
            let key = synx_core::signing::VerifyKey::from_hex(&text)?;
            Ok(json!({"hex": key.to_hex()}))
        }

        // Search for a `:sort` list that panics: `count` random lists of
        // `items` entries mixing integers, floats and quoted numbers. The
        // first document that panics is returned.
        "sort_hunt" => {
            let mut seed = req.get("seed").and_then(J::as_u64).unwrap_or(1) | 1;
            let count = req.get("count").and_then(J::as_u64).unwrap_or(1000);
            let items = req.get("items").and_then(J::as_u64).unwrap_or(40) as usize;
            let mut next = move || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            };
            let opts = options_of(req);
            for round in 0..count {
                let mut doc = String::from("!active\nl:sort\n");
                for _ in 0..items {
                    let v = next() % 30;
                    match next() % 4 {
                        0 => doc.push_str(&format!("  - {}\n", v)),
                        1 => doc.push_str(&format!("  - \"{}\"\n", v)),
                        2 => doc.push_str(&format!("  - {}.5\n", v)),
                        _ => doc.push_str(&format!("  - x{}\n", v)),
                    }
                }
                let caught = panic::catch_unwind(AssertUnwindSafe(|| {
                    let _ = resolved_root(&doc, &opts);
                }));
                if caught.is_err() {
                    let msg = LAST_PANIC.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
                    return Ok(json!({"found": doc, "round": round, "panic": msg}));
                }
            }
            Ok(json!({"found": J::Null, "rounds": count}))
        }

        #[cfg(feature = "wasm")]
        "wasm_load" => {
            let data = bytes_of(req, "b64");
            let mut rt = synx_core::wasm::WasmMarkerRuntime::new();
            match rt.load_module(&data, synx_core::wasm::WasmCapabilities::default()) {
                Ok(names) => Ok(json!({"markers": names})),
                Err(e) => Ok(json!({"wasm_err": e})),
            }
        }

        other => Err(format!("unknown op '{}'", other)),
    }
}

/// JSON with object keys in the order the document declares them — the
/// canonical writer sorts keys, which hides a change of order.
fn ordered_json(v: &Value) -> String {
    fn write(v: &Value, out: &mut String, depth: usize) {
        if depth > 300 {
            out.push_str("null");
            return;
        }
        match v {
            Value::Object(m) => {
                out.push('{');
                for (i, (k, child)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push(':');
                    write(child, out, depth + 1);
                }
                out.push('}');
            }
            Value::Array(a) => {
                out.push('[');
                for (i, child) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(child, out, depth + 1);
                }
                out.push(']');
            }
            leaf => out.push_str(&synx_core::to_json(leaf)),
        }
    }
    let mut out = String::new();
    write(v, &mut out, 0);
    out
}

/// `{}` of every value in the tree, one per line.
fn collect_display(v: &Value, out: &mut String) {
    out.push_str(&format!("{}\n", v));
    match v {
        Value::Object(m) => {
            for (_, child) in m {
                collect_display(child, out);
            }
        }
        Value::Array(a) => {
            for child in a {
                collect_display(child, out);
            }
        }
        _ => {}
    }
}

// ─── Peak committed memory of this process ──────────────────

#[cfg(windows)]
fn peak_commit_mb() -> f64 {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(process: *mut core::ffi::c_void, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: `c` is a properly sized, writable PROCESS_MEMORY_COUNTERS and
    // the pseudo-handle of the current process needs no closing.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 {
        return 0.0;
    }
    ((c.peak_pagefile_usage as f64) / (1024.0 * 1024.0) * 10.0).round() / 10.0
}

#[cfg(not(windows))]
fn peak_commit_mb() -> f64 {
    // VmHWM from /proc/self/status, in MiB.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|kb| kb.parse::<f64>().ok()))
        })
        .map(|kb| (kb / 1024.0 * 10.0).round() / 10.0)
        .unwrap_or(0.0)
}

// ─── base64 (standard alphabet, padding optional on input) ──

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        out.push(B64[(b[0] >> 2) as usize] as char);
        out.push(B64[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 { B64[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[(b[2] & 63) as usize] as char } else { '=' });
    }
    out
}

fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}
