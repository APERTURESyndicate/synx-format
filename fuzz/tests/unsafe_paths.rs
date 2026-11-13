//! The `unsafe` paths of synx-core, driven through the public API so that an
//! interpreter with aliasing checks can watch them:
//!
//!   cargo +nightly miri test --manifest-path fuzz/Cargo.toml --test unsafe_paths
//!
//! * `parser.rs` keeps a raw pointer to the last parent map (`NavCache`) and
//!   walks the tree through raw pointers (`navigate_to_parent`).
//! * `engine.rs` reads the whole document through a raw pointer while it holds
//!   a mutable reference to one of its objects (`root_ptr` in `resolve_value`).
//!
//! Under plain `cargo test` these only check that nothing panics.

use synx_core::{parse, resolve, to_json, Options};

fn static_json(text: &str) -> String {
    to_json(&parse(text).root)
}

fn active_json(text: &str) -> String {
    let mut r = parse(text);
    let opts = Options { env: Some(Default::default()), ..Default::default() };
    resolve(&mut r, &opts);
    to_json(&r.root)
}

#[test]
fn parser_parent_cache_across_pushes_and_pops() {
    // Groups that open and close under one parent, list items that become
    // objects, a block flushed by a dedent, a key replaced by a later one.
    let doc = "a\n  b\n    x 1\n  c\n    y 2\n  b\n    z 3\nlist\n  - k 1\n    sub\n      d 4\n  - k 2\n    e 5\n  - plain\ntext |\n  one\n  two\nafter 1\na 9\n";
    let json = static_json(doc);
    assert!(json.contains("\"after\":1"), "{json}");
}

#[test]
fn parser_depth_cap_and_long_keys() {
    let mut doc = String::new();
    for i in 0..140 {
        doc.push_str(&" ".repeat(i));
        doc.push_str(&format!("k{i}\n"));
    }
    doc.push_str(&format!("{}\n  a\n    x 1\n  a\n    x 2\n", "K".repeat(300)));
    let _ = static_json(&doc);
}

#[test]
fn parser_lists_inside_lists_and_empty_keys() {
    let doc = "servers\n  - name a\n    tags\n      - x\n      - y\n  - name b\n    tags\n  - name c\n    port 1\nl:join\n  z\n  - item\n";
    let _ = static_json(doc);
}

#[test]
fn engine_reads_the_root_while_an_object_is_borrowed() {
    // Every marker below looks up another key of the document — in the same
    // object, in the root, or in an ancestor — while its own object is being
    // rewritten.
    let doc = "!active\nbase 10\nname world\ngroup\n  hp 5\n  double:calc hp * 2 + base\n  path:calc group.hp + 1\n  copy:alias base\n  up:ref group\n  msg hello {name} {group.hp}\n  pick:map:base\n    - 10 ten\n    - 20 twenty\n  n:i18n:hp\n    en\n      one {count} item\n      other {count} items\nwhole:alias group\nsorted:sort\n  - 3\n  - 1\n  - b\ntoken:secret abc\nlate {token} {whole.hp}\nchild:inherit:group\n  extra 1\n";
    let json = active_json(doc);
    assert!(json.contains("\"double\":20"), "{json}");
}

#[test]
fn engine_list_items_and_nested_markers() {
    let doc = "!active\nport 7\nitems\n  - id 1\n    p:calc port + 1\n    t {port}\n  - id 2\n    q:alias items\ndeep\n  a\n    b\n      c:ref port\n      d:calc deep.a.b.c * 2\n";
    let _ = active_json(doc);
}

#[test]
fn binary_round_trip() {
    let r = parse("!active\na:env:default:1 V\nb\n  - 1\n  - x\nc |\n  t\n");
    let bytes = synx_core::binary::compile(&r, false);
    let back = synx_core::binary::decompile(&bytes).expect("own container");
    assert_eq!(to_json(&back.root), to_json(&r.root));
    assert!(synx_core::binary::decompile(&bytes[..bytes.len() - 3]).is_err());
}
