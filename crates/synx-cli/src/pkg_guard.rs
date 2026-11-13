//! Filesystem guards for package management: package names, archive entry
//! paths, bounded extraction, and directory swaps that never leave
//! `synx_packages/`.
//!
//! Everything here judges input that comes from somewhere else — a registry
//! response, a downloaded archive, a manifest in a cloned repository, a
//! `synx.lock` committed by someone — before it reaches `remove_dir_all`,
//! `create_dir_all` or a file write.

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const PACKAGES_DIR: &str = "synx_packages";

/// Bytes one package may unpack to. The compressed cap is 5 MiB; gzip turns
/// 5 MiB of zeros into about 5 GiB, so the inflated size needs its own bound.
pub const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024;
/// Entries one package archive may hold.
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;
/// Directory levels an archive path or a copied tree may have.
const MAX_PATH_DEPTH: usize = 32;
/// Bytes in one path component (the limit of NTFS, ext4 and APFS).
const MAX_COMPONENT_BYTES: usize = 255;
/// Longest scope or package name.
const MAX_NAME_PART: usize = 64;

/// Whether Windows opens a device for `name` whatever directory it sits in:
/// `NUL`, `con.txt`, `COM1`, `lpt9.log`.
fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = stem.to_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let mut chars = upper.chars();
    let head: String = chars.by_ref().take(3).collect();
    let (digit, rest) = (chars.next(), chars.next());
    (head == "COM" || head == "LPT")
        && rest.is_none()
        && digit.map_or(false, |d| d.is_ascii_digit() || matches!(d, '\u{b9}' | '\u{b2}' | '\u{b3}'))
}

fn is_safe_name_part(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME_PART
        && bytes[0].is_ascii_alphanumeric()
        && !s.ends_with('.')
        && !s.contains("..")
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !is_windows_device_name(s)
}

/// Accepts `name` or `@scope/name` whose parts are plain directory names.
///
/// safety: the name becomes `synx_packages/<name>`, and that directory is
/// removed before the new copy lands. `../keepme` left the packages
/// directory; on Windows `...` and `@x/...` name the parent directory itself,
/// so every part starts with a letter or digit and never ends in a dot.
pub fn validate_package_name(name: &str) -> Result<(), String> {
    let (scope, short) = match name.split_once('/') {
        Some((scope, short)) => (Some(scope), short),
        None => (None, name),
    };
    let is_scope_safe = scope.map_or(true, |s| s.strip_prefix('@').map_or(false, is_safe_name_part));
    if is_scope_safe && is_safe_name_part(short) {
        return Ok(());
    }
    Err(format!(
        "invalid package name {:?}: use @scope/name — letters, digits, '.', '_' and '-', \
         each part starting with a letter or digit",
        name
    ))
}

/// Accepts a scoped registry name: `@scope/name`.
pub fn validate_registry_name(name: &str) -> Result<(), String> {
    if !name.starts_with('@') || !name.contains('/') {
        return Err(format!("invalid package name {:?}: must be @scope/name", name));
    }
    validate_package_name(name)
}

/// Accepts an exact version: `1.2.3`, `1.2.3-rc.1`, `1.2.3+build.5`.
///
/// safety: the version is written into `synx.lock` on its own line, becomes a
/// URL segment and part of the archive file name; a line break forged lock
/// entries and `../` moved the archive out of the package directory.
pub fn validate_version(version: &str) -> Result<(), String> {
    let bytes = version.as_bytes();
    let is_plain = !bytes.is_empty()
        && bytes.len() <= MAX_NAME_PART
        && bytes[0].is_ascii_digit()
        && !version.contains("..")
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    if is_plain {
        Ok(())
    } else {
        Err(format!("invalid version {:?}: use an exact version such as 1.2.3", version))
    }
}

/// Checks one component of a path that will be created on disk.
fn check_component(name: &str) -> Result<(), String> {
    let is_bad_char =
        |c: char| c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
    if name.is_empty() || name.len() > MAX_COMPONENT_BYTES {
        return Err("a name that is empty or longer than 255 bytes".into());
    }
    if name.chars().any(is_bad_char) {
        return Err("a character that is not portable in file names".into());
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("a name ending in a dot or a space".into());
    }
    if is_windows_device_name(name) {
        return Err("a Windows device name".into());
    }
    Ok(())
}

