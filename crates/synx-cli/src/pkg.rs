//! SYNX Package Management — CLI operations for install, publish, login, etc.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use sha2::{Sha256, Digest};

use crate::pkg_guard::{
    self, contained_rel_path, is_secret_file_name, is_service_dir, new_staging_dir, package_dir,
    packages_root, remove_package_dir, remove_staging_root, swap_into_place, unpack_archive,
    terminal_safe, validate_package_name, validate_registry_name, validate_version, PACKAGES_DIR,
};

// ─── Constants ────────────────────────────────────────────

const DEFAULT_REGISTRY: &str = "https://synx.aperturesyndicate.com/api";
const MANIFEST_NAME: &str = "synx-pkg.synx";
const LOCK_FILE: &str = "synx.lock";
const MAX_PACKAGE_SIZE: u64 = 5 * 1024 * 1024; // 5 MiB
/// Lock entry of a package installed from a local directory.
const LOCAL_INTEGRITY: &str = "local";

// ─── Manifest ─────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub main: String,
    pub synx_version: Option<String>,
    pub keywords: Vec<String>,
    pub files: Vec<String>,
    pub synxignore: Vec<String>,
    pub dependencies: HashMap<String, String>,
    pub repository: Option<String>,
}

impl Manifest {
    /// Parse a synx-pkg.synx manifest file.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut name = String::new();
        let mut version = String::new();
        let mut description = String::new();
        let mut author = String::new();
        let mut license = String::new();
        let mut main = String::from("src/main.synx");
        let mut synx_version = None;
        let mut repository = None;
        let mut keywords: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        let mut synxignore: Vec<String> = Vec::new();
        let mut dependencies: HashMap<String, String> = HashMap::new();

        let mut current_list: Option<&str> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // List items
            if trimmed.starts_with("- ") {
                let item = trimmed[2..].trim().to_string();
                match current_list {
                    Some("keywords") => keywords.push(item),
                    Some("files") => files.push(item),
                    Some("synxignore") => synxignore.push(item),
                    Some("dependencies") => {
                        // Format: @scope/name ^1.0.0
                        let mut parts = item.splitn(2, ' ');
                        let dep_name = parts.next().unwrap_or("").to_string();
                        let dep_ver = parts.next().unwrap_or("*").to_string();
                        if !dep_name.is_empty() {
                            dependencies.insert(dep_name, dep_ver);
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Key-value pairs
            current_list = None;
            if let Some((key, value)) = trimmed.split_once(' ') {
                match key {
                    "name" => name = value.trim().to_string(),
                    "version" => version = value.trim().to_string(),
                    "description" => description = value.trim().to_string(),
                    "author" => author = value.trim().to_string(),
                    "license" => license = value.trim().to_string(),
                    "main" | "entry" => main = value.trim().to_string(),
                    "synx-version" => synx_version = Some(value.trim().to_string()),
                    "repository" => repository = Some(value.trim().to_string()),
                    "keywords" | "files" | "synxignore" | "dependencies" => {
                        current_list = Some(key);
                    }
                    _ => {}
                }
            } else {
                // Bare key starts a list section
                match trimmed {
                    "keywords" => current_list = Some("keywords"),
                    "files" => current_list = Some("files"),
                    "synxignore" => current_list = Some("synxignore"),
                    "dependencies" => current_list = Some("dependencies"),
                    _ => {}
                }
            }
        }

        if name.is_empty() {
            return Err("manifest missing required field: name".into());
        }
        if version.is_empty() {
            return Err("manifest missing required field: version".into());
        }

        Ok(Manifest {
            name,
            version,
            description,
            author,
            license,
            main,
            synx_version,
            keywords,
            files,
            synxignore,
            dependencies,
            repository,
        })
    }

    /// Validate manifest fields for publishing.
    pub fn validate_for_publish(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push("name is required".into());
        }
        if self.version.is_empty() {
            errors.push("version is required".into());
        }
        if self.description.is_empty() {
            errors.push("description is required".into());
        }
        if self.author.is_empty() {
            errors.push("author is required".into());
        }
        if self.license.is_empty() {
            errors.push("license is required".into());
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    /// Build the full scoped package name.
    /// If the manifest name already contains @scope/, return as-is.
    /// Otherwise prefix with @nickname/ from the logged-in user.
    pub fn full_name(&self, nickname: &str) -> String {
        if self.name.starts_with('@') && self.name.contains('/') {
            self.name.to_lowercase()
        } else {
            format!("@{}/{}", nickname.to_lowercase(), self.short_name().to_lowercase())
        }
    }

    /// Scope from name (e.g. "@aperture/synx-defaults" → "aperture")
    #[allow(dead_code)]
    pub fn scope(&self) -> &str {
        self.name
            .strip_prefix('@')
            .and_then(|s| s.split('/').next())
            .unwrap_or("")
    }

    /// Short name (e.g. "@aperture/synx-defaults" → "synx-defaults")
    pub fn short_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
}

// ─── Lock file ────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub integrity: String,
    /// Public key (hex) that signed the installed archive, pinned on first
    /// install; empty when the registry sent no signature.
    pub signer: String,
}

impl LockEntry {
    /// Whether the entry pins a registry archive (not a local copy, not a bare name).
    pub fn is_pinned(&self) -> bool {
        !self.version.is_empty() && !self.integrity.is_empty() && self.integrity != LOCAL_INTEGRITY
    }
}

#[derive(Debug, Clone, Default)]
pub struct LockFile {
    pub entries: Vec<LockEntry>,
}

impl LockFile {
    /// Read synx.lock from the project root.
    pub fn read(project_root: &Path) -> Self {
        let lock_path = project_root.join(LOCK_FILE);
        let text = match fs::read_to_string(&lock_path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut current = LockEntry::default();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                // Flush current entry
                if !current.name.is_empty() {
                    entries.push(std::mem::take(&mut current));
                }
                continue;
            }
            if trimmed.starts_with('@') && !trimmed.contains(' ') {
                // Package name line
                if !current.name.is_empty() {
                    entries.push(std::mem::take(&mut current));
                }
                current.name = trimmed.to_string();
            } else if let Some((key, val)) = trimmed.split_once(' ') {
                match key {
                    "version" => current.version = val.trim().to_string(),
                    "integrity" => current.integrity = val.trim().to_string(),
                    "signer" => current.signer = val.trim().to_string(),
                    _ => {}
                }
            }
        }
        // Flush last entry
        if !current.name.is_empty() {
            entries.push(current);
        }

        LockFile { entries }
    }

    /// Write synx.lock to disk.
    pub fn write(&self, project_root: &Path) -> Result<(), String> {
        let lock_path = project_root.join(LOCK_FILE);
        let mut out = String::new();
        out.push_str("# synx.lock — auto-generated, commit to git\n");
        out.push_str("# DO NOT EDIT MANUALLY\n\n");
        for entry in &self.entries {
            out.push_str(&entry.name);
            out.push('\n');
            out.push_str(&format!("  version {}\n", entry.version));
            if !entry.integrity.is_empty() {
                out.push_str(&format!("  integrity {}\n", entry.integrity));
            }
            if !entry.signer.is_empty() {
                out.push_str(&format!("  signer {}\n", entry.signer));
            }
            out.push('\n');
        }
        fs::write(&lock_path, &out)
            .map_err(|e| format!("cannot write {}: {}", lock_path.display(), e))
    }

    /// Find entry by name.
    pub fn find(&self, name: &str) -> Option<&LockEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Add or update an entry. The pinned signer of an existing entry is kept.
    pub fn upsert(&mut self, name: &str, version: &str, integrity: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.version = version.to_string();
            entry.integrity = integrity.to_string();
        } else {
            self.entries.push(LockEntry {
                name: name.to_string(),
                version: version.to_string(),
                integrity: integrity.to_string(),
                signer: String::new(),
            });
        }
    }

    /// Pin the key that signed `name`'s archive.
    pub fn set_signer(&mut self, name: &str, signer: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.signer = signer.to_string();
        }
    }

    /// Remove an entry.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|e| e.name != name);
    }
}

// ─── Credentials ──────────────────────────────────────────

fn credentials_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aperturesyndicate")
}

fn signing_key_path() -> PathBuf {
    credentials_dir().join("signing-key.hex")
}

/// Generate a new Ed25519 signing key pair and save to credentials dir.
pub fn keygen() -> Result<(), String> {
    let path = signing_key_path();
    if path.exists() {
        return Err(format!(
            "signing key already exists at {}. Delete it first to regenerate.",
            path.display()
        ));
    }
    let dir = credentials_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;

    let key = synx_core::signing::SigningKey::generate();
    let hex = key.to_hex();
    write_private(&path, hex.as_bytes())
        .map_err(|e| format!("cannot write signing key: {}", e))?;

    let verify_key = key.verify_key();
    println!("Signing key generated ✓");
    println!("  Private key: {}", path.display());
    println!("  Public key:  {}", verify_key.to_hex());
    println!();
    println!("Share your public key with users who want to verify your packages.");
    Ok(())
}

/// Read the signing key from disk, if present.
fn read_signing_key() -> Option<synx_core::signing::SigningKey> {
    let path = signing_key_path();
    let hex = fs::read_to_string(&path).ok()?;
    synx_core::signing::SigningKey::from_hex(hex.trim()).ok()
}

/// Show the public verify key.
pub fn show_public_key() -> Result<(), String> {
    let key = read_signing_key()
        .ok_or("no signing key found — run 'synx keygen' first")?;
    println!("{}", key.verify_key().to_hex());
    Ok(())
}

fn credentials_path() -> PathBuf {
    credentials_dir().join("credentials.toml")
}

pub fn read_token() -> Option<String> {
    let path = credentials_path();
    let text = fs::read_to_string(&path).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table
        .get("default")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn read_nickname() -> Option<String> {
    let path = credentials_path();
    let text = fs::read_to_string(&path).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table
        .get("default")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("nickname"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn registry_url() -> String {
    let path = credentials_dir().join("config.toml");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(table) = text.parse::<toml::Table>() {
            if let Some(url) = table
                .get("default")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("registry"))
                .and_then(|v| v.as_str())
            {
                return url.to_string();
            }
        }
    }
    DEFAULT_REGISTRY.to_string()
}

/// Writes a file only its owner can read.
///
/// safety: the file was created with the default mode and narrowed
/// afterwards (the signing key never was), so on a shared Unix machine the
/// token was readable in between. `%APPDATA%` on Windows is per-user already.
fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` applies to a new file only; an existing wider one is narrowed here.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content)
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)
    }
}

/// The text of `credentials.toml`.
///
/// safety: values were pasted between quotes with `format!`; a nickname from
/// the registry containing `"` or a line break rewrote the file — `registry`
/// included, which is where the token is sent next.
fn credentials_toml(token: &str, nickname: &str, registry: &str, created: &str) -> String {
    let quoted = |s: &str| toml::Value::String(s.to_string()).to_string();
    format!(
        "[default]\ntoken = {}\nnickname = {}\nregistry = {}\ncreated = {}\n",
        quoted(token),
        quoted(nickname),
        quoted(registry),
        quoted(created)
    )
}

pub fn save_token(token: &str, nickname: &str) -> Result<(), String> {
    let dir = credentials_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;

    let content = credentials_toml(token, nickname, &registry_url(), &chrono_now_rfc3339());
    let path = credentials_path();
    write_private(&path, content.as_bytes())
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

/// One HTTP client for every registry call.
///
/// why: ureq turns a 4xx/5xx into an error and drops the body, so the status
/// checks below never ran and the registry's reason ("Version already
/// exists") was lost; the status is read by the caller instead.
/// safety: no request had a time limit; an `https` registry is never followed
/// to plain `http`.
fn registry_agent(registry: &str) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(registry.starts_with("https://"))
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// Percent-encodes a query value.
fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

pub fn remove_token() -> Result<(), String> {
    let path = credentials_path();
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("cannot remove {}: {}", path.display(), e))?;
    }
    Ok(())
}

