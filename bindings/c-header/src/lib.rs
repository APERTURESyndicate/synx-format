//! SYNX C FFI — exposes synx_parse, synx_stringify, synx_format and synx_free for C/C++/Go/etc.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};
use synx_core::{parse, to_json};

/// Runs `f`; a panic inside it becomes `fallback`.
///
/// safety: a panic that leaves an `extern "C"` function aborts the process.
/// Every host of this library (a JVM through JNA, a C server) would die on a
/// document that trips a bug in the engine, so each entry point returns its
/// error value instead.
fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Borrows a C string as UTF-8 text; `None` for NULL or invalid UTF-8.
///
/// # Safety
/// `ptr` is NULL or a valid null-terminated C string that outlives the call.
unsafe fn text_of<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// Hands a Rust string to the caller; NULL if it holds a NUL byte.
fn into_c(text: String) -> *mut c_char {
    match CString::new(text) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The environment `:env` may read, from a JSON object of strings.
fn env_from_json(json: &str) -> Option<HashMap<String, String>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let object = value.as_object()?;
    let mut env = HashMap::with_capacity(object.len());
    for (name, value) in object {
        env.insert(name.clone(), value.as_str()?.to_string());
    }
    Some(env)
}

/// Resolution options from the two optional `_ex` arguments.
///
/// # Safety
/// Each pointer is NULL or a valid null-terminated C string.
unsafe fn options_of(env_json: *const c_char, base_path: *const c_char) -> Option<synx_core::Options> {
    let mut options = synx_core::Options::default();
    if !env_json.is_null() {
        options.env = Some(env_from_json(unsafe { text_of(env_json) }?)?);
    }
    if !base_path.is_null() {
        options.base_path = Some(unsafe { text_of(base_path) }?.to_string());
    }
    Some(options)
}

fn parse_active_json(text: &str, options: &synx_core::Options) -> String {
    let mut result = parse(text);
    if result.mode == synx_core::Mode::Active {
        synx_core::resolve(&mut result, options);
    }
    to_json(&result.root)
}

fn parse_tool_json(text: &str, options: &synx_core::Options) -> String {
    let mut result = parse(text);
    if result.mode == synx_core::Mode::Active {
        synx_core::resolve(&mut result, options);
    }
    let shaped = synx_core::reshape_tool_output(&result.root, result.schema);
    to_json(&shaped)
}

/// Parse a SYNX string and return a JSON string.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_parse(input: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        into_c(to_json(&parse(text).root))
    })
}

/// Parse a SYNX string with engine resolution (active mode) and return JSON.
/// `:env` reads the process environment and files resolve in the current
/// directory; `synx_parse_active_ex` sets both.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_parse_active(input: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        into_c(parse_active_json(text, &synx_core::Options::default()))
    })
}

/// `synx_parse_active` with the environment and the base directory chosen by
/// the caller.
///
/// `env_json` is a JSON object of strings — the only variables `:env` sees;
/// `"{}"` gives the document none, NULL gives it the process environment.
/// `base_path` is the directory `:include`, `:watch`, `!use` and the `:once`
/// lock file resolve in and are jailed to; NULL is the current directory.
/// Returns NULL when either argument is not valid.
///
/// safety: a host that parses a document it did not write had no way to keep
/// `key:env DATABASE_URL` from returning its own environment.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string; `env_json` and
/// `base_path` must each be NULL or one.
#[no_mangle]
pub unsafe extern "C" fn synx_parse_active_ex(
    input: *const c_char,
    env_json: *const c_char,
    base_path: *const c_char,
) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        let Some(options) = (unsafe { options_of(env_json, base_path) }) else {
            return std::ptr::null_mut();
        };
        into_c(parse_active_json(text, &options))
    })
}

/// Convert a JSON string (representing SYNX data) back to SYNX format text.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `json_input` must be a valid null-terminated UTF-8 C string containing valid JSON.
#[no_mangle]
pub unsafe extern "C" fn synx_stringify(json_input: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(json) = (unsafe { text_of(json_input) }) else {
            return std::ptr::null_mut();
        };
        // Parse JSON into Value, then stringify to SYNX
        let val: synx_core::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };
        into_c(synx_core::Synx::stringify(&val))
    })
}