/// The relative path an archive entry (or a manifest `files` / `main` line)
/// may be created at: plain components only.
///
/// safety: refuses `..`, a root, a drive or UNC prefix, a backslash on any
/// platform (a separator once the package reaches Windows), alternate data
/// streams (`:`), device names and names Windows rewrites (trailing dot or
/// space) — each of them lands the file somewhere other than where the path
/// reads.
pub fn contained_rel_path(path: &Path) -> Result<PathBuf, String> {
    let shown = path.to_string_lossy();
    let refuse = |why: &str| Err(format!("unsafe path {:?}: {}", shown, why));
    let mut rel = PathBuf::new();
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(os) => {
                let Some(name) = os.to_str() else {
                    return refuse("a name that is not valid UTF-8");
                };
                if let Err(why) = check_component(name) {
                    return refuse(&why);
                }
                depth += 1;
                if depth > MAX_PATH_DEPTH {
                    return refuse("nested deeper than 32 directories");
                }
                rel.push(name);
            }
            Component::ParentDir => return refuse("`..` is not allowed"),
            Component::RootDir | Component::Prefix(_) => return refuse("absolute paths are not allowed"),
        }
    }
    Ok(rel)
}

/// What [`unpack_archive`] wrote.
#[derive(Debug, PartialEq, Eq)]
pub struct Unpacked {
    pub files: usize,
    pub bytes: u64,
}

/// Unpacks a gzipped tar into the directory `dest`, which must be empty.
///
/// Only directories and regular files are created, each at a path
/// [`contained_rel_path`] accepts. A link, device, FIFO or sparse entry
/// refuses the whole archive: a package has no use for them, and a link is how
/// an archive writes or later reads outside its directory. File modes in the
/// archive are ignored.
///
/// safety: `tar::Archive::unpack` created symlinks as written and inflated
/// without a bound. The inflated stream is capped here as a whole — long-name
/// and pax headers are read into memory by the tar reader and are not part of
/// any file size.
pub fn unpack_archive(gz: &[u8], dest: &Path) -> Result<Unpacked, String> {
    let stream_cap = MAX_UNPACKED_BYTES + (MAX_ARCHIVE_ENTRIES as u64) * 2048;
    let inflated = flate2::read::GzDecoder::new(gz).take(stream_cap);
    let mut archive = tar::Archive::new(inflated);
    let mut unpacked = Unpacked { files: 0, bytes: 0 };
    let mut seen = 0usize;

    let entries = archive.entries().map_err(|e| format!("cannot read archive: {}", e))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("corrupt or oversized archive: {}", e))?;
        seen += 1;
        if seen > MAX_ARCHIVE_ENTRIES {
            return Err(format!("archive holds more than {} entries", MAX_ARCHIVE_ENTRIES));
        }
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        let raw = entry.path().map_err(|e| format!("corrupt archive path: {}", e))?.into_owned();
        let rel = contained_rel_path(&raw)?;

        if kind.is_dir() {
            if !rel.as_os_str().is_empty() {
                let dir = dest.join(&rel);
                fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
            }
            continue;
        }
        if !kind.is_file() && !kind.is_contiguous() {
            return Err(format!(
                "archive entry {:?} is not a regular file (links, devices and sparse files are refused)",
                raw.to_string_lossy()
            ));
        }
        if rel.as_os_str().is_empty() {
            return Err("archive holds a file with an empty name".into());
        }
        let size = entry.size();
        unpacked.bytes = unpacked.bytes.saturating_add(size);
        if unpacked.bytes > MAX_UNPACKED_BYTES {
            return Err(format!("archive unpacks to more than {} bytes", MAX_UNPACKED_BYTES));
        }
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
        }
        // safety: `create_new` never writes through something already there —
        // a second entry with the same path is an error, not an overwrite.
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| format!("cannot write {:?}: {}", rel.to_string_lossy(), e))?;
        let copied = io::copy(&mut (&mut entry).take(size), &mut out)
            .map_err(|e| format!("cannot unpack {:?}: {}", rel.to_string_lossy(), e))?;
        if copied != size {
            return Err(format!("archive ends inside {:?}", rel.to_string_lossy()));
        }
        unpacked.files += 1;
    }
    Ok(unpacked)
}