fn chrono_now_rfc3339() -> String {
    // RFC3339 without chrono dependency — civil calendar from Unix epoch
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();
    let secs_of_day = total_secs % 86400;
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    let ss = secs_of_day % 60;

    // Days since 1970-01-01 → civil date (algorithm from Howard Hinnant)
    let mut days = (total_secs / 86400) as i64;
    days += 719468; // shift epoch from 1970-01-01 to 0000-03-01
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss)
}

// ─── SHA-256 ──────────────────────────────────────────────

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)
            .map_err(|e| format!("read error: {}", e))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256-{:x}", hasher.finalize()))
}

fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("sha256-{:x}", hasher.finalize())
}

// ─── Pack (tarball creation) ──────────────────────────────

/// Check if a relative path matches any synxignore pattern.
/// Supports gitignore-style patterns: `*.ext`, `dir/`, `file.txt`, `path/to/file`.
fn synxignore_matches(rel_path: &str, patterns: &[String]) -> bool {
    // Normalize: strip leading ./ and use forward slashes
    let norm = rel_path.replace('\\', "/");
    let norm = norm.strip_prefix("./").unwrap_or(&norm);

    for pattern in patterns {
        let pat = pattern.replace('\\', "/");
        let pat = pat.strip_prefix("./").unwrap_or(&pat);
        let pat = pat.trim();
        if pat.is_empty() {
            continue;
        }

        // Directory pattern: "dir/" matches anything under that dir
        if pat.ends_with('/') {
            let prefix = pat.strip_suffix('/').unwrap_or(pat);
            if norm == prefix || norm.starts_with(&format!("{}/", prefix)) {
                return true;
            }
            continue;
        }

        // Exact match
        if norm == pat {
            return true;
        }

        // Glob pattern with *
        if pat.contains('*') {
            if glob_match(pat, norm) {
                return true;
            }
            // Also match just the filename against the pattern (e.g. *.bat matches src/build.bat)
            if let Some(filename) = norm.rsplit('/').next() {
                if glob_match(pat, filename) {
                    return true;
                }
            }
            continue;
        }

        // Plain filename match against any path component
        if !pat.contains('/') {
            if let Some(filename) = norm.rsplit('/').next() {
                if filename == pat {
                    return true;
                }
            }
        }
    }
    false
}

/// Simple glob matching supporting `*` (any chars except `/`) and `**` (any path).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_bytes = pattern.as_bytes();
    let txt_bytes = text.as_bytes();
    let (plen, tlen) = (pat_bytes.len(), txt_bytes.len());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0usize);

    while ti < tlen {
        if pi < plen && pat_bytes[pi] == b'*' {
            // Skip consecutive stars
            while pi < plen && pat_bytes[pi] == b'*' {
                pi += 1;
            }
            if pi >= plen {
                return true; // trailing *
            }
            star_pi = pi;
            star_ti = ti;
        } else if pi < plen && (pat_bytes[pi] == b'?' || pat_bytes[pi] == txt_bytes[ti]) {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < plen && pat_bytes[pi] == b'*' {
        pi += 1;
    }
    pi >= plen
}

/// One file in a package archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedFile {
    pub name: String,
    pub bytes: u64,
}

/// What `pack` wrote, and what it left out or wants looked at.
#[derive(Debug)]
pub struct PackReport {
    pub tarball: PathBuf,
    pub integrity: String,
    pub files: Vec<PackedFile>,
    /// Paths that were found and not packed, with the reason.
    pub skipped: Vec<(String, &'static str)>,
    /// `file: key` of every packed `.synx` value written as a literal `:secret`.
    pub secret_literals: Vec<String>,
    /// Files packed because `files` names them, although they look like credentials.
    pub listed_secret_files: Vec<String>,
}

const NOTHING_TO_PACK: &str = "nothing to pack: synx-pkg.synx has no `files` list and the package has no src/ directory.\n\
List what to publish under `files`, or move the package sources into src/.\n\
(Without either, earlier versions packed every .synx file in the project — \
configs with secrets included.)";

/// Whether `m` is a real directory rather than a link to one.
fn is_real_dir(m: &fs::Metadata) -> bool {
    m.is_dir() && !m.file_type().is_symlink()
}

/// Whether `m` is a real file rather than a link to one.
fn is_real_file(m: &fs::Metadata) -> bool {
    m.is_file() && !m.file_type().is_symlink()
}

/// `a/b/c` for a relative path, whatever the platform separator.
fn archive_name(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Collects the regular files under `dir` (`rel` is its path inside the package).
///
/// safety: links are not followed — a link in a cloned package pulls files
/// from outside it into a public archive, and a link to a parent directory
/// never ends. Credential files and service directories are left out and
/// reported.
fn collect_package_files(
    dir: &Path,
    rel: &Path,
    out: &mut Vec<(PathBuf, PathBuf)>,
    skipped: &mut Vec<(String, &'static str)>,
    depth: usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err(format!("{} is nested deeper than 32 directories", dir.display()));
    }
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {}", dir.display(), e))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name_os = entry.file_name();
        let child_rel = rel.join(&name_os);
        let shown = archive_name(&child_rel);
        let Some(name) = name_os.to_str() else {
            skipped.push((shown, "a name that is not valid UTF-8"));
            continue;
        };
        let kind = entry
            .file_type()
            .map_err(|e| format!("cannot read {}: {}", entry.path().display(), e))?;
        if kind.is_symlink() {
            skipped.push((shown, "a link"));
        } else if kind.is_dir() {
            if is_service_dir(name) || name == PACKAGES_DIR {
                skipped.push((shown, "a service directory"));
            } else {
                collect_package_files(&entry.path(), &child_rel, out, skipped, depth + 1)?;
            }
        } else if kind.is_file() {
            if is_secret_file_name(name) {
                skipped.push((shown, "looks like a credential file"));
            } else {
                out.push((entry.path(), child_rel));
            }
        }
    }
    Ok(())
}

/// Markers whose key holds a reference (a variable name, a path, an
/// expression) rather than the secret itself.
const SECRET_SOURCE_MARKERS: &[&str] = &[
    "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default", "fallback",
    "once", "split", "join", "format", "replace", "random", "template", "inherit", "prompt",
];

/// Dot-paths of keys in `text` whose `:secret` value is written in the file.
///
/// why: a published archive is public and stays in the registry's storage; a
/// literal secret in it is leaked for good, and in a file without `!active`
/// it is not even masked on output.
fn find_secret_literals(text: &str) -> Vec<String> {
    let parsed = synx_core::parse(text);
    // Markers are recorded in active mode only; the scan needs them either way.
    let parsed = if parsed.mode == synx_core::Mode::Active {
        parsed
    } else {
        synx_core::parse(&format!("!active\n{}", text))
    };
    let mut found = Vec::new();
    for (prefix, keys) in &parsed.metadata {
        for (key, meta) in keys {
            let is_literal = meta.markers.iter().any(|m| m == "secret")
                && !meta.markers.iter().any(|m| SECRET_SOURCE_MARKERS.contains(&m.as_str()));
            if !is_literal {
                continue;
            }
            let mut node = Some(&parsed.root);
            let path = prefix.split('.').filter(|s| !s.is_empty()).chain(std::iter::once(key.as_str()));
            for segment in path {
                node = match node {
                    Some(synx_core::Value::Object(map)) => map.get(segment),
                    _ => None,
                };
            }
            let has_value = match node {
                Some(synx_core::Value::String(s)) => !s.is_empty(),
                Some(synx_core::Value::Int(_))
                | Some(synx_core::Value::Float(_))
                | Some(synx_core::Value::Bool(_))
                | Some(synx_core::Value::Secret(_)) => true,
                _ => false,
            };
            if has_value {
                found.push(if prefix.is_empty() { key.clone() } else { format!("{}.{}", prefix, key) });
            }
        }
    }
    found.sort();
    found
}

/// Builds `<name>-<version>.tar.gz` in `dir` from the manifest, the README,
/// the `files` list (or `src/` when there is none) and the `main` entry.
pub fn pack_report(dir: &Path) -> Result<PackReport, String> {
    let manifest_path = dir.join(MANIFEST_NAME);
    let manifest = Manifest::from_file(&manifest_path)?;
    manifest.validate_for_publish().map_err(|errs| errs.join("\n"))?;
    // safety: both become the archive file name, joined onto `dir`.
    validate_package_name(&manifest.name)?;
    validate_version(&manifest.version)?;

    let mut skipped: Vec<(String, &'static str)> = Vec::new();
    // (path on disk, path in the package, named in `files` as a file)
    let mut candidates: Vec<(PathBuf, PathBuf, bool)> = Vec::new();

    if !manifest.files.is_empty() {
        for listed in &manifest.files {
            let rel = contained_rel_path(Path::new(listed)).map_err(|e| format!("files: {}", e))?;
            let path = dir.join(&rel);
            match fs::symlink_metadata(&path) {
                Err(_) => skipped.push((listed.clone(), "not found")),
                Ok(m) if is_real_dir(&m) => {
                    let mut inside = Vec::new();
                    collect_package_files(&path, &rel, &mut inside, &mut skipped, 0)?;
                    candidates.extend(inside.into_iter().map(|(p, r)| (p, r, false)));
                }
                Ok(m) if is_real_file(&m) => candidates.push((path, rel, true)),
                Ok(_) => skipped.push((listed.clone(), "a link")),
            }
        }
    } else {
        // safety: with no `files` and no src/ this used to walk the whole
        // project for .synx files — `config/secrets.synx` and
        // `deploy/prod/app.synx` went into a public archive.
        let src_dir = dir.join("src");
        match fs::symlink_metadata(&src_dir) {
            Ok(m) if is_real_dir(&m) => {
                let mut inside = Vec::new();
                collect_package_files(&src_dir, Path::new("src"), &mut inside, &mut skipped, 0)?;
                candidates.extend(inside.into_iter().map(|(p, r)| (p, r, false)));
            }
            _ => return Err(NOTHING_TO_PACK.into()),
        }
    }

    let main_rel = contained_rel_path(Path::new(&manifest.main)).map_err(|e| format!("main: {}", e))?;
    let main_path = dir.join(&main_rel);

    // Enforce source code presence: at least one non-binary source file required
    let source_exts = ["rs", "synx", "js", "ts", "py", "go", "kt", "swift", "c", "cpp", "h", "mojo"];
    let has_source = candidates.iter().any(|(path, _, _)| {
        path.extension()
            .and_then(|e| e.to_str())
            .map_or(false, |ext| source_exts.contains(&ext))
    });
    if !has_source {
        return Err(
            "source code is required for publish.\n\
             Add source files to the 'files' field in synx-pkg.synx, \
             or place them in src/.\n\
             Packages must include source code for transparency and security."
                .into(),
        );
    }

    // Build synxignore patterns (always exclude *.tar.gz)
    let mut ignore_patterns = manifest.synxignore.clone();
    ignore_patterns.push("*.tar.gz".to_string());

    // safety: only the archive being written is replaced. The cleanup that
    // ran here removed every `<name>*.tar.gz` next to the manifest —
    // `tools-prod-backup.tar.gz` went with it.
    let tarball_name = format!("{}-{}.tar.gz", manifest.short_name(), manifest.version);
    let tarball_path = dir.join(&tarball_name);

    let tar_file = fs::File::create(&tarball_path)
        .map_err(|e| format!("cannot create {}: {}", tarball_path.display(), e))?;
    let enc = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
    let mut tar_builder = tar::Builder::new(enc);
    tar_builder.follow_symlinks(false);
    // why: no local uid, gid or mtime in a public archive, and the same
    // sources give the same integrity hash.
    tar_builder.mode(tar::HeaderMode::Deterministic);

    let mut packing = Packing::default();

    // Always include the manifest
    packing.add(&mut tar_builder, &manifest_path, MANIFEST_NAME.to_string())?;

    // Include README if present
    let readme = dir.join("README.md");
    if fs::symlink_metadata(&readme).map_or(false, |m| is_real_file(&m)) {
        packing.add(&mut tar_builder, &readme, "README.md".to_string())?;
    }

    let mut listed_secret_files: Vec<String> = Vec::new();
    for (path, rel, is_listed_file) in &candidates {
        let name = archive_name(rel);
        if synxignore_matches(&name, &ignore_patterns) {
            continue;
        }
        let file_name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if *is_listed_file && is_secret_file_name(&file_name) {
            listed_secret_files.push(name.clone());
        }
        packing.add(&mut tar_builder, path, name)?;
    }

    // Include main entry if it's not already covered by files list and not ignored
    if fs::symlink_metadata(&main_path).map_or(false, |m| is_real_file(&m)) {
        let name = archive_name(&main_rel);
        if !synxignore_matches(&name, &ignore_patterns) {
            packing.add(&mut tar_builder, &main_path, name)?;
        }
    }

    let enc = tar_builder.into_inner().map_err(|e| format!("tar finish error: {}", e))?;
    enc.finish().map_err(|e| format!("tar finish error: {}", e))?;

    let integrity = sha256_file(&tarball_path)?;
    Ok(PackReport {
        tarball: tarball_path,
        integrity,
        files: packing.files,
        skipped,
        secret_literals: packing.secret_literals,
        listed_secret_files,
    })
}

/// The archive being filled: what is in it already and what was noticed on the way.
#[derive(Default)]
struct Packing {
    files: Vec<PackedFile>,
    names: HashSet<String>,
    secret_literals: Vec<String>,
}

impl Packing {
    /// Appends `path` as `name` once; a second request for the same name (the
    /// manifest listed again under `files`) is ignored.
    fn add<W: Write>(&mut self, builder: &mut tar::Builder<W>, path: &Path, name: String) -> Result<(), String> {
        if !self.names.insert(name.clone()) {
            return Ok(());
        }
        let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        builder
            .append_path_with_name(path, &name)
            .map_err(|e| format!("tar error: {}", e))?;
        if name.ends_with(".synx") && bytes <= 10 * 1024 * 1024 {
            if let Ok(text) = fs::read_to_string(path) {
                let literals = find_secret_literals(&text);
                self.secret_literals.extend(literals.into_iter().map(|key| format!("{}: {}", name, key)));
            }
        }
        self.files.push(PackedFile { name, bytes });
        Ok(())
    }
}

/// Prints what went into the archive and what was left out, on stderr.
pub fn print_pack_report(report: &PackReport) {
    eprintln!("packed {} file(s):", report.files.len());
    for file in &report.files {
        eprintln!("  {} ({} bytes)", file.name, file.bytes);
    }
    for (path, why) in &report.skipped {
        eprintln!("  left out: {} — {}", path, why);
    }
    for name in &report.listed_secret_files {
        eprintln!("warning: {} looks like a credential file and is packed because `files` names it", name);
    }
    for literal in &report.secret_literals {
        eprintln!("warning: literal :secret value in {}", literal);
    }
}

// ─── Install ──────────────────────────────────────────────

/// What the registry says about one version of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryVersion {
    version: String,
    tarball_url: String,
    integrity: String,
    signature: String,
    public_key: String,
    yanked: bool,
}

impl RegistryVersion {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
        let version = v["version"].as_str()?.to_string();
        Some(RegistryVersion {
            version,
            tarball_url: text("tarballUrl"),
            integrity: text("integrity"),
            signature: text("signature"),
            public_key: text("publicKey"),
            yanked: v["yanked"].as_bool().unwrap_or(false),
        })
    }
}

