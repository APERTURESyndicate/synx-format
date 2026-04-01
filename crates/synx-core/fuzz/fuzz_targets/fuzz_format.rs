#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // Canonical formatter must not panic
        let _ = synx_core::Synx::format(text);

        // safe_calc must not panic on arbitrary expressions
        let _ = synx_core::safe_calc(text);
    }
});
