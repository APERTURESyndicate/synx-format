//! Text through every surface: static parse, JSON, stringify and its re-read,
//! format twice, `.synxb` both ways, and a read-only resolve against an empty
//! environment in a scratch directory with a few files to reach.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();

fn base() -> &'static str {
    BASE.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("synx-libfuzzer-{}", std::process::id()));
        let _ = std::fs::create_dir_all(d.join("sub"));
        let _ = std::fs::write(d.join("a.synx"), "x 1\n!active\ny:alias x\nz:include sub/b.synx\n");
        let _ = std::fs::write(d.join("sub").join("b.synx"), "!active\nq:calc 2 * 3\nr:include ../a.synx\n");
        let _ = std::fs::write(d.join("d.json"), "{\"a\":[1,{\"b\":2}],\"n\":3}");
        d.to_string_lossy().into_owned()
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let mut r = synx_core::parse(text);
    let _ = synx_core::to_json(&r.root);
    let s = synx_core::Synx::stringify(&r.root);
    let _ = synx_core::parse(&s);
    let f = synx_core::Synx::format(text);
    let _ = synx_core::Synx::format(&f);
    let bin = synx_core::binary::compile(&r, false);
    let _ = synx_core::Synx::decompile(&bin);
    let opts = synx_core::Options {
        env: Some(Default::default()),
        base_path: Some(base().to_string()),
        ..Default::default()
    };
    synx_core::resolve_read_only(&mut r, &opts);
    let _ = synx_core::to_json(&r.root);
    let _ = synx_core::Synx::stringify(&r.root);
});
