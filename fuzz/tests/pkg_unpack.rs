//! `synx install` unpacks a registry archive with `pkg_guard::unpack_archive`.
//! Archives built byte by byte — GNU long names, pax records, odd entry types,
//! names Windows rewrites — must either unpack inside the destination or be
//! refused, and nothing outside it may change.
//!
//!   cargo test --release --manifest-path fuzz/Cargo.toml --test pkg_unpack

#[path = "../../crates/synx-cli/src/pkg_guard.rs"]
#[allow(dead_code)]
mod pkg_guard;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synx-unpack-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 512-byte ustar/GNU header with the name and link written as raw bytes.
fn header(name: &[u8], kind: u8, size: u64, link: &[u8]) -> [u8; 512] {
    let mut h = [0u8; 512];
    h[..name.len().min(100)].copy_from_slice(&name[..name.len().min(100)]);
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    let sz = format!("{:011o}\0", size);
    h[124..136].copy_from_slice(sz.as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[156] = kind;
    h[157..157 + link.len().min(100)].copy_from_slice(&link[..link.len().min(100)]);
    h[257..265].copy_from_slice(b"ustar  \0");
    // checksum: spaces while summing
    for b in &mut h[148..156] {
        *b = b' ';
    }
    let sum: u32 = h.iter().map(|&b| b as u32).sum();
    let ck = format!("{:06o}\0 ", sum);
    h[148..156].copy_from_slice(ck.as_bytes());
    h
}

fn padded(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    while v.len() % 512 != 0 {
        v.push(0);
    }
    v
}

/// One entry: header + data blocks.
fn entry(name: &[u8], kind: u8, data: &[u8], link: &[u8]) -> Vec<u8> {
    let mut v = header(name, kind, data.len() as u64, link).to_vec();
    v.extend(padded(data));
    v
}

fn pax_record(key: &str, value: &[u8]) -> Vec<u8> {
    // "<len> key=value\n", where len counts itself.
    let body_len = 1 + key.len() + 1 + value.len() + 1;
    let mut len = body_len + 1;
    loop {
        let total = len.to_string().len() + body_len;
        if total == len {
            break;
        }
        len = total;
    }
    let mut v = format!("{} {}=", len, key).into_bytes();
    v.extend_from_slice(value);
    v.push(b'\n');
    v
}

fn gz(tar: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(tar).unwrap();
    e.write_all(&[0u8; 1024]).unwrap();
    e.finish().unwrap()
}

/// Unpack `archive` into a fresh `dest` inside `root`; returns the result and
/// every path under `root` that is not under `dest` and not the victim file.
fn attempt(label: &str, archive: &[u8]) -> (Result<pkg_guard::Unpacked, String>, Vec<String>) {
    let root = scratch(label);
    let dest = root.join("staging").join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("victim.txt"), "untouched").unwrap();
    let got = pkg_guard::unpack_archive(archive, &dest);
    let mut stray = Vec::new();
    walk(&root, &dest, &mut stray);
    let victim = fs::read_to_string(root.join("victim.txt")).unwrap_or_default();
    if victim != "untouched" {
        stray.push(format!("victim.txt now {:?}", victim));
    }
    let _ = fs::remove_dir_all(&root);
    (got, stray)
}

fn walk(dir: &Path, dest: &Path, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.starts_with(dest) {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "staging" {
                out.push(p.display().to_string());
            }
            walk(&p, dest, out);
        } else if name != "victim.txt" {
            out.push(p.display().to_string());
        }
    }
}

