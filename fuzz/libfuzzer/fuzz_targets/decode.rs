//! Bytes as a `.synxb` container: the typed decoder, the text writer, and what
//! the text reads back as. A valid header is put in front of most inputs so the
//! fuzzer spends its time in the payload.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = synx_core::Synx::is_synxb(data);
    let _ = synx_core::binary::decompile(data);
    let _ = synx_core::Synx::decompile(data);
    if data.is_empty() {
        return;
    }
    // First byte: header flags; the rest: raw payload, deflated here.
    let payload = &data[1..];
    let mut c = b"SYNXB\x01".to_vec();
    c.push(data[0]);
    c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    c.extend_from_slice(&miniz_deflate(payload));
    if let Ok(r) = synx_core::binary::decompile(&c) {
        let _ = synx_core::to_json(&r.root);
        let _ = synx_core::Synx::stringify(&r.root);
    }
    if let Ok(text) = synx_core::Synx::decompile(&c) {
        let _ = synx_core::parse(&text);
    }
});

/// Stored (uncompressed) deflate blocks: no compressor needed, any payload.
fn miniz_deflate(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5 * (payload.len() / 65535 + 1));
    let mut chunks = payload.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}