/// Reformat a SYNX string into canonical form (sorted keys, normalized indentation).
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_format(input: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        into_c(synx_core::Synx::format(text))
    })
}

/// Parse a `!tool` SYNX string and return reshaped JSON.
/// Call mode: `{ "tool": "name", "params": { ... } }`.
/// Schema mode (`!tool\n!schema`): `{ "tools": [ { "name": ..., "params": ... } ] }`.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_parse_tool(input: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        into_c(parse_tool_json(text, &synx_core::Options::default()))
    })
}

/// `synx_parse_tool` with the environment and the base directory chosen by the
/// caller; the arguments are those of `synx_parse_active_ex`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string; `env_json` and
/// `base_path` must each be NULL or one.
#[no_mangle]
pub unsafe extern "C" fn synx_parse_tool_ex(
    input: *const c_char,
    env_json: *const c_char,
    base_path: *const c_char,
) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        let Some(options) = (unsafe { options_of(env_json, base_path) }) else {
            return std::ptr::null_mut();
        };
        into_c(parse_tool_json(text, &options))
    })
}

/// Free a string returned by synx_parse, synx_parse_active, synx_stringify, or synx_format.
///
/// # Safety
/// `ptr` must be a pointer returned by a synx_* function,
/// and must not have been previously freed.
#[no_mangle]
pub unsafe extern "C" fn synx_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Free a byte buffer returned by `synx_compile`.
///
/// # Safety
/// `ptr` and `len` must originate from a prior `synx_compile` call.
#[no_mangle]
pub unsafe extern "C" fn synx_free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        // The buffer left `synx_compile` as a boxed slice of exactly `len` bytes.
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len))) };
    }
}

/// Compile a SYNX string into compact binary .synxb format.
/// Sets `*out_len` to the byte count. Caller must free with `synx_free_bytes`.
/// `resolved` is 0 or non-zero, as `int resolved` in synx.h.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
/// `out_len` must point to a valid `usize`.
#[no_mangle]
pub unsafe extern "C" fn synx_compile(
    input: *const c_char,
    // safety: declared `bool` here and `int` in synx.h. A C caller passing 2
    // put a value in a Rust `bool` that is neither true nor false — undefined
    // behaviour, seen as a third state where the flag was set and nothing was
    // resolved.
    resolved: c_int,
    out_len: *mut usize,
) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    guarded(std::ptr::null_mut(), || {
        let Some(text) = (unsafe { text_of(input) }) else {
            return std::ptr::null_mut();
        };
        // safety: the `Vec` used to be leaked with `forget` and rebuilt in
        // `synx_free_bytes` as `from_raw_parts(ptr, len, len)`. Its capacity
        // is usually larger than its length, and freeing with the wrong
        // capacity is undefined behaviour. A boxed slice has no spare capacity.
        let data = synx_core::Synx::compile(text, resolved != 0).into_boxed_slice();
        unsafe { *out_len = data.len() };
        if data.is_empty() {
            return std::ptr::null_mut();
        }
        Box::into_raw(data) as *mut u8
    })
}