/// Picks the version to install from a registry response.
///
/// `want` is the exact version asked for (by the user or by `synx.lock`);
/// without it the first version that is not yanked is taken.
///
/// safety: the answer is checked against the question — a registry that
/// replies with another version than the one asked for is refused, and the
/// version text is validated before it reaches a path, a URL or the lock.
fn pick_version(meta: &serde_json::Value, want: Option<&str>) -> Result<RegistryVersion, String> {
    let picked = if meta["version"].is_string() {
        RegistryVersion::from_json(meta)
    } else if let Some(versions) = meta["versions"].as_array() {
        let mut listed = versions.iter().filter_map(RegistryVersion::from_json);
        match want {
            Some(v) => listed.find(|r| r.version == v),
            None => listed.find(|r| !r.yanked),
        }
    } else {
        return Err("unexpected registry response format".to_string());
    };
    let picked = picked.ok_or("no available versions")?;
    validate_version(&picked.version).map_err(|e| format!("registry sent an {}", e))?;
    if let Some(v) = want {
        if picked.version != v {
            return Err(format!(
                "registry answered with version {} when {} was asked for",
                picked.version, v
            ));
        }
    }
    Ok(picked)
}

/// The newest version `locked` may move to: the highest one that is not
/// yanked and stays inside `^locked`.
fn pick_update(meta: &serde_json::Value, locked: &str) -> Option<RegistryVersion> {
    let requirement = format!("^{}", locked);
    meta["versions"]
        .as_array()?
        .iter()
        .filter_map(RegistryVersion::from_json)
        .filter(|r| !r.yanked && validate_version(&r.version).is_ok())
        .filter(|r| version_satisfies(&requirement, &r.version))
        .max_by_key(|r| parse_semver(&r.version))
}

/// Refuses a download address the registry has no business sending.
///
/// safety: the address comes from the registry's response. `http://` let
/// anyone on the path swap the archive; a plain-http registry (a local one
/// the user configured) may still point at plain http.
fn check_download_url(url: &str, registry: &str) -> Result<(), String> {
    let is_allowed = url.starts_with("https://")
        || (url.starts_with("http://") && registry.starts_with("http://"));
    if is_allowed {
        Ok(())
    } else {
        Err(format!("registry sent a download address that is not https: {:?}", url))
    }
}

/// Downloads at most `cap` bytes; a larger body is an error, not a truncation.
///
/// safety: the body was read to the end before its size was looked at.
fn download_capped(agent: &ureq::Agent, url: &str, cap: u64) -> Result<Vec<u8>, String> {
    let mut resp = agent.get(url).call().map_err(|e| format!("download error: {}", e))?;
    if resp.status() != 200 {
        return Err(format!("download failed (HTTP {})", resp.status()));
    }
    let too_large = || format!("package too large (more than {} bytes)", cap);
    if resp.body().content_length().map_or(false, |len| len > cap) {
        return Err(too_large());
    }
    let mut data = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(cap + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("download read error: {}", e))?;
    if data.len() as u64 > cap {
        return Err(too_large());
    }
    Ok(data)
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16)?;
            let lo = (pair[1] as char).to_digit(16)?;
            Some((hi * 16 + lo) as u8)
        })
        .collect()
}

/// Checks a downloaded archive against the registry's answer and `synx.lock`.
/// Returns the archive's hash and the key that signed it (empty when unsigned).
///
/// safety: `integrity` and the public key travel in the same response as the
/// archive address, so on their own they prove nothing about a registry (or a
/// bucket) that changed its mind. `synx.lock` is the one record the registry
/// cannot rewrite: the hash pinned for a version must match, and once a
/// package has been installed with a signature, the same key must sign every
/// later archive.
fn verify_download(
    name: &str,
    picked: &RegistryVersion,
    data: &[u8],
    locked: Option<&LockEntry>,
) -> Result<(String, String), String> {
    let actual = sha256_bytes(data);
    if !picked.integrity.is_empty() && picked.integrity != actual {
        return Err(format!(
            "integrity check failed: expected {}, got {}",
            picked.integrity, actual
        ));
    }
    if let Some(entry) = locked {
        if entry.is_pinned() && entry.version == picked.version && entry.integrity != actual {
            return Err(format!(
                "integrity differs from synx.lock for {}@{}: locked {}, downloaded {}.\n\
                 The archive in the registry is not the one that was locked. \
                 If that is expected, remove the entry from synx.lock and install again.",
                name, picked.version, entry.integrity, actual
            ));
        }
    }

    let pinned_signer = locked.map(|e| e.signer.as_str()).unwrap_or("");
    let is_signed = !picked.signature.is_empty() && !picked.public_key.is_empty();
    if !is_signed {
        if !pinned_signer.is_empty() {
            return Err(format!(
                "synx.lock pins a publisher key for {}, but the registry sent no signature",
                name
            ));
        }
        return Ok((actual, String::new()));
    }
    let public_key = picked.public_key.to_ascii_lowercase();
    // The key parser slices by byte; anything but 64 hex digits stops here.
    if public_key.len() != 64 || !public_key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid public key: expected 64 hex digits".to_string());
    }
    if !pinned_signer.is_empty() && !pinned_signer.eq_ignore_ascii_case(&public_key) {
        return Err(format!(
            "publisher key for {} changed: synx.lock pins {}, the registry sent {}.\n\
             If the publisher rotated the key, remove the `signer` line from synx.lock and install again.",
            name, pinned_signer, public_key
        ));
    }
    let verify_key = synx_core::signing::VerifyKey::from_hex(&public_key)
        .map_err(|e| format!("invalid public key: {}", e))?;
    let signature = decode_hex(&picked.signature).ok_or("invalid signature hex")?;
    match verify_key.verify(data, &signature) {
        Ok(true) => {}
        Ok(false) => return Err("signature verification failed — package may be tampered".to_string()),
        Err(e) => return Err(format!("signature error: {}", e)),
    }
    // safety: a public key of small order (the neutral point, with R equal to
    // it and S = 0) "signs" every message — one such line would be pinned on
    // first install and accept any archive afterwards. A signature that also
    // holds for other bytes proves nothing about these.
    let mut other = Vec::with_capacity(data.len() + 1);
    other.extend_from_slice(data);
    other.push(0x5a);
    if verify_key.verify(&other, &signature) == Ok(true) {
        return Err("signature does not depend on the archive (a degenerate key) — refused".to_string());
    }
    Ok((actual, public_key))
}

