#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // Compile must not panic
        let compiled = synx_core::Synx::compile(text, false);

        // Decompile the output — round-trip must not panic
        if !compiled.is_empty() {
            let _ = synx_core::Synx::decompile(&compiled);
        }

        // Compile with resolved=true
        let _ = synx_core::Synx::compile(text, true);
    }

    // Feed raw bytes directly to decompile — must not panic
    let _ = synx_core::Synx::decompile(data);
    let _ = synx_core::Synx::is_synxb(data);
});
