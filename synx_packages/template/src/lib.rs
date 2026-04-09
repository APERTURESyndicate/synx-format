//! Custom SYNX WASM markers — starter template.
//!
//! Build:
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/my_synx_markers.wasm ./markers.wasm
//!
//! ## How to add a marker
//! 1. Add the marker name to the JSON array in `synx_markers()`
//! 2. Add a match arm in `synx_apply()`
//! 3. Build and copy the .wasm file
//!
//! ## ABI v1
//! - `synx_alloc(size) → ptr`           — host allocates guest memory
//! - `synx_markers() → packed(ptr,len)` — return JSON array of marker names
//! - `synx_apply(ptr, len) → packed`    — receive JSON request, return JSON result

use std::alloc::{alloc, Layout};

// ── Memory allocator (host → guest) ─────────────────────────

#[no_mangle]
pub extern "C" fn synx_alloc(size: i32) -> i32 {
    let layout = Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc(layout) as i32 }
}

// ── Helpers ─────────────────────────────────────────────────

fn write_output(s: &str) -> i64 {
    let bytes = s.as_bytes();
    let ptr = synx_alloc(bytes.len() as i32);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    }
    ((ptr as i64) << 32) | (bytes.len() as i64)
}

fn json_ok(value: &str) -> String {
    format!("{{\"value\":{}}}", serde_json::Value::String(value.to_string()))
}

fn json_err(msg: &str) -> String {
    format!("{{\"error\":{}}}", serde_json::Value::String(msg.to_string()))
}

// ── Marker registry ─────────────────────────────────────────

/// Return a JSON array of marker names this module provides.
/// Add your marker names here.
#[no_mangle]
pub extern "C" fn synx_markers() -> i64 {
    // ✏️  Add your marker names to this array:
    let json = r#"["shout","repeat"]"#;
    write_output(json)
}

// ── Marker dispatch ─────────────────────────────────────────

/// Receive a JSON request: {"marker": "name", "value": "input", "args": [...]}
/// Return a JSON response: {"value": "result"} or {"error": "message"}
#[no_mangle]
pub extern "C" fn synx_apply(in_ptr: i32, in_len: i32) -> i64 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(in_ptr as *const u8, in_len as usize);
        String::from_utf8_lossy(slice).into_owned()
    };

    let req: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => return write_output(&json_err(&format!("invalid JSON: {}", e))),
    };

    let marker = req["marker"].as_str().unwrap_or("");
    let value = req["value"].as_str().unwrap_or("");
    let args: Vec<&str> = req["args"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // ✏️  Add your marker implementations here:
    let result = match marker {
        "shout" => json_ok(&format!("{}!!!", value.to_uppercase())),
        "repeat" => {
            let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(2);
            json_ok(&value.repeat(n))
        }
        _ => json_err(&format!("unknown marker: {}", marker)),
    };

    write_output(&result)
}