fn fetch_json(agent: &ureq::Agent, url: &str, name: &str) -> Result<serde_json::Value, String> {
    let resp = agent.get(url).call().map_err(|e| format!("registry error: {}", e))?;
    if resp.status() != 200 {
        return Err(format!(
            "package '{}' not found in registry (HTTP {})",
            name,
            resp.status()
        ));
    }
    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {}", e))?;
    serde_json::from_str(&body).map_err(|e| format!("invalid registry response: {}", e))
}

/// Install a package from the registry into synx_packages/
///
/// A package already in `synx.lock` is installed at the locked version, and
/// its archive must hash to what the lock records; `synx update` is the
/// command that moves a lock entry.
pub fn install(project_root: &Path, package: &str, version: Option<&str>) -> Result<(), String> {
    install_from(&registry_url(), project_root, package, version)
}

fn install_from(registry: &str, project_root: &Path, package: &str, version: Option<&str>) -> Result<(), String> {
    let (name, requested_version) = parse_package_spec(package, version);

    // Validate package name format
    validate_registry_name(&name)?;

    let lock = LockFile::read(project_root);
    let locked = lock.find(&name).cloned();

    // Check for version conflicts with existing lock file
    if let Some(req) = &requested_version {
        validate_version(req)?;
        if let Some(conflict) = check_conflicts(&lock, &name, req) {
            return Err(conflict);
        }
    }
    // safety: without an explicit version this took whatever the registry
    // called latest and overwrote the lock entry — the lock pinned nothing.
    let pinned_version = locked.as_ref().filter(|e| e.is_pinned()).map(|e| e.version.clone());
    let wanted = requested_version.or(pinned_version);
    if let Some(v) = &wanted {
        validate_version(v).map_err(|e| format!("synx.lock: {}", e))?;
    }

    // Fetch package metadata from registry
    // Route: GET /api/packages/:scope/:name or GET /api/packages/:scope/:name/:version
    // Note: name is @scope/short, API expects /packages/scope/short
    let agent = registry_agent(registry);
    let api_name = name.strip_prefix('@').unwrap_or(&name);
    let url = match &wanted {
        Some(v) => format!("{}/packages/{}/{}", registry, api_name, v),
        None => format!("{}/packages/{}", registry, api_name),
    };
    let meta = fetch_json(&agent, &url, &name)?;
    let picked = pick_version(&meta, wanted.as_deref())?;
    if picked.yanked {
        eprintln!("warning: {}@{} is yanked", name, picked.version);
    }

    install_picked(&agent, registry, project_root, &name, &picked, locked.as_ref())
}

/// Downloads, verifies and unpacks one resolved version, then records it in `synx.lock`.
fn install_picked(
    agent: &ureq::Agent,
    registry: &str,
    project_root: &Path,
    name: &str,
    picked: &RegistryVersion,
    locked: Option<&LockEntry>,
) -> Result<(), String> {
    check_download_url(&picked.tarball_url, registry)?;
    let tarball_data = download_capped(agent, &picked.tarball_url, MAX_PACKAGE_SIZE)?;
    let (integrity, signer) = verify_download(name, picked, &tarball_data, locked)?;
    if !signer.is_empty() {
        println!("  Signature verified ✓");
    }

    // Extract to synx_packages/@scope/name/ — into a staging directory first,
    // so a refused archive leaves the installed package as it was.
    let packages = packages_root(project_root, true)?;
    let pkg_dir = package_dir(&packages, name)?;
    let staged = new_staging_dir(&packages)?;
    let installed = unpack_archive(&tarball_data, &staged)
        .map_err(|e| format!("extract error: {}", e))
        .and_then(|_| swap_into_place(&staged, &pkg_dir));
    if installed.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    remove_staging_root(&packages);
    installed?;

    // Update synx.lock
    let mut lock = LockFile::read(project_root);
    lock.upsert(name, &picked.version, &integrity);
    if !signer.is_empty() {
        lock.set_signer(name, &signer);
    }
    lock.write(project_root)?;

    println!("Installed {}@{} ✓", name, picked.version);
    Ok(())
}

/// Install a package from a local directory (copy).
pub fn install_local(project_root: &Path, source_dir: &Path) -> Result<(), String> {
    let manifest_path = source_dir.join(MANIFEST_NAME);
    let manifest = Manifest::from_file(&manifest_path)?;

    // Determine the fully-scoped name.
    // If manifest.name already has '@scope/', use it.
    // Otherwise infer scope from directory structure (e.g. @assynx/text-tools/).
    let full_name = if manifest.name.starts_with('@') && manifest.name.contains('/') {
        manifest.name.clone()
    } else {
        // Try to infer scope from parent dir name (e.g. source_dir = .../@assynx/text-tools)
        let parent = source_dir.parent().and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if parent.starts_with('@') {
            format!("{}/{}", parent, manifest.name)
        } else {
            manifest.name.clone()
        }
    };

    // safety: the name comes from a manifest someone else may have written.
    // `name ../keepme` deleted the project's `keepme` directory and copied the
    // package over it; the version goes into synx.lock on its own line.
    validate_package_name(&full_name)?;
    validate_version(&manifest.version)?;

    // why: copied into a staging directory and swapped in — the old copy was
    // removed first, so a package installed from its own directory, or a
    // copy that failed halfway, left nothing.
    let packages = packages_root(project_root, true)?;
    let pkg_dir = package_dir(&packages, &full_name)?;
    let staged = new_staging_dir(&packages)?;
    let installed = pkg_guard::copy_package_tree(source_dir, &staged, &packages, 0)
        .and_then(|_| swap_into_place(&staged, &pkg_dir));
    if installed.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    remove_staging_root(&packages);
    installed?;

    // Update synx.lock
    let mut lock = LockFile::read(project_root);
    lock.upsert(&full_name, &manifest.version, LOCAL_INTEGRITY);
    lock.set_signer(&full_name, "");
    lock.write(project_root)?;

    println!("Installed {} (local) ✓", full_name);
    Ok(())
}

// ─── Uninstall ────────────────────────────────────────────

pub fn uninstall(project_root: &Path, package: &str) -> Result<(), String> {
    let name = if package.contains('@') {
        package.to_string()
    } else {
        return Err(format!(
            "invalid package name '{}': must be @scope/name",
            package
        ));
    };
    // safety: `@x/../../victim` and an absolute path both passed the check
    // above and went to `remove_dir_all`.
    validate_registry_name(&name)?;

    let not_installed = || format!("package '{}' is not installed", name);
    let packages = packages_root(project_root, false).map_err(|_| not_installed())?;
    let pkg_dir = package_dir(&packages, &name)?;
    if fs::symlink_metadata(&pkg_dir).is_err() {
        return Err(not_installed());
    }

    remove_package_dir(&pkg_dir)?;

    // Clean up empty scope directory
    if let Some(scope_dir) = pkg_dir.parent() {
        if scope_dir.read_dir().map_or(false, |mut d| d.next().is_none()) {
            let _ = fs::remove_dir(scope_dir);
        }
    }

    // Update synx.lock
    let mut lock = LockFile::read(project_root);
    lock.remove(&name);
    lock.write(project_root)?;

    println!("Uninstalled {} ✓", name);
    Ok(())
}

// ─── List ─────────────────────────────────────────────────

pub fn list_installed(project_root: &Path) -> Result<(), String> {
    let pkg_root = project_root.join(PACKAGES_DIR);
    if !pkg_root.exists() {
        println!("No packages installed.");
        return Ok(());
    }

    let lock = LockFile::read(project_root);
    let mut count = 0;

    for scope_entry in fs::read_dir(&pkg_root).map_err(|e| e.to_string())?.flatten() {
        let scope_path = scope_entry.path();
        if !scope_path.is_dir() { continue; }
        let scope_name = scope_entry.file_name();
        let scope_str = scope_name.to_string_lossy();
        if !scope_str.starts_with('@') { continue; }

        for pkg_entry in fs::read_dir(&scope_path).map_err(|e| e.to_string())?.flatten() {
            let pkg_path = pkg_entry.path();
            if !pkg_path.is_dir() { continue; }

            let full_name = format!("{}/{}", scope_str, pkg_entry.file_name().to_string_lossy());

            // Try to read manifest for version
            let manifest_path = pkg_path.join(MANIFEST_NAME);
            let version = if let Ok(m) = Manifest::from_file(&manifest_path) {
                m.version
            } else {
                lock.find(&full_name)
                    .map(|e| e.version.clone())
                    .unwrap_or_else(|| "?".into())
            };

            let lock_status = if lock.find(&full_name).is_some() { "" } else { " (not in synx.lock)" };
            println!("  {}@{}{}", terminal_safe(&full_name), terminal_safe(&version), lock_status);
            count += 1;
        }
    }

    if count == 0 {
        println!("No packages installed.");
    } else {
        println!("\n{} package(s) installed.", count);
    }
    Ok(())
}

// ─── Info ─────────────────────────────────────────────────