/// Decompile a .synxb binary back into a SYNX string.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn synx_decompile(
    data: *const u8,
    len: usize,
) -> *mut c_char {
    if data.is_null() || len == 0 {
        return std::ptr::null_mut();
    }
    guarded(std::ptr::null_mut(), || {
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        match synx_core::Synx::decompile(slice) {
            Ok(text) => into_c(text),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Check whether the given bytes start with the .synxb magic header.
/// Returns 1 or 0.
///
/// # Safety
/// `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn synx_is_synxb(data: *const u8, len: usize) -> c_int {
    // safety: this returned a Rust `bool` — one byte — while synx.h and the
    // Kotlin binding read an `int`. The other three bytes of the register
    // were whatever was left there, so the answer was "yes" for nearly any
    // input.
    if data.is_null() || len < 5 {
        return 0;
    }
    guarded(0, || {
        let slice = unsafe { std::slice::from_raw_parts(data, len) };
        c_int::from(synx_core::Synx::is_synxb(slice))
    })
}

/// Structural diff between two SYNX strings. Returns JSON.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input_a` and `input_b` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn synx_diff(input_a: *const c_char, input_b: *const c_char) -> *mut c_char {
    guarded(std::ptr::null_mut(), || {
        let (Some(text_a), Some(text_b)) = (unsafe { text_of(input_a) }, unsafe { text_of(input_b) }) else {
            return std::ptr::null_mut();
        };
        let map_a = synx_core::Synx::parse(text_a);
        let map_b = synx_core::Synx::parse(text_b);
        let result = synx_core::Synx::diff(&map_a, &map_b);
        let val = synx_core::diff_to_value(&result);
        into_c(to_json(&val))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_and_free(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null(), "expected non-null pointer");
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .expect("utf-8 output expected")
            .to_string();
        unsafe { synx_free(ptr) };
        s
    }

    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    #[test]
    fn smoke_parse_and_format() {
        let input = CString::new("name John\nage 25\n").unwrap();
        let json_ptr = unsafe { synx_parse(input.as_ptr()) };
        let json = read_and_free(json_ptr);
        assert!(json.contains("\"name\":\"John\""));

        let fmt_in = CString::new("b 2\na 1\n").unwrap();
        let fmt_ptr = unsafe { synx_format(fmt_in.as_ptr()) };
        let formatted = read_and_free(fmt_ptr);
        assert!(formatted.contains("a 1"));
        assert!(formatted.contains("b 2"));
    }

    #[test]
    fn smoke_stringify() {
        let input = CString::new("{\"name\":\"John\",\"age\":25}").unwrap();
        let synx_ptr = unsafe { synx_stringify(input.as_ptr()) };
        let synx = read_and_free(synx_ptr);
        assert!(synx.contains("name John"));
        assert!(synx.contains("age 25"));
    }

    // The header promises `int`: every byte of the result is defined.
    #[test]
    fn is_synxb_answers_with_a_whole_int() {
        let call: unsafe extern "C" fn(*const u8, usize) -> c_int = synx_is_synxb;
        let text = b"hello world, definitely not a binary";
        assert_eq!(unsafe { call(text.as_ptr(), text.len()) }, 0);
        assert_eq!(unsafe { call(std::ptr::null(), 10) }, 0);
        assert_eq!(unsafe { call(text.as_ptr(), 4) }, 0);

        let mut len = 0usize;
        let bin = unsafe { synx_compile(c("k v\n").as_ptr(), 0, &mut len) };
        assert!(!bin.is_null());
        assert_eq!(unsafe { call(bin, len) }, 1);
        unsafe { synx_free_bytes(bin, len) };
    }

    // `int resolved`: any non-zero value means "resolved", as C callers expect.
    #[test]
    fn compile_takes_resolved_as_an_int() {
        let source = c("!active\nport:env:default:8080 SYNX_C_TEST_UNSET\n");
        let compile = |resolved: c_int| {
            let mut len = 0usize;
            let ptr = unsafe { synx_compile(source.as_ptr(), resolved, &mut len) };
            assert!(!ptr.is_null());
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
            unsafe { synx_free_bytes(ptr, len) };
            bytes
        };
        let unresolved = compile(0);
        let resolved = compile(1);
        assert_ne!(unresolved, resolved);
        for odd in [2, 3, 255, -1, 256] {
            assert_eq!(compile(odd), resolved, "resolved = {}", odd);
        }
    }

    // The buffer is freed with the capacity it was allocated with.
    #[test]
    fn compiled_bytes_round_trip_and_free() {
        for size in [1usize, 7, 64, 1000, 4097] {
            let text = format!("key {}\n", "x".repeat(size));
            let mut len = 0usize;
            let ptr = unsafe { synx_compile(c(&text).as_ptr(), 0, &mut len) };
            assert!(!ptr.is_null() && len > 0);
            let back = read_and_free(unsafe { synx_decompile(ptr, len) });
            assert!(back.contains("key"));
            unsafe { synx_free_bytes(ptr, len) };
        }
        let mut len = 0usize;
        assert!(unsafe { synx_compile(std::ptr::null(), 0, &mut len) }.is_null());
        assert!(unsafe { synx_compile(c("a 1\n").as_ptr(), 0, std::ptr::null_mut()) }.is_null());
    }

    #[test]
    fn parse_active_ex_gives_the_document_only_the_environment_it_is_handed() {
        std::env::set_var("SYNX_C_TEST_SECRET", "from-process");
        let doc = c("!active\nleak:env SYNX_C_TEST_SECRET\nport:env:default:8080 SYNX_C_TEST_UNSET\n");

        // The plain call keeps reading the process environment.
        let plain = read_and_free(unsafe { synx_parse_active(doc.as_ptr()) });
        assert!(plain.contains("from-process"), "{}", plain);

        let empty = read_and_free(unsafe {
            synx_parse_active_ex(doc.as_ptr(), c("{}").as_ptr(), std::ptr::null())
        });
        assert!(!empty.contains("from-process"), "{}", empty);
        assert!(empty.contains("\"port\":8080"), "{}", empty);

        let given = read_and_free(unsafe {
            synx_parse_active_ex(doc.as_ptr(), c("{\"SYNX_C_TEST_SECRET\":\"handed\"}").as_ptr(), std::ptr::null())
        });
        assert!(given.contains("\"leak\":\"handed\""), "{}", given);

        // NULL environment: the process one, as the plain call.
        let inherited = read_and_free(unsafe {
            synx_parse_active_ex(doc.as_ptr(), std::ptr::null(), std::ptr::null())
        });
        assert_eq!(inherited, plain);

        for bad_env in ["not json", "[1]", "{\"A\":1}", "\"x\""] {
            let got = unsafe { synx_parse_active_ex(doc.as_ptr(), c(bad_env).as_ptr(), std::ptr::null()) };
            assert!(got.is_null(), "{}", bad_env);
        }
        std::env::remove_var("SYNX_C_TEST_SECRET");
    }

    #[test]
    fn parse_active_ex_resolves_files_in_the_base_it_is_handed() {
        let dir = std::env::temp_dir().join(format!("synx-c-base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        std::fs::write(dir.join("inside").join("part.synx"), "name inside\n").unwrap();
        std::fs::write(dir.join("outside.synx"), "name outside\n").unwrap();
        let base = c(&dir.join("inside").to_string_lossy());
        let doc = c("!active\nok:include part.synx\nescape:include ../outside.synx\n");

        let json = read_and_free(unsafe {
            synx_parse_active_ex(doc.as_ptr(), c("{}").as_ptr(), base.as_ptr())
        });
        assert!(json.contains("\"ok\":{\"name\":\"inside\"}"), "{}", json);
        assert!(!json.contains("name\":\"outside"), "{}", json);
        assert!(json.contains("INCLUDE_ERR"), "{}", json);

        let tool = read_and_free(unsafe {
            synx_parse_tool_ex(c("!tool\nsearch\n  query x\n").as_ptr(), c("{}").as_ptr(), base.as_ptr())
        });
        assert!(tool.contains("\"tool\":\"search\""), "{}", tool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panic_inside_an_entry_point_becomes_the_error_value() {
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let ptr: *mut c_char = guarded(std::ptr::null_mut(), || panic!("engine bug"));
        let flag: c_int = guarded(0, || panic!("engine bug"));
        std::panic::set_hook(quiet);
        assert!(ptr.is_null());
        assert_eq!(flag, 0);
    }

    #[test]
    fn invalid_input_is_null_not_a_crash() {
        assert!(unsafe { synx_parse(std::ptr::null()) }.is_null());
        assert!(unsafe { synx_parse_active(std::ptr::null()) }.is_null());
        assert!(unsafe { synx_parse_tool(std::ptr::null()) }.is_null());
        assert!(unsafe { synx_format(std::ptr::null()) }.is_null());
        assert!(unsafe { synx_stringify(c("not json {{{").as_ptr()) }.is_null());
        assert!(unsafe { synx_diff(std::ptr::null(), c("a 1").as_ptr()) }.is_null());
        let invalid_utf8 = [0xffu8, 0xfe, 0x00];
        assert!(unsafe { synx_parse(invalid_utf8.as_ptr() as *const c_char) }.is_null());
        let garbage = [0x53u8, 0x59, 0x4e, 0x58, 0x42, 0x01, 0x00, 0xff, 0xff, 0xff, 0xff, 0x01, 0x02];
        assert!(unsafe { synx_decompile(garbage.as_ptr(), garbage.len()) }.is_null());
        unsafe { synx_free(std::ptr::null_mut()) };
        unsafe { synx_free_bytes(std::ptr::null_mut(), 0) };
    }
}
