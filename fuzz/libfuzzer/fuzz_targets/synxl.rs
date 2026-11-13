//! SYNXL: batch reader with and without validation, writer and re-read, and
//! the streaming reader over the same bytes.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(doc) = synx_core::parse_lines(text) {
            let _ = doc.to_json();
            if let Ok(written) = doc.to_synxl() {
                let _ = synx_core::parse_lines(&written);
            }
        }
        let _ = synx_core::parse_lines_with(text, &synx_core::SynxlOptions { validate: true });
    }
    if let Ok(reader) = synx_core::SynxlStreamReader::new(std::io::Cursor::new(data)) {
        for rec in reader.take(100_000) {
            if rec.is_err() {
                break;
            }
        }
    }
});