pub fn info(package: &str) -> Result<(), String> {
    // safety: the name is joined onto synx_packages/ and into the registry URL.
    validate_package_name(package)?;

    // Try local first
    let local_dir = Path::new(PACKAGES_DIR).join(package);
    if local_dir.exists() {
        let manifest_path = local_dir.join(MANIFEST_NAME);
        if let Ok(m) = Manifest::from_file(&manifest_path) {
            print_manifest_info(&m, true);
            return Ok(());
        }
    }

    // Try registry
    let registry = registry_url();
    let api_name = package.strip_prefix('@').unwrap_or(package);
    let url = format!("{}/packages/{}", registry, api_name);
    let resp = registry_agent(&registry).get(&url)
        .call()
        .map_err(|e| format!("registry error: {}", e))?;

    if resp.status() != 200 {
        return Err(format!("package '{}' not found", package));
    }

    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {}", e))?;
    let meta: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("invalid response: {}", e))?;

    // Get latest version from versions array
    let latest_ver = meta["versions"].as_array()
        .and_then(|v| v.first())
        .and_then(|v| v["version"].as_str())
        .unwrap_or("?");
    println!("{}@{}", package, terminal_safe(latest_ver));
    if let Some(desc) = meta["description"].as_str() {
        println!("{}", terminal_safe(desc));
    }
    if let Some(author) = meta["author"].as_str() {
        println!("author: {}", terminal_safe(author));
    }
    if let Some(license) = meta["license"].as_str() {
        println!("license: {}", terminal_safe(license));
    }
    if let Some(versions) = meta["versions"].as_array() {
        println!("versions:");
        for v in versions {
            let ver = v["version"].as_str().unwrap_or("?");
            let yanked = if v["yanked"].as_bool().unwrap_or(false) { " (yanked)" } else { "" };
            let dl = v["downloads"].as_u64().unwrap_or(0);
            println!("  {} — {} downloads{}", terminal_safe(ver), dl, yanked);
        }
    }

    Ok(())
}

fn print_manifest_info(m: &Manifest, local: bool) {
    println!("{}@{}{}", terminal_safe(&m.name), terminal_safe(&m.version), if local { " (local)" } else { "" });
    if !m.description.is_empty() {
        println!("{}", terminal_safe(&m.description));
    }
    println!("author:  {}", terminal_safe(&m.author));
    println!("license: {}", terminal_safe(&m.license));
    println!("main:    {}", terminal_safe(&m.main));
    if let Some(ref sv) = m.synx_version {
        println!("synx:    {}", terminal_safe(sv));
    }
    if !m.keywords.is_empty() {
        println!("keywords: {}", terminal_safe(&m.keywords.join(", ")));
    }
    if !m.dependencies.is_empty() {
        println!("dependencies:");
        for (dep, ver) in &m.dependencies {
            println!("  {} {}", terminal_safe(dep), terminal_safe(ver));
        }
    }
}

// ─── Search ───────────────────────────────────────────────

pub fn search(query: &str) -> Result<(), String> {
    let registry = registry_url();
    let url = format!("{}/packages?q={}", registry, url_encode(query));
    let resp = registry_agent(&registry).get(&url)
        .call()
        .map_err(|e| format!("registry error: {}", e))?;

    if resp.status() != 200 {
        return Err(format!("search failed (HTTP {})", resp.status()));
    }

    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {}", e))?;
    let results: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("invalid response: {}", e))?;

    let total = results["total"].as_u64().unwrap_or(0);
    if let Some(packages) = results["packages"].as_array() {
        if packages.is_empty() {
            println!("No packages found for '{}'.", terminal_safe(query));
            return Ok(());
        }
        println!("Found {} packages:", total);
        println!();
        for pkg in packages {
            let name = pkg["name"].as_str().unwrap_or("?");
            let version = pkg["latestVersion"].as_str().unwrap_or("?");
            let desc = pkg["description"].as_str().unwrap_or("");
            let dl = pkg["totalDownloads"].as_u64().unwrap_or(0);
            println!("{}@{} — {} downloads", terminal_safe(name), terminal_safe(version), dl);
            if !desc.is_empty() {
                println!("  {}", terminal_safe(desc));
            }
        }
    } else {
        println!("No packages found for '{}'.", terminal_safe(query));
    }

    Ok(())
}

// ─── Yank ─────────────────────────────────────────────────

/// Yank a published version (mark as unavailable without deleting).
pub fn yank(package: &str, version: &str) -> Result<(), String> {
    let token = read_token()
        .ok_or("not logged in — run 'synx login' first")?;

    let name = if package.starts_with('@') {
        package.strip_prefix('@').unwrap_or(package)
    } else {
        package
    };

    let registry = registry_url();
    let url = format!("{}/packages/{}/{}/yank", registry, name, version);

    let resp = registry_agent(&registry).post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send(&[] as &[u8])
        .map_err(|e| format!("yank error: {}", e))?;

    let status = resp.status();
    if status == 200 || status == 204 {
        println!("Yanked {}@{} ✓", package, version);
        println!("  Package is no longer installable, but existing installs are unaffected.");
        Ok(())
    } else {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        Err(format!("yank failed (HTTP {}): {}", status, body))
    }
}

/// Undo a yank — make a previously yanked version available again.
pub fn unyank(package: &str, version: &str) -> Result<(), String> {
    let token = read_token()
        .ok_or("not logged in — run 'synx login' first")?;

    let name = if package.starts_with('@') {
        package.strip_prefix('@').unwrap_or(package)
    } else {
        package
    };

    let registry = registry_url();
    let url = format!("{}/packages/{}/{}/unyank", registry, name, version);

    let resp = registry_agent(&registry).post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send(&[] as &[u8])
        .map_err(|e| format!("unyank error: {}", e))?;

    let status = resp.status();
    if status == 200 || status == 204 {
        println!("Unyanked {}@{} ✓", package, version);
        Ok(())
    } else {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        Err(format!("unyank failed (HTTP {}): {}", status, body))
    }
}

// ─── Delete ───────────────────────────────────────────────

/// Delete an entire package from the registry (author only).
pub fn delete_package(package: &str) -> Result<(), String> {
    let token = read_token()
        .ok_or("not logged in — run 'synx login' first")?;

    let name_owned = if package.starts_with('@') {
        package.strip_prefix('@').unwrap_or(package).to_lowercase()
    } else {
        package.to_lowercase()
    };
    let name = name_owned.as_str();

    // Confirm
    eprintln!("⚠  This will soft-delete @{} and yank ALL its versions.", name);
    eprintln!("  The package will be hidden from listings and no longer installable.");
    eprintln!("  You have 30 days to restore it before permanent deletion.");
    eprint!("  Type the full package name to confirm: ");
    std::io::stdout().flush().ok();
    let mut confirm = String::new();
    std::io::stdin().read_line(&mut confirm).unwrap_or(0);
    let confirm = confirm.trim();
    let expected = format!("@{}", name);
    if confirm != expected && confirm != name {
        return Err("delete aborted — name did not match".into());
    }

    let registry = registry_url();
    let url = format!("{}/packages/{}", registry, name);

    let resp = registry_agent(&registry).delete(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .call()
        .map_err(|e| format!("delete error: {}", e))?;

    let status = resp.status();
    if status == 200 || status == 204 {
        println!("Soft-deleted @{} ✓", name);
        println!("  Package hidden from listings. Restore within 30 days with 'synx restore @{}'.", name);
        Ok(())
    } else {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        Err(format!("delete failed (HTTP {}): {}", status, body))
    }
}

// ─── Create (scaffold) ───────────────────────────────────

/// Restore a soft-deleted package (within 30-day grace period).
pub fn restore_package(package: &str) -> Result<(), String> {
    let token = read_token()
        .ok_or("not logged in — run 'synx login' first")?;

    let name_owned = if package.starts_with('@') {
        package.strip_prefix('@').unwrap_or(package).to_lowercase()
    } else {
        package.to_lowercase()
    };
    let name = name_owned.as_str();

    let registry = registry_url();
    let url = format!("{}/packages/{}/restore", registry, name);

    let resp = registry_agent(&registry).post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send(&[] as &[u8])
        .map_err(|e| format!("restore error: {}", e))?;

    let status = resp.status();
    if status == 200 || status == 204 {
        println!("Restored @{} ✓", name);
        println!("  Package is now visible and all versions are unyanked.");
        Ok(())
    } else {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        Err(format!("restore failed (HTTP {}): {}", status, body))
    }
}

fn prompt(label: &str, default: &str) -> String {
    if default.is_empty() {
        eprint!("  {} ", label);
    } else {
        eprint!("  {} ({}): ", label, default);
    }
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).unwrap_or(0);
    let val = buf.trim().to_string();
    if val.is_empty() { default.to_string() } else { val }
}

fn prompt_choice(label: &str, options: &[&str], default: usize) -> usize {
    eprintln!("  {}:", label);
    for (i, opt) in options.iter().enumerate() {
        let marker = if i == default { ">" } else { " " };
        eprintln!("    {} {}. {}", marker, i + 1, opt);
    }
    eprint!("  Choice [{}]: ", default + 1);
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).unwrap_or(0);
    let val = buf.trim();
    if val.is_empty() {
        return default;
    }
    val.parse::<usize>().ok().map(|n| n.saturating_sub(1).min(options.len() - 1)).unwrap_or(default)
}

/// Interactively scaffold a new SYNX package.
pub fn create_package() -> Result<(), String> {
    println!();
    println!("  SYNX Package Creator");
    println!("  ====================");
    println!();

    let scope = prompt("Scope (@yourscope):", "@yourscope");
    let name = prompt("Package name:", "my-markers");
    let description = prompt("Description:", "My custom SYNX markers");
    let author = prompt("Author:", "yourname");
    let license = prompt("License:", "MIT");
    let pkg_type = prompt_choice("Package type", &["WASM Marker Package", "SYNX Config Package"], 0);

    let scope_clean = scope.strip_prefix('@').unwrap_or(&scope);
    let full_name = format!("@{}/{}", scope_clean, name);
    let dir_name = &name;

    let out_dir = PathBuf::from(dir_name);
    if out_dir.exists() {
        return Err(format!("directory '{}' already exists", dir_name));
    }

    fs::create_dir_all(out_dir.join("src"))
        .map_err(|e| format!("cannot create directory: {}", e))?;

    if pkg_type == 0 {
        // WASM Marker Package
        let crate_name = name.replace('-', "_");

        let cargo_toml = format!(
            r#"[package]
name = "{name}"
version = "1.0.0"
edition = "2021"
description = "{description}"
license = "{license}"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
serde_json = "1"
"#,
            name = name,
            description = description,
            license = license,
        );

        let lib_rs = r##"//! Custom SYNX WASM markers.
//!
//! Build:  cargo build --target wasm32-unknown-unknown --release
//!
//! ## ABI v1
//! - synx_alloc(size) → ptr
//! - synx_markers()   → packed(ptr, len)  — JSON array of names
//! - synx_apply(ptr, len) → packed(ptr, len)  — apply marker

use std::alloc::{alloc, Layout};

#[no_mangle]
pub extern "C" fn synx_alloc(size: i32) -> i32 {
    let layout = Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc(layout) as i32 }
}

fn write_output(s: &str) -> i64 {
    let bytes = s.as_bytes();
    let ptr = synx_alloc(bytes.len() as i32);
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()); }
    ((ptr as i64) << 32) | (bytes.len() as i64)
}

fn json_ok(value: &str) -> String {
    format!("{{\"{}\": {}}}", "value", serde_json::Value::String(value.to_string()))
}

fn json_err(msg: &str) -> String {
    format!("{{\"{}\": {}}}", "error", serde_json::Value::String(msg.to_string()))
}