/// The real path of `<project>/synx_packages`, which must lie inside the project.
///
/// safety: a repository can ship `synx_packages` as a link to anywhere;
/// installing through it would delete and write outside the project.
pub fn packages_root(project_root: &Path, should_create: bool) -> Result<PathBuf, String> {
    let root = fs::canonicalize(project_root)
        .map_err(|e| format!("cannot resolve {}: {}", project_root.display(), e))?;
    let dir = root.join(PACKAGES_DIR);
    if should_create {
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    }
    let real = fs::canonicalize(&dir).map_err(|e| format!("cannot resolve {}: {}", dir.display(), e))?;
    if !real.starts_with(&root) {
        return Err(format!(
            "{} leads outside the project ({}); refusing to manage packages through a link",
            dir.display(),
            real.display()
        ));
    }
    Ok(real)
}

/// The directory of package `name` under `packages`, a path from
/// [`packages_root`]. A scope directory that is a link is refused; the package
/// directory itself may be one (a linked local package).
pub fn package_dir(packages: &Path, name: &str) -> Result<PathBuf, String> {
    validate_package_name(name)?;
    let mut dir = packages.to_path_buf();
    let (scope, short) = match name.split_once('/') {
        Some((scope, short)) => (Some(scope), short),
        None => (None, name),
    };
    if let Some(scope) = scope {
        dir.push(scope);
        if let Ok(meta) = fs::symlink_metadata(&dir) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(format!(
                    "{} is a link or a file, not a directory; refusing to manage packages through it",
                    dir.display()
                ));
            }
        }
    }
    dir.push(short);
    Ok(dir)
}

static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// A fresh empty directory under `<packages>/.staging/`, on the same
/// filesystem as the package it will replace.
pub fn new_staging_dir(packages: &Path) -> Result<PathBuf, String> {
    let base = packages.join(".staging");
    fs::create_dir_all(&base).map_err(|e| format!("cannot create {}: {}", base.display(), e))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    for _ in 0..64 {
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("{}-{}-{}", std::process::id(), nanos, seq));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot create {}: {}", dir.display(), e)),
        }
    }
    Err(format!("cannot create a staging directory in {}", base.display()))
}

/// Removes `<packages>/.staging` once nothing is left in it.
pub fn remove_staging_root(packages: &Path) {
    let _ = fs::remove_dir(packages.join(".staging"));
}

/// Removes the link at `path` and nothing it points at.
fn remove_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

/// Removes an installed package: a link is unlinked, a directory is deleted.
pub fn remove_package_dir(pkg_dir: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(pkg_dir)
        .map_err(|e| format!("cannot read {}: {}", pkg_dir.display(), e))?;
    let removed = if meta.file_type().is_symlink() {
        remove_link(pkg_dir)
    } else {
        fs::remove_dir_all(pkg_dir)
    };
    removed.map_err(|e| format!("cannot remove {}: {}", pkg_dir.display(), e))
}

/// Puts the staged directory in place of `pkg_dir`.
///
/// why: the old copy is moved aside first and deleted last, so a failed
/// install leaves the package that was there. A linked package is unlinked —
/// what it points at is never touched.
pub fn swap_into_place(staged: &Path, pkg_dir: &Path) -> Result<(), String> {
    if let Some(parent) = pkg_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    let mut moved_aside: Option<PathBuf> = None;
    if let Ok(meta) = fs::symlink_metadata(pkg_dir) {
        if meta.file_type().is_symlink() {
            remove_link(pkg_dir).map_err(|e| format!("cannot unlink {}: {}", pkg_dir.display(), e))?;
        } else {
            let aside = staged.with_extension("old");
            fs::rename(pkg_dir, &aside).map_err(|e| {
                format!("cannot replace {} (is a file in it open?): {}", pkg_dir.display(), e)
            })?;
            moved_aside = Some(aside);
        }
    }
    if let Err(e) = fs::rename(staged, pkg_dir) {
        if let Some(aside) = &moved_aside {
            let _ = fs::rename(aside, pkg_dir);
        }
        return Err(format!("cannot move the package into {}: {}", pkg_dir.display(), e));
    }
    if let Some(aside) = moved_aside {
        let _ = fs::remove_dir_all(aside);
    }
    Ok(())
}

