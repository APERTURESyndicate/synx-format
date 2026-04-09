//! @assynx/text-tools — 8 text utility WASM markers for SYNX.
//!
//! Markers: :upper :lower :reverse :base64 :hash :truncate :pad :count
//!
//! Build:
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/assynx_text_tools.wasm ./markers.wasm

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

fn json_ok_int(value: usize) -> String {
    format!("{{\"value\":{}}}", value)
}

fn json_err(msg: &str) -> String {
    format!("{{\"error\":{}}}", serde_json::Value::String(msg.to_string()))
}

// ── FNV-1a hash ─────────────────────────────────────────────

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ── Base64 encode ───────────────────────────────────────────

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ── Marker registry ─────────────────────────────────────────

#[no_mangle]
pub extern "C" fn synx_markers() -> i64 {
    let json = r#"["upper","lower","reverse","base64","hash","truncate","pad","count"]"#;
    write_output(json)
}

// ── Marker dispatch ─────────────────────────────────────────

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

    let result = match marker {
        // :upper — convert to uppercase
        "upper" => json_ok(&value.to_uppercase()),

        // :lower — convert to lowercase
        "lower" => json_ok(&value.to_lowercase()),

        // :reverse — reverse the string
        "reverse" => json_ok(&value.chars().rev().collect::<String>()),

        // :base64 — encode to base64
        "base64" => json_ok(&base64_encode(value.as_bytes())),

        // :hash — FNV-1a hash as hex string
        "hash" => json_ok(&format!("{:016x}", fnv1a(value.as_bytes()))),

        // :truncate — truncate to max length with ellipsis
        // Usage: :truncate:MAX_LEN
        "truncate" => {
            let max: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(80);
            if value.chars().count() <= max {
                json_ok(value)
            } else {
                let truncated: String = value.chars().take(max.saturating_sub(3)).collect();
                json_ok(&format!("{}...", truncated))
            }
        }

        // :pad — left-pad to width with a fill character
        // Usage: :pad:WIDTH or :pad:WIDTH:FILL_CHAR
        "pad" => {
            let width: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(8);
            let fill = args.get(1).and_then(|s| s.chars().next()).unwrap_or(' ');
            let current = value.chars().count();
            if current >= width {
                json_ok(value)
            } else {
                let padding: String = std::iter::repeat(fill).take(width - current).collect();
                json_ok(&format!("{}{}", padding, value))
            }
        }

        // :count — return character count
        "count" => json_ok_int(value.chars().count()),

        _ => json_err(&format!("unknown marker: {}", marker)),
    };

    write_output(&result)
}