// ✏️ Add your marker names here:
#[no_mangle]
pub extern "C" fn synx_markers() -> i64 {
    write_output(r#"["example"]"#)
}

// ✏️ Add your marker logic here:
#[no_mangle]
pub extern "C" fn synx_apply(in_ptr: i32, in_len: i32) -> i64 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(in_ptr as *const u8, in_len as usize);
        String::from_utf8_lossy(slice).into_owned()
    };

    let req: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => return write_output(&json_err(&format!("invalid JSON: {}", e))),
    };

    let marker = req["marker"].as_str().unwrap_or("");
    let value = req["value"].as_str().unwrap_or("");

    let result = match marker {
        "example" => json_ok(&format!("[example] {}", value)),
        _ => json_err(&format!("unknown marker: {}", marker)),
    };

    write_output(&result)
}
"##;

        let manifest = format!(
"name {}
version 1.0.0
description {}
author {}
license {}
main markers.wasm
synx-version >=3.6.0
category Markers
keywords
  - custom
  - markers
  - wasm
capabilities
  - string
synxignore
  - synx-pkg.synx
  - *.bat
  - *.sh
  - Cargo.toml
  - Cargo.lock
  - target/
  - src/
dependencies
", name, description, author, license);

        let build_bat = format!(
r#"@echo off
echo Building {} WASM...
cargo build --target wasm32-unknown-unknown --release
if errorlevel 1 (
    echo BUILD FAILED
    exit /b 1
)
copy /Y target\wasm32-unknown-unknown\release\{}.wasm markers.wasm
echo Done: markers.wasm
"#, full_name, crate_name);

        let build_sh = format!(
r#"#!/bin/bash
set -e
echo "Building {} WASM..."
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/{}.wasm markers.wasm
echo "Done: markers.wasm"
"#, full_name, crate_name);

        let readme = [
            format!("# {}", full_name),
            String::new(),
            description.clone(),
            String::new(),
            "## Build".to_string(),
            String::new(),
            "```bash".to_string(),
            "cargo build --target wasm32-unknown-unknown --release".to_string(),
            "```".to_string(),
            String::new(),
            "## Install".to_string(),
            String::new(),
            "```bash".to_string(),
            format!("synx install {}", full_name),
            "```".to_string(),
            String::new(),
            "## Usage".to_string(),
            String::new(),
            "```synx".to_string(),
            "!active".to_string(),
            format!("!use {}", full_name),
            String::new(),
            "key:example hello".to_string(),
            "```".to_string(),
            String::new(),
        ].join("\n");

        fs::write(out_dir.join("Cargo.toml"), cargo_toml).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("src/lib.rs"), lib_rs).map_err(|e| e.to_string())?;
        fs::write(out_dir.join(MANIFEST_NAME), manifest).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("build.bat"), build_bat).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("build.sh"), build_sh).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("README.md"), readme).map_err(|e| e.to_string())?;
    } else {
        // SYNX Config Package
        let manifest = format!(
"name {}
version 1.0.0
description {}
author {}
license {}
main src/main.synx
synx-version >=3.6.0
category Back-end
keywords
  - config
  - defaults
synxignore
  - synx-pkg.synx
dependencies
", name, description, author, license);

        let main_synx = format!(
"# {}
# Install: synx install {}
# Usage:   !use {}

!active

app_name {}
version 1.0.0
debug false
log_level info
port:env:default:3000 PORT
host:env:default:0.0.0.0 HOST
", full_name, full_name, full_name, name);

        let readme = [
            format!("# {}", full_name),
            String::new(),
            description.clone(),
            String::new(),
            "## Install".to_string(),
            String::new(),
            "```bash".to_string(),
            format!("synx install {}", full_name),
            "```".to_string(),
            String::new(),
            "## Usage".to_string(),
            String::new(),
            "```synx".to_string(),
            "!active".to_string(),
            format!("!use {}", full_name),
            String::new(),
            "my_key my_value".to_string(),
            "```".to_string(),
            String::new(),
        ].join("\n");

        fs::write(out_dir.join(MANIFEST_NAME), manifest).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("src/main.synx"), main_synx).map_err(|e| e.to_string())?;
        fs::write(out_dir.join("README.md"), readme).map_err(|e| e.to_string())?;
    }

    println!();
    println!("  Package created: ./{}/", dir_name);
    println!();
    println!("  Files:");
    for entry in walkdir(&out_dir) {
        println!("    {}", entry.strip_prefix(&out_dir).unwrap_or(&entry).display());
    }
    println!();
    if pkg_type == 0 {
        println!("  Next steps:");
        println!("    cd {}", dir_name);
        println!("    # Edit src/lib.rs — add your markers");
        println!("    build.bat   (Windows)");
        println!("    ./build.sh  (Linux/macOS)");
        println!("    synx publish");
    } else {
        println!("  Next steps:");
        println!("    cd {}", dir_name);
        println!("    # Edit src/main.synx — add your config");
        println!("    synx publish");
    }
    println!();
    Ok(())
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                files.extend(walkdir(&p));
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

// ─── Publish ──────────────────────────────────────────────

/// Prompt for a one-time token (used by publish when not logged in).
/// Returns the token and the nickname the registry knows it by.
fn prompt_token() -> Result<(String, Option<String>), String> {
    println!("Not logged in. Authenticate to publish:");
    println!();
    print!("Paste your token: ");
    std::io::stdout().flush().ok();

    let mut token = String::new();
    std::io::stdin()
        .read_line(&mut token)
        .map_err(|e| format!("cannot read input: {}", e))?;
    let token = token.trim().to_string();

    if token.is_empty() {
        return Err("no token provided".into());
    }

    // Verify token
    let registry = registry_url();
    let url = format!("{}/auth/whoami", registry);
    let mut nickname = None;
    match registry_agent(&registry).get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .call()
    {
        Ok(resp) if resp.status() == 200 => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(nick) = data["nickname"].as_str() {
                    println!("Authenticated as {}", nick);
                    nickname = Some(nick.to_string());
                }
            }
        }
        Ok(resp) => return Err(format!("invalid token (HTTP {})", resp.status())),
        Err(e) => return Err(format!("cannot reach registry: {}", e)),
    }

    Ok((token, nickname))
}

/// Publishes the package in `dir`. A literal `:secret` value in a packed
/// `.synx` file stops the publish unless `allow_secret_literals` is set.
pub fn publish(dir: &Path, allow_secret_literals: bool) -> Result<(), String> {
    // why: a token pasted at the prompt comes with its nickname; asking for
    // the saved one afterwards made the prompt a dead end.
    let (token, prompted_nickname) = match read_token() {
        Some(t) => (t, None),
        None => prompt_token()?,
    };

    // Resolve nickname for scoped name
    let nickname = read_nickname()
        .or(prompted_nickname)
        .ok_or("not logged in — run 'synx login' first (nickname not found)")?;

    let manifest_path = dir.join(MANIFEST_NAME);
    let manifest = Manifest::from_file(&manifest_path)?;
    manifest.validate_for_publish().map_err(|errs| errs.join("\n"))?;

    let full_name = manifest.full_name(&nickname);
    validate_registry_name(&full_name)?;

    // Pack first
    let report = pack_report(dir)?;
    print_pack_report(&report);
    let tarball_path = report.tarball.clone();
    let tarball_size = fs::metadata(&tarball_path)
        .map_err(|e| format!("cannot stat tarball: {}", e))?
        .len();

    // why: what is published stays public and is not deleted by a yank.
    if !report.secret_literals.is_empty() && !allow_secret_literals {
        let _ = fs::remove_file(&tarball_path);
        return Err(format!(
            "not published: {} literal :secret value(s) in the package (listed above).\n\
             Read them from the environment (`key:secret:env NAME`), or pass \
             --allow-secret-literals if they are placeholders.",
            report.secret_literals.len()
        ));
    }

    if tarball_size > MAX_PACKAGE_SIZE {
        let _ = fs::remove_file(&tarball_path);
        return Err(format!(
            "package too large ({} bytes, max {} bytes)",
            tarball_size, MAX_PACKAGE_SIZE
        ));
    }

    let tarball_data = fs::read(&tarball_path)
        .map_err(|e| format!("cannot read tarball: {}", e));
    // Clean up tarball: it is in memory now, and no failure below leaves it behind.
    let _ = fs::remove_file(&tarball_path);
    let tarball_data = tarball_data?;

    // Sign the tarball if a signing key is available
    let signature_hex;
    let public_key_hex;
    if let Some(signing_key) = read_signing_key() {
        let sig_bytes = signing_key.sign(&tarball_data);
        signature_hex = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        public_key_hex = signing_key.verify_key().to_hex();
    } else {
        signature_hex = String::new();
        public_key_hex = String::new();
    }

    let registry = registry_url();
    let url = format!("{}/packages", registry);

    // Send tarball as raw body with metadata in headers
    let mut req = registry_agent(&registry).post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .header("Content-Type", "application/gzip")
        .header("X-Package-Name", &full_name)
        .header("X-Package-Version", &manifest.version)
        .header("X-Package-Description", &manifest.description)
        .header("X-Package-License", &manifest.license)
        .header("X-Package-Main", &manifest.main)
        .header("X-Package-Synx-Version", manifest.synx_version.as_deref().unwrap_or(""))
        .header("X-Package-Category", "")
        .header("X-Package-Repository", manifest.repository.as_deref().unwrap_or(""))
        .header("X-Package-Keywords", &manifest.keywords.join(","));

    if !signature_hex.is_empty() {
        req = req
            .header("X-Package-Signature", &signature_hex)
            .header("X-Package-Public-Key", &public_key_hex);
    }

    let resp = req
        .send(&tarball_data[..])
        .map_err(|e| format!("publish error: {}", e))?;

    let status = resp.status();
    if status == 200 || status == 201 {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(int) = data["integrity"].as_str() {
                println!("integrity: {}", int);
            }
        }
        println!("Published {}@{} ✓", full_name, manifest.version);
        if !signature_hex.is_empty() {
            println!("  Signed with key: {}", &public_key_hex[..16]);
        }
        Ok(())
    } else {
        let err_body = resp.into_body().read_to_string().unwrap_or_default();
        Err(format!("publish failed (HTTP {}): {}", status, err_body))
    }
}

// ─── Login ────────────────────────────────────────────────