/// Directory names never copied or packed: version control, dependency and
/// build trees, and the places keys live.
pub fn is_service_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | ".hg" | "node_modules" | "target" | ".ssh" | ".aws" | ".gnupg" | ".staging"
    )
}

/// Whether a file name is one that usually holds a credential.
///
/// `.env.example`, `.env.sample` and `.env.template` are not: they exist to be
/// published.
pub fn is_secret_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == ".env" {
        return true;
    }
    if let Some(rest) = lower.strip_prefix(".env.") {
        return !matches!(rest, "example" | "sample" | "template" | "dist");
    }
    // `prod.env`, `local.env`, `app.env.production`: the same file under the
    // other naming habit — the rule of the MCP sandbox (`sandbox.js isSecretPath`).
    if let Some(at) = lower.find(".env") {
        let after = &lower[at + 4..];
        if after.is_empty() {
            return true;
        }
        if let Some(rest) = after.strip_prefix('.') {
            return !matches!(rest, "example" | "sample" | "template" | "dist");
        }
    }
    // `id_rsa_github`, `id_ed25519.backup`: private keys under a suffix.
    if ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].iter().any(|k| lower.starts_with(k) && !lower.ends_with(".pub")) {
        return true;
    }
    if matches!(
        lower.as_str(),
        ".npmrc" | ".pypirc" | ".netrc" | "_netrc" | ".htpasswd" | "credentials.toml" | "signing-key.hex"
            | ".synx.lock" | ".envrc" | ".pgpass" | ".my.cnf" | ".git-credentials" | ".dockercfg" | "credentials"
            | "kubeconfig"
    ) {
        return true;
    }
    [".pem", ".key", ".p12", ".pfx", ".p8", ".jks", ".keystore", ".kdbx"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Text from a package manifest or a registry answer, made safe to print.
///
/// safety: a manifest field or a registry description reaches the terminal of
/// whoever lists, inspects or searches packages; an escape sequence in it can
/// erase the line, set the window title or write the clipboard (OSC 52).
/// Control characters (C0, DEL, C1), the line and paragraph separators and the
/// bidirectional overrides are written as `\u{…}`.
pub fn terminal_safe(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let is_hidden = c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        if is_hidden {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// Copies the package tree `src` into the empty directory `dst`.
///
/// Links are not followed and not copied; `skip_dir` (the project's own
/// packages directory) is never entered, so installing a package from the
/// project root does not copy the tree into itself.
pub fn copy_package_tree(src: &Path, dst: &Path, skip_dir: &Path, depth: usize) -> Result<(), String> {
    if depth > MAX_PATH_DEPTH {
        return Err(format!("{} is nested deeper than {} directories", src.display(), MAX_PATH_DEPTH));
    }
    fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {}", dst.display(), e))?;
    let entries = fs::read_dir(src).map_err(|e| format!("cannot read {}: {}", src.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {}", src.display(), e))?;
        let path = entry.path();
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        if kind.is_symlink() {
            eprintln!("  skipped link: {}", path.display());
            continue;
        }
        if kind.is_dir() {
            let is_destination = fs::canonicalize(&path).map_or(false, |real| real == skip_dir);
            if is_service_dir(&name) || is_destination {
                continue;
            }
            copy_package_tree(&path, &dst.join(&name_os), skip_dir, depth + 1)?;
        } else if kind.is_file() {
            if name.ends_with(".tar.gz") {
                continue;
            }
            let dest = dst.join(&name_os);
            fs::copy(&path, &dest)
                .map_err(|e| format!("cannot copy {} → {}: {}", path.display(), dest.display(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printed_manifest_text_carries_no_terminal_escape() {
        let shown = terminal_safe("1.0.0\u{1b}[2K\u{1b}[1G  @trusted/core@9.9.9 ok\u{1b}]0;title\u{7}\u{9b}x\u{2028}\u{202e}");
        assert!(!shown.chars().any(|c| c.is_control() || c == '\u{2028}' || c == '\u{202e}'), "{}", shown);
        assert!(shown.starts_with("1.0.0\\u{1b}[2K"), "{}", shown);
        assert_eq!(terminal_safe("@scope/pkg — описание"), "@scope/pkg — описание");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("synx-guard-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A directory link: a symlink on Unix, a junction on Windows (no privilege needed).
    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .map_or(false, |o| o.status.success())
        }
    }

    fn gz_tar(build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>)) -> Vec<u8> {
        let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(enc);
        build(&mut builder);
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// An entry whose path bytes are written as given — `tar::Builder` refuses `..` itself.
    fn raw_entry(
        builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
        path: &str,
        kind: tar::EntryType,
        link: Option<&str>,
        data: &[u8],
    ) {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
        if let Some(link) = link {
            header.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
        }
        header.set_entry_type(kind);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, data).unwrap();
    }

    #[test]
    fn package_names_stay_inside_the_packages_directory() {
        for good in ["@assynx/text-tools", "text-tools", "Foo.Bar_1", "@my.scope/pkg", "@a/b", "a1"] {
            assert!(validate_package_name(good).is_ok(), "{}", good);
        }
        for bad in [
            "", "..", "...", "....", "@x/...", "@../x", "a/..", "../keepme", "@a/../../x", ".foo", "a.",
            "C:/w", "C:\\w", "a\\b", "@x/y/z", "@/x", "@x/", "x/y", "@x y/z", "@x/y\n@evil/z", "nul",
            "@con/x", "@x/COM1", "@x/lpt9.txt", "@x/-rf", "@x/a..b",
        ] {
            assert!(validate_package_name(bad).is_err(), "{:?} must be refused", bad);
        }
        assert!(validate_registry_name("text-tools").is_err());
        assert!(validate_registry_name("@assynx/text-tools").is_ok());
    }

    #[test]
    fn versions_are_single_plain_tokens() {
        for good in ["1.0.0", "0.1.2-rc.1", "10.20.30+build.5", "1.0.0-alpha"] {
            assert!(validate_version(good).is_ok(), "{}", good);
        }
        for bad in ["", "^1.0.0", "1.0.0\n@evil/pkg", "1.0/../../x", "../1", "v1.0.0", "1..0", "1.0.0 x"] {
            assert!(validate_version(bad).is_err(), "{:?} must be refused", bad);
        }
    }

    #[test]
    fn archive_paths_that_leave_the_directory_are_refused() {
        for good in ["src/main.synx", "./README.md", "a/b/c.txt", "файл.synx", "synx-pkg.synx"] {
            assert!(contained_rel_path(Path::new(good)).is_ok(), "{}", good);
        }
        for bad in [
            "../x", "a/../../x", "/etc/passwd", "a/b:stream", "con", "NUL", "nul.txt", "a/COM1", "a/lpt1.log",
            "trailing.", "trailing ", "a/b.", "x\u{7}y", "a*b", "q?", "\\\\host\\share\\x", "a\\..\\..\\x",
            "CONOUT$", "com\u{b9}",
        ] {
            assert!(contained_rel_path(Path::new(bad)).is_err(), "{:?} must be refused", bad);
        }
        #[cfg(windows)]
        for bad in ["C:\\Windows\\x", "C:x", "\\rooted"] {
            assert!(contained_rel_path(Path::new(bad)).is_err(), "{:?} must be refused", bad);
        }
    }

    #[test]
    fn unpack_writes_plain_files_only_inside_dest() {
        let dir = scratch("unpack-ok");
        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        let gz = gz_tar(|b| {
            raw_entry(b, "synx-pkg.synx", tar::EntryType::Regular, None, b"name @t/p\nversion 1.0.0\n");
            raw_entry(b, "src/", tar::EntryType::Directory, None, b"");
            raw_entry(b, "src/main.synx", tar::EntryType::Regular, None, b"k v\n");
        });
        let got = unpack_archive(&gz, &dest).unwrap();
        assert_eq!(got, Unpacked { files: 2, bytes: 24 + 4 });
        assert_eq!(fs::read_to_string(dest.join("src/main.synx")).unwrap(), "k v\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_refuses_traversal_links_devices_and_duplicates() {
        let dir = scratch("unpack-bad");
        fs::write(dir.join("outside.txt"), "untouched").unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("dotdot", gz_tar(|b| raw_entry(b, "../outside.txt", tar::EntryType::Regular, None, b"x"))),
            ("deep dotdot", gz_tar(|b| raw_entry(b, "a/../../outside.txt", tar::EntryType::Regular, None, b"x"))),
            ("absolute", gz_tar(|b| raw_entry(b, "/tmp/synx-abs-probe", tar::EntryType::Regular, None, b"x"))),
            ("backslash", gz_tar(|b| raw_entry(b, "a\\..\\..\\outside.txt", tar::EntryType::Regular, None, b"x"))),
            ("symlink", gz_tar(|b| raw_entry(b, "link", tar::EntryType::Symlink, Some("../outside.txt"), b""))),
            ("hardlink", gz_tar(|b| raw_entry(b, "hard", tar::EntryType::Link, Some("../outside.txt"), b""))),
            ("fifo", gz_tar(|b| raw_entry(b, "pipe", tar::EntryType::Fifo, None, b""))),
            ("chardev", gz_tar(|b| raw_entry(b, "dev", tar::EntryType::Char, None, b""))),
            ("device name", gz_tar(|b| raw_entry(b, "src/NUL", tar::EntryType::Regular, None, b"x"))),
            ("trailing dot", gz_tar(|b| raw_entry(b, "src/x.", tar::EntryType::Regular, None, b"x"))),
            ("stream", gz_tar(|b| raw_entry(b, "a.synx:hidden", tar::EntryType::Regular, None, b"x"))),
            (
                "duplicate",
                gz_tar(|b| {
                    raw_entry(b, "a.synx", tar::EntryType::Regular, None, b"1");
                    raw_entry(b, "a.synx", tar::EntryType::Regular, None, b"2");
                }),
            ),
        ];
        for (label, gz) in cases {
            let dest = dir.join(format!("out-{}", label.replace(' ', "-")));
            fs::create_dir_all(&dest).unwrap();
            let got = unpack_archive(&gz, &dest);
            assert!(got.is_err(), "{} must be refused, got {:?}", label, got);
        }
        assert_eq!(fs::read_to_string(dir.join("outside.txt")).unwrap(), "untouched");
        assert!(!Path::new("/tmp/synx-abs-probe").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_stops_at_the_size_and_entry_caps() {
        let dir = scratch("unpack-bomb");
        // A header that claims more than the cap is refused before a byte is written.
        let mut header = tar::Header::new_gnu();
        header.set_path("big.bin").unwrap();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(MAX_UNPACKED_BYTES + 1);
        header.set_cksum();
        let mut tar_bytes = header.as_bytes().to_vec();
        tar_bytes.extend(std::iter::repeat(0u8).take(4096));
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &tar_bytes).unwrap();
        let gz = enc.finish().unwrap();
        let dest = dir.join("a");
        fs::create_dir_all(&dest).unwrap();
        let err = unpack_archive(&gz, &dest).unwrap_err();
        assert!(err.contains("unpacks to more than"), "{}", err);
        assert!(!dest.join("big.bin").exists());

        let gz = gz_tar(|b| {
            for i in 0..=MAX_ARCHIVE_ENTRIES {
                raw_entry(b, &format!("f{}", i), tar::EntryType::Regular, None, b"");
            }
        });
        let dest = dir.join("b");
        fs::create_dir_all(&dest).unwrap();
        let err = unpack_archive(&gz, &dest).unwrap_err();
        assert!(err.contains("more than"), "{}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_linked_packages_directory_is_refused() {
        let dir = scratch("linked-root");
        let project = dir.join("project");
        let outside = dir.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        if !link_dir(&outside, &project.join(PACKAGES_DIR)) {
            eprintln!("SKIP: cannot create a directory link here");
            return;
        }
        let err = packages_root(&project, true).unwrap_err();
        assert!(err.contains("outside the project"), "{}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_linked_scope_directory_is_refused_and_a_linked_package_is_only_unlinked() {
        let dir = scratch("linked-scope");
        let project = dir.join("project");
        let outside = dir.join("outside");
        fs::create_dir_all(outside.join("pkg")).unwrap();
        fs::write(outside.join("pkg/keep.txt"), "keep").unwrap();
        fs::create_dir_all(&project).unwrap();
        let packages = packages_root(&project, true).unwrap();
        // Links are made through the plain path: `mklink` does not take the
        // `\\?\` form `canonicalize` returns on Windows.
        let plain = project.join(PACKAGES_DIR);
        if !link_dir(&outside, &plain.join("@evil")) {
            eprintln!("SKIP: cannot create a directory link here");
            return;
        }
        let err = package_dir(&packages, "@evil/pkg").unwrap_err();
        assert!(err.contains("link"), "{}", err);

        // A package directory that is itself a link: replaced without touching its target.
        fs::create_dir_all(plain.join("@ok")).unwrap();
        assert!(link_dir(&outside.join("pkg"), &plain.join("@ok").join("linked")));
        let pkg_dir = package_dir(&packages, "@ok/linked").unwrap();
        let staged = new_staging_dir(&packages).unwrap();
        fs::write(staged.join("new.txt"), "new").unwrap();
        swap_into_place(&staged, &pkg_dir).unwrap();
        assert_eq!(fs::read_to_string(pkg_dir.join("new.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(outside.join("pkg/keep.txt")).unwrap(), "keep");

        assert!(link_dir(&outside.join("pkg"), &plain.join("@ok").join("again")));
        remove_package_dir(&packages.join("@ok/again")).unwrap();
        assert_eq!(fs::read_to_string(outside.join("pkg/keep.txt")).unwrap(), "keep");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_swap_keeps_the_installed_package() {
        let dir = scratch("swap");
        let packages = packages_root(&dir, true).unwrap();
        let pkg_dir = package_dir(&packages, "@a/b").unwrap();
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("old.txt"), "old").unwrap();
        // The staged directory does not exist, so the second rename fails.
        let missing = packages.join(".staging").join("never-created");
        fs::create_dir_all(packages.join(".staging")).unwrap();
        assert!(swap_into_place(&missing, &pkg_dir).is_err());
        assert_eq!(fs::read_to_string(pkg_dir.join("old.txt")).unwrap(), "old");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copying_a_tree_skips_links_and_its_own_destination() {
        let dir = scratch("copy");
        let project = dir.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/main.synx"), "k v\n").unwrap();
        fs::write(project.join("synx-pkg.synx"), "name @a/b\nversion 1.0.0\n").unwrap();
        fs::create_dir_all(project.join(".git")).unwrap();
        fs::write(project.join(".git/config"), "secret").unwrap();
        let outside = dir.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("id_rsa"), "PRIVATE").unwrap();
        let has_link = link_dir(&outside, &project.join("src/keys"));

        let packages = packages_root(&project, true).unwrap();
        let staged = new_staging_dir(&packages).unwrap();
        copy_package_tree(&project, &staged, &packages, 0).unwrap();
        assert!(staged.join("src/main.synx").is_file());
        assert!(!staged.join(".git").exists());
        assert!(!staged.join(PACKAGES_DIR).exists(), "the destination was copied into itself");
        if has_link {
            assert!(!staged.join("src/keys").exists(), "a link was followed");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn secret_file_names() {
        for name in [".env", ".env.local", ".env.production", "id_rsa", "server.key", "cert.PEM", "AuthKey_X.p8",
            ".npmrc", ".synx.lock", "credentials.toml", "prod.env", "local.env", "staging.env", ".envrc", ".pgpass",
            ".my.cnf", ".git-credentials", ".dockercfg", "_netrc", "credentials", "kubeconfig", "id_rsa_github",
            "app.env.production"] {
            assert!(is_secret_file_name(name), "{}", name);
        }
        for name in [".env.example", "main.synx", "lib.rs", "README.md", "keys.synx", "environment.md", "id_rsa.pub",
            "credentials.md", "prod.env.example", "app.environment"] {
            assert!(!is_secret_file_name(name), "{}", name);
        }
    }
}
