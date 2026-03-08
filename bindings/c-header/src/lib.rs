//! SYNX C FFI — exposes synx_parse, synx_stringify, synx_format and synx_free for C/C++/Go/etc.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use synx_core::{parse, to_json};

/// Parse a SYNX string and return a JSON string.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_parse(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(input) };
    let text = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let result = parse(text);
    let json = to_json(&result.root);
    match CString::new(json) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Parse a SYNX string with engine resolution (active mode) and return JSON.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_parse_active(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(input) };
    let text = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let mut result = parse(text);
    if result.mode == synx_core::Mode::Active {
        synx_core::resolve(&mut result, &synx_core::Options::default());
    }
    let json = to_json(&result.root);
    match CString::new(json) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Convert a JSON string (representing SYNX data) back to SYNX format text.
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `json_input` must be a valid null-terminated UTF-8 C string containing valid JSON.
#[no_mangle]
pub unsafe extern "C" fn synx_stringify(json_input: *const c_char) -> *mut c_char {
    if json_input.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(json_input) };
    let json = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    // Parse JSON into Value, then stringify to SYNX
    let val: synx_core::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };
    let synx_text = synx_core::Synx::stringify(&val);
    match CString::new(synx_text) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Reformat a SYNX string into canonical form (sorted keys, normalized indentation).
/// Caller must free the result with `synx_free`.
///
/// # Safety
/// `input` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn synx_format(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(input) };
    let text = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let formatted = synx_core::Synx::format(text);
    match CString::new(formatted) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
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
}