pub fn login() -> Result<(), String> {
    let registry = registry_url();
    println!("Registry: {}", registry);
    println!();
    // why: the registry accepts the token its own page issues; an API key
    // from the developer portal was answered with 401.
    println!("1. Visit {}/auth/cli-token", registry);
    println!("2. Log in with your Syndicate account");
    println!("3. Copy the token");
    println!();
    print!("Paste your token: ");
    std::io::stdout().flush().ok();

    let mut token = String::new();
    std::io::stdin()
        .read_line(&mut token)
        .map_err(|e| format!("cannot read input: {}", e))?;
    let token = token.trim().to_string();

    if token.is_empty() {
        return Err("no token provided".into());
    }

    // Verify token and fetch nickname from the registry
    print!("Verifying... ");
    std::io::stdout().flush().ok();

    let url = format!("{}/auth/whoami", registry);
    let nickname = match registry_agent(&registry).get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .call()
    {
        Ok(resp) if resp.status() == 200 => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(data) => {
                    data["nickname"]
                        .as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| "server returned no nickname".to_string())?
                }
                Err(_) => return Err("invalid response from registry".into()),
            }
        }
        Ok(resp) => {
            return Err(format!("invalid token (HTTP {})", resp.status()));
        }
        Err(e) => {
            return Err(format!("cannot reach registry: {}", e));
        }
    };

    println!("ok");
    println!();
    print!("Save token for future use? [Y/n]: ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("cannot read input: {}", e))?;
    let answer = answer.trim().to_lowercase();

    if answer.is_empty() || answer == "y" || answer == "yes" {
        save_token(&token, &nickname)?;
        let path = credentials_path();
        println!("Logged in as {} ✓", nickname);
        println!("Token saved to {}", path.display());
    } else {
        println!("Logged in as {} (token not saved)", nickname);
    }

    Ok(())
}

pub fn logout() -> Result<(), String> {
    remove_token()?;
    println!("Logged out ✓");
    Ok(())
}

pub fn whoami() -> Result<(), String> {
    match read_token() {
        Some(token) => {
            // Try to verify with registry
            let registry = registry_url();
            let url = format!("{}/auth/whoami", registry);
            match registry_agent(&registry).get(&url)
                .header("Authorization", &format!("Bearer {}", token))
                .call()
            {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_body().read_to_string().unwrap_or_default();
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(user) = data["nickname"].as_str() {
                            println!("{}", user);
                            return Ok(());
                        }
                    }
                    // Fallback to local nickname
                    match read_nickname() {
                        Some(nick) => println!("{}", nick),
                        None => println!("(authenticated, nickname unavailable)"),
                    }
                }
                _ => {
                    match read_nickname() {
                        Some(nick) => println!("{} (offline)", nick),
                        None => println!("(token saved, registry unavailable)"),
                    }
                }
            }
            Ok(())
        }
        None => {
            println!("Not logged in. Run 'synx login' first.");
            Ok(())
        }
    }
}

pub fn account_info() -> Result<(), String> {
    match read_token() {
        Some(token) => {
            let nickname = read_nickname().unwrap_or_else(|| "(unknown)".to_string());
            let registry = registry_url();
            println!("nickname: {}", nickname);
            println!("registry: {}", registry);

            let url = format!("{}/auth/whoami", registry);
            match registry_agent(&registry).get(&url)
                .header("Authorization", &format!("Bearer {}", token))
                .call()
            {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_body().read_to_string().unwrap_or_default();
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(plan) = data["plan"].as_str() {
                            println!("plan: {}", plan);
                        }
                    }
                    println!("status: authenticated ✓");
                }
                _ => {
                    println!("status: token saved (registry unavailable)");
                }
            }
            Ok(())
        }
        None => {
            println!("Not logged in. Run 'synx login' first.");
            Ok(())
        }
    }
}

// ─── Update ───────────────────────────────────────────────

pub fn update(project_root: &Path, package: Option<&str>) -> Result<(), String> {
    update_from(&registry_url(), project_root, package)
}

/// What `synx update` did with one lock entry.
#[derive(Debug, PartialEq, Eq)]
enum UpdateOutcome {
    Updated(String),
    UpToDate,
    Skipped(&'static str),
}

/// Moves lock entries to the newest compatible version and says what changed.
///
/// safety: this reinstalled every entry at whatever the registry called
/// latest — a new major included, packages installed from a local path
/// included (anyone who took the same name in the registry replaced them) —
/// and reported a failure as a warning with exit code 0.
fn update_from(registry: &str, project_root: &Path, package: Option<&str>) -> Result<(), String> {
    let lock = LockFile::read(project_root);
    if lock.entries.is_empty() {
        println!("No packages to update.");
        return Ok(());
    }

    let entries: Vec<LockEntry> = match package {
        Some(name) => {
            let entry = lock
                .find(name)
                .ok_or_else(|| format!("package '{}' is not in synx.lock", name))?;
            vec![entry.clone()]
        }
        None => lock.entries.clone(),
    };

    let agent = registry_agent(registry);
    let mut failed = 0usize;
    for entry in &entries {
        match update_one(&agent, registry, project_root, entry) {
            Ok(UpdateOutcome::Updated(to)) => println!("updated {}: {} → {}", terminal_safe(&entry.name), terminal_safe(&entry.version), to),
            Ok(UpdateOutcome::UpToDate) => println!("{}@{} is up to date", terminal_safe(&entry.name), terminal_safe(&entry.version)),
            Ok(UpdateOutcome::Skipped(why)) => println!("skipped {} ({})", terminal_safe(&entry.name), why),
            Err(e) => {
                eprintln!("error: cannot update {}: {}", terminal_safe(&entry.name), e);
                failed += 1;
            }
        }
    }

    if failed > 0 {
        return Err(format!("{} package(s) not updated", failed));
    }
    Ok(())
}

fn update_one(
    agent: &ureq::Agent,
    registry: &str,
    project_root: &Path,
    entry: &LockEntry,
) -> Result<UpdateOutcome, String> {
    if entry.integrity == LOCAL_INTEGRITY {
        return Ok(UpdateOutcome::Skipped("installed from a local path"));
    }
    // safety: the names come from synx.lock, a file of the repository.
    validate_registry_name(&entry.name)?;
    validate_version(&entry.version).map_err(|e| format!("synx.lock: {}", e))?;

    let api_name = entry.name.strip_prefix('@').unwrap_or(&entry.name);
    let url = format!("{}/packages/{}", registry, api_name);
    let meta = fetch_json(agent, &url, &entry.name)?;
    let Some(best) = pick_update(&meta, &entry.version) else {
        return Ok(UpdateOutcome::UpToDate);
    };
    if parse_semver(&best.version) <= parse_semver(&entry.version) {
        return Ok(UpdateOutcome::UpToDate);
    }
    install_picked(agent, registry, project_root, &entry.name, &best, Some(entry))?;
    Ok(UpdateOutcome::Updated(best.version))
}

// ─── Helpers ──────────────────────────────────────────────

fn parse_package_spec<'a>(spec: &'a str, version: Option<&'a str>) -> (String, Option<String>) {
    if let Some(ver) = version {
        return (spec.to_string(), Some(ver.to_string()));
    }
    // Check for @scope/name@version format: the first `@` past the scope's own.
    // why: `spec[1..]` panicked on an empty argument and on one that starts
    // with a multi-byte character.
    let after_first = spec.char_indices().nth(1).map(|(i, _)| i);
    if let Some(pos) = after_first.and_then(|start| spec[start..].find('@').map(|at| start + at)) {
        let name = spec[..pos].to_string();
        let ver = spec[pos + 1..].to_string();
        return (name, Some(ver));
    }
    (spec.to_string(), None)
}

// ─── Semver utilities ─────────────────────────────────────

/// Parse a semver string "x.y.z" into (major, minor, patch).
fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let clean = version.trim().trim_start_matches('v').trim_start_matches('V');
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let major = parts[0].parse::<u64>().ok()?;
    let minor = parts[1].parse::<u64>().ok()?;
    // Strip pre-release suffix (e.g. "1-beta")
    let p = parts[2].split('-').next().unwrap_or("0");
    let patch = p.parse::<u64>().ok()?;
    Some((major, minor, patch))
}

/// Check if `candidate` satisfies the `requirement` range.
/// Supports: exact ("1.2.3"), caret ("^1.2.0"), tilde ("~1.2.0"), wildcard ("1.*"), operators (">=1.0.0").
fn version_satisfies(requirement: &str, candidate: &str) -> bool {
    let req = requirement.trim();
    let cand = match parse_semver(candidate) {
        Some(v) => v,
        None => return false,
    };

    // Caret range: ^1.2.3 means >=1.2.3 and <2.0.0 (major locked)
    if let Some(rest) = req.strip_prefix('^') {
        if let Some(rv) = parse_semver(rest) {
            if rv.0 == 0 {
                // ^0.x.y — lock minor: >=0.x.y, <0.(x+1).0
                return cand.0 == rv.0 && cand.1 == rv.1 && cand.2 >= rv.2;
            }
            return cand.0 == rv.0
                && (cand.1 > rv.1 || (cand.1 == rv.1 && cand.2 >= rv.2));
        }
        return false;
    }

    // Tilde range: ~1.2.3 means >=1.2.3 and <1.3.0 (minor locked)
    if let Some(rest) = req.strip_prefix('~') {
        if let Some(rv) = parse_semver(rest) {
            return cand.0 == rv.0 && cand.1 == rv.1 && cand.2 >= rv.2;
        }
        return false;
    }

    // Wildcard: "*", "1.*", or "1.2.*"
    if req == "*" {
        return true;
    }
    if req.contains('*') {
        let parts: Vec<&str> = req.split('.').collect();
        if let Ok(major) = parts[0].parse::<u64>() {
            if parts.len() >= 2 && parts[1] != "*" {
                if let Ok(minor) = parts[1].parse::<u64>() {
                    return cand.0 == major && cand.1 == minor;
                }
            }
            return cand.0 == major;
        }
        return false;
    }

    // Operator ranges: >=, <=, >, <
    if let Some(rest) = req.strip_prefix(">=") {
        if let Some(rv) = parse_semver(rest) {
            return cand >= rv;
        }
        return false;
    }
    if let Some(rest) = req.strip_prefix("<=") {
        if let Some(rv) = parse_semver(rest) {
            return cand <= rv;
        }
        return false;
    }
    if let Some(rest) = req.strip_prefix('>') {
        if let Some(rv) = parse_semver(rest) {
            return cand > rv;
        }
        return false;
    }
    if let Some(rest) = req.strip_prefix('<') {
        if let Some(rv) = parse_semver(rest) {
            return cand < rv;
        }
        return false;
    }

    // Exact match
    if let Some(rv) = parse_semver(req) {
        return cand == rv;
    }
    false
}

/// Detect conflicting versions in the lock file.
/// Returns a list of (package_name, locked_version, requested_version) conflicts.
pub fn check_conflicts(lock: &LockFile, name: &str, requested: &str) -> Option<String> {
    if let Some(entry) = lock.find(name) {
        // If a version requirement is given, check compatibility
        if !requested.is_empty() && !version_satisfies(requested, &entry.version) {
            return Some(format!(
                "version conflict for '{}': locked at {}, but {} requested\n\
                 (`synx update {}` moves to the newest compatible version; \
                 to switch to another one, `synx uninstall {}` first)",
                name, entry.version, requested, name, name
            ));
        }
    }
    None
}