fn cases() -> Vec<(&'static str, Vec<u8>, bool)> {
    let mut v: Vec<(&'static str, Vec<u8>, bool)> = Vec::new();
    // (label, tar bytes, may it succeed)
    let gnu_long = |name: &[u8]| {
        let mut t = entry(b"././@LongLink", b'L', &[name, b"\0"].concat(), b"");
        t.extend(entry(b"innocent.txt", b'0', b"pwned", b""));
        t
    };
    v.push(("gnu long name ../", gnu_long(b"../victim.txt"), false));
    v.push(("gnu long name ../../", gnu_long(b"a/../../victim.txt"), false));
    v.push(("gnu long name absolute", gnu_long(b"/tmp/synx-unpack-abs"), false));
    v.push(("gnu long name drive", gnu_long(b"C:/synx-unpack-abs.txt"), false));
    v.push(("gnu long name backslash", gnu_long(b"..\\victim.txt"), false));
    v.push(("gnu long name UNC", gnu_long(b"\\\\?\\C:\\synx-unpack-abs.txt"), false));
    v.push(("gnu long name stream", gnu_long(b"x.synx:$DATA"), false));
    v.push(("gnu long name device", gnu_long(b"sub/CON.synx"), false));
    v.push(("gnu long name trailing space", gnu_long(b"sub /x.synx"), false));
    v.push(("gnu long name 200 a's", gnu_long(&[b'a'; 200]), true));
    v.push(("gnu long name NUL inside", gnu_long(b"a\0../../victim.txt"), true));

    let pax = |key: &str, value: &[u8], kind: u8| {
        let rec = pax_record(key, value);
        let mut t = entry(b"PaxHeader/x", b'x', &rec, b"");
        t.extend(entry(b"innocent.txt", kind, b"pwned", b""));
        t
    };
    v.push(("pax path ../", pax("path", b"../victim.txt", b'0'), false));
    v.push(("pax path ../../", pax("path", b"x/../../victim.txt", b'0'), false));
    v.push(("pax path absolute", pax("path", b"/tmp/synx-unpack-abs", b'0'), false));
    v.push(("pax path drive relative", pax("path", b"C:victim.txt", b'0'), false));
    v.push(("pax path verbatim", pax("path", b"\\\\?\\GLOBALROOT\\x", b'0'), false));
    v.push(("pax linkpath on a file", pax("linkpath", b"../victim.txt", b'0'), true));
    v.push(("pax size larger", pax("size", b"99999999999", b'0'), false));
    v.push(("pax size zero", pax("size", b"0", b'0'), true));
    v.push(("pax path on a symlink", pax("path", b"ok.txt", b'2'), false));
    let mut gpax = entry(b"g", b'g', &pax_record("path", b"../victim.txt"), b"");
    gpax.extend(entry(b"innocent.txt", b'0', b"pwned", b""));
    v.push(("global pax path", gpax, true));

    for (label, kind) in [("gnu sparse", b'S'), ("volume label", b'V'), ("multivolume", b'M'), ("dumpdir", b'D'), ("old long name N", b'N'), ("long link K", b'K'), ("block device", b'4'), ("contiguous", b'7'), ("unknown Z", b'Z')] {
        v.push((label, entry(b"thing.txt", kind, b"pwned", b"../victim.txt"), kind == b'7'));
    }
    let mut dir_then_file = entry(b"x/", b'5', b"", b"");
    dir_then_file.extend(entry(b"X", b'0', b"pwned", b""));
    v.push(("dir then same name as file, other case", dir_then_file, false));
    let mut case_dup = entry(b"A.synx", b'0', b"1", b"");
    case_dup.extend(entry(b"a.synx", b'0', b"2", b""));
    v.push(("two files differing in case", case_dup, false));
    let mut short = entry(b"verylongdirectoryname/", b'5', b"", b"");
    short.extend(entry(b"VERYLO~1/x.synx", b'0', b"pwned", b""));
    v.push(("8.3 short name of a directory just made", short, true));
    let mut dots = entry(b"a/./b/./c.synx", b'0', b"ok", b"");
    dots.extend(entry(b"./d.synx", b'0', b"ok", b""));
    v.push(("current-dir components", dots, true));
    v.push(("directory with a link name", entry(b"d/", b'5', b"", b"../.."), true));
    v.push(("hard link to an earlier file", { let mut t = entry(b"a.txt", b'0', b"x", b""); t.extend(entry(b"b.txt", b'1', b"", b"a.txt")); t }, false));
    v.push(("unicode RTL override in a name", entry("gnp\u{202e}.synx".as_bytes(), b'0', b"x", b""), true));
    v.push(("fullwidth solidus", entry("a\u{ff0f}..\u{ff0f}x".as_bytes(), b'0', b"x", b""), true));
    v.push(("size field beyond the data", { let mut t = header(b"big.bin", b'0', 1 << 30, b"").to_vec(); t.extend([0u8; 512]); t }, false));
    v
}

#[test]
fn hostile_archives_stay_inside_their_directory() {
    let mut problems = Vec::new();
    for (label, tar, may_succeed) in cases() {
        let (got, stray) = attempt(&label.replace(|c: char| !c.is_ascii_alphanumeric(), "_"), &gz(&tar));
        if !stray.is_empty() {
            problems.push(format!("{}: wrote outside the destination: {:?}", label, stray));
        }
        if got.is_ok() && !may_succeed {
            problems.push(format!("{}: unpacked {:?}", label, got));
        }
        println!("{:45} {:?}", label, got.as_ref().map(|u| (u.files, u.bytes)));
    }
    assert!(!std::path::Path::new("/tmp/synx-unpack-abs").exists());
    assert!(!std::path::Path::new("C:/synx-unpack-abs.txt").exists());
    assert!(problems.is_empty(), "{:#?}", problems);
}

#[test]
fn bombs_stop_at_the_caps() {
    // 70 MiB of zeros in one file entry: the stream cap ends it.
    let mut t = header(b"zeros.bin", b'0', 70 << 20, b"").to_vec();
    t.extend(vec![0u8; 70 << 20]);
    let started = std::time::Instant::now();
    let (got, stray) = attempt("bomb", &gz(&t));
    assert!(got.is_err() && stray.is_empty(), "{:?} {:?}", got, stray);
    assert!(started.elapsed().as_secs() < 20);
    // A GNU long name of 60 MiB: read into memory by the tar reader, under the cap.
    let mut t = entry(b"././@LongLink", b'L', &vec![b'a'; 60 << 20], b"");
    t.extend(entry(b"x", b'0', b"x", b""));
    let (got, stray) = attempt("longname-bomb", &gz(&t));
    assert!(got.is_err() && stray.is_empty(), "{:?} {:?}", got, stray);
    // 4096 directory entries, each 32 levels deep.
    let mut t = Vec::new();
    for i in 0..4096 {
        let name = format!("{}/{}", (0..31).map(|_| "d").collect::<Vec<_>>().join("/"), i);
        let mut gnu = entry(b"././@LongLink", b'L', &[name.as_bytes(), b"/\0"].concat(), b"");
        gnu.extend(entry(b"x/", b'5', b"", b""));
        t.extend(gnu);
    }
    let started = std::time::Instant::now();
    let (got, stray) = attempt("dirs", &gz(&t));
    println!("4096 deep directories: {:?} in {:?}", got.as_ref().map(|u| u.files), started.elapsed());
    assert!(stray.is_empty());
}
