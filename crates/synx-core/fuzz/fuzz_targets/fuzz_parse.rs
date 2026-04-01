#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // Must not panic on any input
        let result = synx_core::parse(text);

        // Exercise serialisation paths
        let _ = synx_core::to_json(&result.root);
        let _ = synx_core::Synx::stringify(&result.root);

        // Exercise active engine (safe even without !active — just a no-op)
        let mut result2 = synx_core::parse(text);
        synx_core::resolve(&mut result2, &synx_core::Options::default());

        // Exercise tool reshape
        let _ = synx_core::reshape_tool_output(&result.root, result.schema);
    }
});