// ─── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_manifest_parse_minimal() {
        let text = "name @test/pkg\nversion 1.0.0\n";
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.name, "@test/pkg");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.main, "src/main.synx");
    }

    #[test]
    fn test_manifest_parse_full() {
        let text = r#"name @aperture/synx-defaults
version 2.1.0
description Test package
author aperture
license MIT
main src/entry.synx
synx-version >=3.6.0
keywords
  - config
  - defaults
dependencies
  - @other/base ^1.0.0
"#;
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.name, "@aperture/synx-defaults");
        assert_eq!(m.version, "2.1.0");
        assert_eq!(m.main, "src/entry.synx");
        assert_eq!(m.keywords, vec!["config", "defaults"]);
        assert_eq!(m.dependencies.get("@other/base"), Some(&"^1.0.0".to_string()));
    }

    #[test]
    fn test_manifest_parse_legacy_entry() {
        let text = "name @test/pkg\nversion 1.0.0\nentry src/custom.synx\n";
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.main, "src/custom.synx");
    }

    #[test]
    fn test_manifest_missing_name() {
        let text = "version 1.0.0\n";
        assert!(Manifest::parse(text).is_err());
    }

    #[test]
    fn test_manifest_missing_version() {
        let text = "name @test/pkg\n";
        assert!(Manifest::parse(text).is_err());
    }

    #[test]
    fn test_manifest_validate_for_publish() {
        let m = Manifest {
            name: "@test/pkg".into(),
            version: "1.0.0".into(),
            description: "A test".into(),
            author: "test".into(),
            license: "MIT".into(),
            main: "src/main.synx".into(),
            synx_version: None,
            keywords: vec![],
            files: vec![],
            synxignore: vec![],
            dependencies: HashMap::new(),
            repository: None,
        };
        assert!(m.validate_for_publish().is_ok());
    }

    #[test]
    fn test_manifest_validate_bad_name() {
        let m = Manifest {
            name: "no-scope".into(),
            version: "1.0.0".into(),
            description: "A test".into(),
            author: "test".into(),
            license: "MIT".into(),
            main: "src/main.synx".into(),
            synx_version: None,
            keywords: vec![],
            files: vec![],
            synxignore: vec![],
            dependencies: HashMap::new(),
            repository: None,
        };
        // "no-scope" passes validate_for_publish (scope added later via full_name)
        assert!(m.validate_for_publish().is_ok());
    }

    #[test]
    fn test_lockfile_roundtrip() {
        let mut lock = LockFile::default();
        lock.upsert("@test/a", "1.0.0", "sha256-abc123");
        lock.upsert("@test/b", "2.0.0", "sha256-def456");

        let tmp = std::env::temp_dir().join("synx-lock-test");
        fs::create_dir_all(&tmp).unwrap();
        lock.write(&tmp).unwrap();

        let loaded = LockFile::read(&tmp);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.find("@test/a").unwrap().version, "1.0.0");
        assert_eq!(loaded.find("@test/b").unwrap().integrity, "sha256-def456");

        // Update existing
        let mut lock2 = loaded;
        lock2.upsert("@test/a", "1.1.0", "sha256-updated");
        lock2.write(&tmp).unwrap();

        let reloaded = LockFile::read(&tmp);
        assert_eq!(reloaded.find("@test/a").unwrap().version, "1.1.0");

        // Remove
        let mut lock3 = reloaded;
        lock3.remove("@test/b");
        assert_eq!(lock3.entries.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_package_spec_simple() {
        let (name, ver) = parse_package_spec("@user/pkg", None);
        assert_eq!(name, "@user/pkg");
        assert!(ver.is_none());
    }

    #[test]
    fn test_parse_package_spec_with_version() {
        let (name, ver) = parse_package_spec("@user/pkg@1.2.0", None);
        assert_eq!(name, "@user/pkg");
        assert_eq!(ver, Some("1.2.0".to_string()));
    }

    #[test]
    fn test_parse_package_spec_explicit_version() {
        let (name, ver) = parse_package_spec("@user/pkg", Some("3.0.0"));
        assert_eq!(name, "@user/pkg");
        assert_eq!(ver, Some("3.0.0".to_string()));
    }

    #[test]
    fn test_sha256_bytes() {
        let hash = sha256_bytes(b"hello");
        assert!(hash.starts_with("sha256-"));
        assert_eq!(hash.len(), 7 + 64); // "sha256-" + 64 hex chars
    }

    #[test]
    fn test_pack_creates_tarball() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap();
        let pkg_dir = workspace_root.join("synx_packages/@assynx/text-tools");
        let result = pack_report(&pkg_dir);
        assert!(result.is_ok(), "pack failed: {:?}", result.err());
        let report = result.unwrap();
        let (path, integrity) = (report.tarball, report.integrity);
        assert!(path.exists());
        assert!(integrity.starts_with("sha256-"));
        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_install_local_and_uninstall() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap();
        let source = workspace_root.join("synx_packages/@assynx/text-tools");

        let tmp_project = std::env::temp_dir().join("synx-install-test");
        let _ = fs::remove_dir_all(&tmp_project);
        fs::create_dir_all(&tmp_project).unwrap();

        // Install
        let result = install_local(&tmp_project, &source);
        assert!(result.is_ok(), "install_local failed: {:?}", result.err());

        // Package should be in synx_packages/@assynx/text-tools (scope inferred from dir)
        let installed = tmp_project.join("synx_packages/@assynx/text-tools/synx-pkg.synx");
        assert!(installed.exists());

        // Lock should exist with scoped name
        let lock = LockFile::read(&tmp_project);
        assert!(lock.find("@assynx/text-tools").is_some());

        // Uninstall
        let result = uninstall(&tmp_project, "@assynx/text-tools");
        assert!(result.is_ok());
        assert!(!installed.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&tmp_project);
    }

    #[test]
    fn test_real_manifests_parse() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap();

        // @assynx/text-tools
        let m = Manifest::from_file(
            &workspace_root.join("synx_packages/@assynx/text-tools/synx-pkg.synx")
        ).unwrap();
        assert_eq!(m.name, "text-tools");
        assert_eq!(m.main, "markers.wasm");
    }

    #[test]
    fn test_template_manifest() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap();
        let m = Manifest::from_file(
            &workspace_root.join("synx_packages/template/synx-pkg.synx")
        ).unwrap();
        assert_eq!(m.name, "@yourscope/my-markers");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.main, "markers.wasm");
    }

    // ─── Semver tests ──────────────────────────────────────

    #[test]
    fn test_parse_semver_basic() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.0.0"), Some((0, 0, 0)));
        assert_eq!(parse_semver("10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn test_parse_semver_with_prerelease() {
        assert_eq!(parse_semver("1.2.3-alpha"), Some((1, 2, 3)));
        assert_eq!(parse_semver("2.0.0-rc.1"), Some((2, 0, 0)));
    }

    #[test]
    fn test_parse_semver_invalid() {
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("1.2"), None);
        assert_eq!(parse_semver("not-a-version"), None);
    }

    #[test]
    fn test_version_satisfies_exact() {
        assert!(version_satisfies("1.2.3", "1.2.3"));
        assert!(!version_satisfies("1.2.3", "1.2.4"));
    }

    #[test]
    fn test_version_satisfies_caret() {
        // ^1.2.3 → >=1.2.3, <2.0.0
        assert!(version_satisfies("^1.2.3", "1.2.3"));
        assert!(version_satisfies("^1.2.3", "1.9.9"));
        assert!(!version_satisfies("^1.2.3", "2.0.0"));
        assert!(!version_satisfies("^1.2.3", "1.2.2"));
        // ^0.2.3 → >=0.2.3, <0.3.0
        assert!(version_satisfies("^0.2.3", "0.2.5"));
        assert!(!version_satisfies("^0.2.3", "0.3.0"));
    }

    #[test]
    fn test_version_satisfies_tilde() {
        // ~1.2.3 → >=1.2.3, <1.3.0
        assert!(version_satisfies("~1.2.3", "1.2.5"));
        assert!(!version_satisfies("~1.2.3", "1.3.0"));
    }

    #[test]
    fn test_version_satisfies_wildcard() {
        assert!(version_satisfies("*", "5.0.0"));
        assert!(version_satisfies("*", "0.0.1"));
    }

    #[test]
    fn test_version_satisfies_comparison() {
        assert!(version_satisfies(">=1.0.0", "1.0.0"));
        assert!(version_satisfies(">=1.0.0", "2.0.0"));
        assert!(!version_satisfies(">=1.0.0", "0.9.9"));
        assert!(version_satisfies("<2.0.0", "1.9.9"));
        assert!(!version_satisfies("<2.0.0", "2.0.0"));
    }

    #[test]
    fn test_check_conflicts_no_conflict() {
        let lock = LockFile::default();
        assert!(check_conflicts(&lock, "@test/pkg", "^1.0.0").is_none());
    }

    #[test]
    fn test_check_conflicts_compatible() {
        let mut lock = LockFile::default();
        lock.upsert("@test/pkg", "1.5.0", "sha256-abc");
        assert!(check_conflicts(&lock, "@test/pkg", "^1.0.0").is_none());
    }

    #[test]
    fn test_check_conflicts_incompatible() {
        let mut lock = LockFile::default();
        lock.upsert("@test/pkg", "2.0.0", "sha256-abc");
        assert!(check_conflicts(&lock, "@test/pkg", "^1.0.0").is_some());
    }

    // ─── E2E Registry tests (skip if server unreachable) ──

    fn registry_is_reachable() -> bool {
        let url = format!("{}/packages?q=synx", super::registry_url());
        matches!(ureq::get(&url).call(), Ok(resp) if resp.status() == 200)
    }

    #[test]
    fn test_e2e_search_api() {
        if !registry_is_reachable() {
            eprintln!("SKIP: registry unreachable");
            return;
        }
        let result = super::search("synx");
        assert!(result.is_ok(), "search failed: {:?}", result.err());
    }

    #[test]
    fn test_e2e_info_api() {
        if !registry_is_reachable() {
            eprintln!("SKIP: registry unreachable");
            return;
        }
        // Try to get info for a known package — might return 404 if not published yet
        let result = super::info("@assynx/text-tools");
        // Both Ok (found) and Err (404/not found) are acceptable —
        // we just verify the API call doesn't panic or crash
        let _ = result;
    }

    #[test]
    fn test_e2e_install_from_registry() {
        if !registry_is_reachable() {
            eprintln!("SKIP: registry unreachable");
            return;
        }
        let tmp = std::env::temp_dir().join("synx-e2e-install-test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Try to install a known package — if it's not in the registry yet,
        // we still verify the HTTP round-trip works without panicking
        let result = super::install(&tmp, "@assynx/text-tools", None);
        // The install might fail with "not found" if not yet published,
        // but it must NOT panic
        if let Ok(()) = result {
            // If it succeeded, verify the files landed
            let manifest = tmp.join("synx_packages/@assynx/text-tools/synx-pkg.synx");
            assert!(manifest.exists(), "manifest should exist after install");
            let lock = LockFile::read(&tmp);
            assert!(lock.find("@assynx/text-tools").is_some());
        }

        let _ = fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
#[path = "pkg_security_tests.rs"]
mod security_tests;
