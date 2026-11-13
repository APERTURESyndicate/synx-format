//! The C ABI (`bindings/c-header`) compiled into this test, so that Miri can
//! follow every pointer the library hands out and takes back. The library's
//! own unit tests run here too.
//!
//!   cargo +nightly miri test --offline --manifest-path fuzz/Cargo.toml --test c_abi
//!   (MIRIFLAGS="-Zmiri-disable-isolation"; add -Zmiri-tree-borrows for the other model)

#[allow(dead_code, clippy::all)]
#[path = "../../bindings/c-header/src/lib.rs"]
mod c_abi;

use c_abi::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

fn take(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { synx_free(ptr) };
    Some(text)
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

#[test]
fn every_string_entry_point_returns_memory_that_frees_cleanly() {
    // One key: the parser's own cache of parents is not what this test is about.
    let doc = c("name x\n");
    assert_eq!(take(unsafe { synx_parse(doc.as_ptr()) }).as_deref(), Some("{\"name\":\"x\"}"));
    assert!(take(unsafe { synx_format(doc.as_ptr()) }).is_some());
    assert!(take(unsafe { synx_parse_tool(c("!tool\nsearch\n").as_ptr()) }).is_some());
    assert!(take(unsafe { synx_stringify(c("{\"a\":[1,2,{\"b\":null}]}").as_ptr()) }).is_some());
    assert!(take(unsafe { synx_diff(doc.as_ptr(), c("name y\n").as_ptr()) }).is_some());
    let active = c("!active\nport:env:default:8080 PORT\n");
    assert_eq!(
        take(unsafe { synx_parse_active_ex(active.as_ptr(), c("{\"PORT\":\"1\"}").as_ptr(), std::ptr::null()) }).as_deref(),
        Some("{\"port\":1}")
    );
}

#[test]
fn compiled_bytes_go_back_through_every_reader() {
    for text in ["k v\n", "!active\nport:env:default:8080 PORT\n", ""] {
        for resolved in [0, 1, 7] {
            let mut len = usize::MAX;
            let ptr = unsafe { synx_compile(c(text).as_ptr(), resolved, &mut len) };
            if ptr.is_null() {
                continue;
            }
            assert_eq!(unsafe { synx_is_synxb(ptr, len) }, 1);
            assert!(take(unsafe { synx_decompile(ptr, len) }).is_some());
            // Every prefix is a truncated container: refused or read, never out of bounds.
            for cut in 0..len {
                let _ = take(unsafe { synx_decompile(ptr, cut) });
                let _ = unsafe { synx_is_synxb(ptr, cut) };
            }
            unsafe { synx_free_bytes(ptr, len) };
        }
    }
}

#[test]
fn text_that_cannot_be_a_c_string_is_null() {
    // The SYNX text of this JSON holds U+0000.
    assert!(take(unsafe { synx_stringify(c("{\"a\":\"x\\u0000y\"}").as_ptr()) }).is_none());
    let not_utf8 = [b'a', b' ', 0xff, 0];
    assert!(take(unsafe { synx_parse(not_utf8.as_ptr() as *const c_char) }).is_none());
    assert!(take(unsafe { synx_parse_active_ex(c("a 1\n").as_ptr(), not_utf8.as_ptr() as *const c_char, std::ptr::null()) }).is_none());
    assert!(take(unsafe { synx_parse_active_ex(c("a 1\n").as_ptr(), std::ptr::null(), not_utf8.as_ptr() as *const c_char) }).is_none());
}
