//! SYNX Package Management — CLI operations for install, publish, login, etc.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};

// ─── Constants ────────────────────────────────────────────

const DEFAULT_REGISTRY: &str = "https://synx.aperturesyndicate.com/api";
const PACKAGES_DIR: &str = "synx_packages";
const MANIFEST_NAME: &str = "synx-pkg.synx";
const LOCK_FILE: &str = "synx.lock";
const MAX_PACKAGE_SIZE: u64 = 5 * 1024 * 1024; // 5 MiB

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

#[derive(Debug, Clone)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub integrity: String,
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
        let mut current_name = String::new();
        let mut current_version = String::new();
        let mut current_integrity = String::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                // Flush current entry
                if !current_name.is_empty() {
                    entries.push(LockEntry {
                        name: std::mem::take(&mut current_name),
                        version: std::mem::take(&mut current_version),
                        integrity: std::mem::take(&mut current_integrity),
                    });
                }
                continue;
            }
            if trimmed.starts_with('@') && !trimmed.contains(' ') {
                // Package name line
                if !current_name.is_empty() {
                    entries.push(LockEntry {
                        name: std::mem::take(&mut current_name),
                        version: std::mem::take(&mut current_version),
                        integrity: std::mem::take(&mut current_integrity),
                    });
                }
                current_name = trimmed.to_string();
            } else if let Some((key, val)) = trimmed.split_once(' ') {
                match key {
                    "version" => current_version = val.trim().to_string(),
                    "integrity" => current_integrity = val.trim().to_string(),
                    _ => {}
                }
            }
        }
        // Flush last entry
        if !current_name.is_empty() {
            entries.push(LockEntry {
                name: current_name,
                version: current_version,
                integrity: current_integrity,
            });
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
            out.push('\n');
        }
        fs::write(&lock_path, &out)
            .map_err(|e| format!("cannot write {}: {}", lock_path.display(), e))
    }

    /// Find entry by name.
    pub fn find(&self, name: &str) -> Option<&LockEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Add or update an entry.
    pub fn upsert(&mut self, name: &str, version: &str, integrity: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.version = version.to_string();
            entry.integrity = integrity.to_string();
        } else {
            self.entries.push(LockEntry {
                name: name.to_string(),
                version: version.to_string(),
                integrity: integrity.to_string(),
            });
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
    fs::write(&path, hex.as_bytes())
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

pub fn save_token(token: &str, nickname: &str) -> Result<(), String> {
    let dir = credentials_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;

    let now = chrono_now_rfc3339();
    let content = format!(
        "[default]\ntoken = \"{}\"\nnickname = \"{}\"\nregistry = \"{}\"\ncreated = \"{}\"\n",
        token, nickname, registry_url(), now
    );

    let path = credentials_path();
    fs::write(&path, &content)
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;

    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
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

pub fn pack(dir: &Path) -> Result<(PathBuf, String), String> {
    let manifest_path = dir.join(MANIFEST_NAME);
    let manifest = Manifest::from_file(&manifest_path)?;
    manifest.validate_for_publish().map_err(|errs| errs.join("\n"))?;

    // Clean up old tarballs from previous packs
    let short = manifest.short_name();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(short) && name.ends_with(".tar.gz") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let tarball_name = format!(
        "{}-{}.tar.gz",
        manifest.short_name(),
        manifest.version
    );
    let tarball_path = dir.join(&tarball_name);

    let tar_file = fs::File::create(&tarball_path)
        .map_err(|e| format!("cannot create {}: {}", tarball_path.display(), e))?;
    let enc = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
    let mut tar_builder = tar::Builder::new(enc);

    // Always include the manifest
    tar_builder
        .append_path_with_name(dir.join(MANIFEST_NAME), MANIFEST_NAME)
        .map_err(|e| format!("tar error: {}", e))?;

    // Include README if present
    let readme = dir.join("README.md");
    if readme.is_file() {
        tar_builder
            .append_path_with_name(&readme, "README.md")
            .map_err(|e| format!("tar error: {}", e))?;
    }

    // Collect files to include
    // Collect files to include — directories are expanded recursively
    let files_to_pack = if !manifest.files.is_empty() {
        let mut collected = Vec::new();
        for f in &manifest.files {
            let path = dir.join(f);
            if path.is_dir() {
                // Directory entry: recursively include all files inside
                collected.extend(collect_all_files(&path));
            } else if path.is_file() {
                collected.push(path);
            }
        }
        collected
    } else {
        // Default: all files under src/ (any type, not just .synx)
        let src_dir = dir.join("src");
        if src_dir.is_dir() {
            collect_all_files(&src_dir)
        } else {
            collect_synx_files(dir)
        }
    };

    // Also include the main file (e.g. markers.wasm) if present and not in `files`
    let main_path = dir.join(&manifest.main);

    // Enforce source code presence: at least one non-binary source file required
    let source_exts = ["rs", "synx", "js", "ts", "py", "go", "kt", "swift", "c", "cpp", "h", "mojo"];
    let has_source = files_to_pack.iter().any(|f| {
        f.extension()
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

    for file_path in &files_to_pack {
        if !file_path.is_file() {
            continue;
        }
        let rel = file_path
            .strip_prefix(dir)
            .unwrap_or(file_path);
        let rel_str = rel.to_string_lossy();
        if synxignore_matches(&rel_str, &ignore_patterns) {
            continue;
        }
        tar_builder
            .append_path_with_name(file_path, rel)
            .map_err(|e| format!("tar error: {}", e))?;
    }

    // Include main entry if it's not already covered by files list and not ignored
    if main_path.is_file() {
        let main_rel = main_path.strip_prefix(dir).unwrap_or(&main_path);
        let main_rel_str = main_rel.to_string_lossy();
        if !synxignore_matches(&main_rel_str, &ignore_patterns) {
            let already_packed = files_to_pack.iter().any(|f| f == &main_path);
            if !already_packed {
                tar_builder
                    .append_path_with_name(&main_path, main_rel)
                    .map_err(|e| format!("tar error: {}", e))?;
            }
        }
    }

    tar_builder.finish().map_err(|e| format!("tar finish error: {}", e))?;

    let integrity = sha256_file(&tarball_path)?;
    Ok((tarball_path, integrity))
}

fn collect_synx_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_synx_files(&path));
            } else if path.extension().map_or(false, |e| e == "synx") {
                files.push(path);
            }
        }
    }
    files
}

/// Recursively collect ALL files under a directory.
fn collect_all_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_all_files(&path));
            } else {
                files.push(path);
            }
        }
    }
    files
}

// ─── Install ──────────────────────────────────────────────

/// Install a package from the registry into synx_packages/
pub fn install(project_root: &Path, package: &str, version: Option<&str>) -> Result<(), String> {
    let (name, requested_version) = parse_package_spec(package, version);

    // Validate package name format
    if !name.starts_with('@') || !name.contains('/') {
        return Err(format!(
            "invalid package name '{}': must be @scope/name",
            name
        ));
    }

    // Check for version conflicts with existing lock file
    if let Some(req) = &requested_version {
        let lock = LockFile::read(project_root);
        if let Some(conflict) = check_conflicts(&lock, &name, req) {
            return Err(conflict);
        }
    }

    let registry = registry_url();

    // Fetch package metadata from registry
    // Route: GET /api/packages/:scope/:name or GET /api/packages/:scope/:name/:version
    // Note: name is @scope/short, API expects /packages/scope/short
    let api_name = name.strip_prefix('@').unwrap_or(&name);
    let url = match &requested_version {
        Some(v) => format!("{}/packages/{}/{}", registry, api_name, v),
        None => format!("{}/packages/{}", registry, api_name),
    };

    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("registry error: {}", e))?;

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

    let meta: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("invalid registry response: {}", e))?;

    // When fetching full package info (no version), pick latest from versions array
    let (version, tarball_url_str, integrity_str);
    #[allow(unused_assignments)]
    let mut signature_str = String::new();
    #[allow(unused_assignments)]
    let mut public_key_str = String::new();
    if let Some(ver_str) = meta["version"].as_str() {
        // Direct version response
        version = ver_str.to_string();
        tarball_url_str = meta["tarballUrl"].as_str().unwrap_or("").to_string();
        integrity_str = meta["integrity"].as_str().unwrap_or("").to_string();
        signature_str = meta["signature"].as_str().unwrap_or("").to_string();
        public_key_str = meta["publicKey"].as_str().unwrap_or("").to_string();
    } else if let Some(versions) = meta["versions"].as_array() {
        // Full package info — pick first non-yanked version
        let v = versions.iter()
            .find(|v| !v["yanked"].as_bool().unwrap_or(false))
            .ok_or("no available versions")?;
        version = v["version"].as_str().unwrap_or("?").to_string();
        tarball_url_str = v["tarballUrl"].as_str().unwrap_or("").to_string();
        integrity_str = v["integrity"].as_str().unwrap_or("").to_string();
        signature_str = v["signature"].as_str().unwrap_or("").to_string();
        public_key_str = v["publicKey"].as_str().unwrap_or("").to_string();
    } else {
        return Err("unexpected registry response format".to_string());
    }
    let tarball_url = &tarball_url_str;
    let integrity = &integrity_str;

    // Download tarball
    let tar_resp = ureq::get(tarball_url)
        .call()
        .map_err(|e| format!("download error: {}", e))?;

    let mut tarball_data = Vec::new();
    tar_resp
        .into_body()
        .as_reader()
        .read_to_end(&mut tarball_data)
        .map_err(|e| format!("download read error: {}", e))?;

    // Verify integrity
    if !integrity.is_empty() {
        let actual = sha256_bytes(&tarball_data);
        if actual != *integrity {
            return Err(format!(
                "integrity check failed: expected {}, got {}",
                integrity, actual
            ));
        }
    }

    // Verify signature if present
    if !signature_str.is_empty() && !public_key_str.is_empty() {
        let verify_key = synx_core::signing::VerifyKey::from_hex(&public_key_str)
            .map_err(|e| format!("invalid public key: {}", e))?;
        let sig_bytes: Result<Vec<u8>, String> = (0..signature_str.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&signature_str[i..i + 2], 16)
                    .map_err(|_| "invalid signature hex".to_string())
            })
            .collect();
        let sig_bytes = sig_bytes?;
        match verify_key.verify(&tarball_data, &sig_bytes) {
            Ok(true) => println!("  Signature verified ✓"),
            Ok(false) => return Err("signature verification failed — package may be tampered".to_string()),
            Err(e) => return Err(format!("signature error: {}", e)),
        }
    }

    // Check size
    if tarball_data.len() as u64 > MAX_PACKAGE_SIZE {
        return Err(format!(
            "package too large ({} bytes, max {} bytes)",
            tarball_data.len(),
            MAX_PACKAGE_SIZE
        ));
    }

    // Extract to synx_packages/@scope/name/
    let pkg_dir = project_root.join(PACKAGES_DIR).join(&name);
    if pkg_dir.exists() {
        fs::remove_dir_all(&pkg_dir)
            .map_err(|e| format!("cannot remove old package: {}", e))?;
    }
    fs::create_dir_all(&pkg_dir)
        .map_err(|e| format!("cannot create {}: {}", pkg_dir.display(), e))?;

    let cursor = std::io::Cursor::new(&tarball_data);
    let dec = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(dec);
    archive
        .unpack(&pkg_dir)
        .map_err(|e| format!("extract error: {}", e))?;

    // Update synx.lock
    let actual_integrity = sha256_bytes(&tarball_data);
    let mut lock = LockFile::read(project_root);
    lock.upsert(&name, &version, &actual_integrity);
    lock.write(project_root)?;

    println!("Installed {}@{} ✓", name, version);
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

    let pkg_dir = project_root.join(PACKAGES_DIR).join(&full_name);
    if pkg_dir.exists() {
        fs::remove_dir_all(&pkg_dir)
            .map_err(|e| format!("cannot remove old package: {}", e))?;
    }

    copy_dir_recursive(source_dir, &pkg_dir)?;

    // Update synx.lock
    let mut lock = LockFile::read(project_root);
    lock.upsert(&full_name, &manifest.version, "local");
    lock.write(project_root)?;

    println!("Installed {} (local) ✓", full_name);
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("cannot create {}: {}", dst.display(), e))?;
    for entry in fs::read_dir(src)
        .map_err(|e| format!("cannot read {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| format!("read dir error: {}", e))?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip build artifacts
        if name_str == "target" || name_str.ends_with(".tar.gz") {
            continue;
        }

        let dest = dst.join(&name);
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            fs::copy(&path, &dest)
                .map_err(|e| format!("cannot copy {} → {}: {}", path.display(), dest.display(), e))?;
        }
    }
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

    let pkg_dir = project_root.join(PACKAGES_DIR).join(&name);
    if !pkg_dir.exists() {
        return Err(format!("package '{}' is not installed", name));
    }

    fs::remove_dir_all(&pkg_dir)
        .map_err(|e| format!("cannot remove {}: {}", pkg_dir.display(), e))?;

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
            println!("  {}@{}{}", full_name, version, lock_status);
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
    let resp = ureq::get(&url)
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
    println!("{}@{}", package, latest_ver);
    if let Some(desc) = meta["description"].as_str() {
        println!("{}", desc);
    }
    if let Some(author) = meta["author"].as_str() {
        println!("author: {}", author);
    }
    if let Some(license) = meta["license"].as_str() {
        println!("license: {}", license);
    }
    if let Some(versions) = meta["versions"].as_array() {
        println!("versions:");
        for v in versions {
            let ver = v["version"].as_str().unwrap_or("?");
            let yanked = if v["yanked"].as_bool().unwrap_or(false) { " (yanked)" } else { "" };
            let dl = v["downloads"].as_u64().unwrap_or(0);
            println!("  {} — {} downloads{}", ver, dl, yanked);
        }
    }

    Ok(())
}

fn print_manifest_info(m: &Manifest, local: bool) {
    println!("{}@{}{}", m.name, m.version, if local { " (local)" } else { "" });
    if !m.description.is_empty() {
        println!("{}", m.description);
    }
    println!("author:  {}", m.author);
    println!("license: {}", m.license);
    println!("main:    {}", m.main);
    if let Some(ref sv) = m.synx_version {
        println!("synx:    {}", sv);
    }
    if !m.keywords.is_empty() {
        println!("keywords: {}", m.keywords.join(", "));
    }
    if !m.dependencies.is_empty() {
        println!("dependencies:");
        for (dep, ver) in &m.dependencies {
            println!("  {} {}", dep, ver);
        }
    }
}

// ─── Search ───────────────────────────────────────────────

pub fn search(query: &str) -> Result<(), String> {
    let registry = registry_url();
    let url = format!("{}/packages?q={}", registry, query);
    let resp = ureq::get(&url)
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
            println!("No packages found for '{}'.", query);
            return Ok(());
        }
        println!("Found {} packages:", total);
        println!();
        for pkg in packages {
            let name = pkg["name"].as_str().unwrap_or("?");
            let version = pkg["latestVersion"].as_str().unwrap_or("?");
            let desc = pkg["description"].as_str().unwrap_or("");
            let dl = pkg["totalDownloads"].as_u64().unwrap_or(0);
            println!("{}@{} — {} downloads", name, version, dl);
            if !desc.is_empty() {
                println!("  {}", desc);
            }
        }
    } else {
        println!("No packages found for '{}'.", query);
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

    let resp = ureq::post(&url)
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

    let resp = ureq::post(&url)
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

    let resp = ureq::delete(&url)
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

    let resp = ureq::post(&url)
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
fn prompt_token() -> Result<String, String> {
    println!("Not logged in. Authenticate to publish:");
    println!();
    print!("Paste your API key: ");
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
    match ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .call()
    {
        Ok(resp) if resp.status() == 200 => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(nick) = data["nickname"].as_str() {
                    println!("Authenticated as {}", nick);
                }
            }
        }
        Ok(resp) => return Err(format!("invalid token (HTTP {})", resp.status())),
        Err(e) => return Err(format!("cannot reach registry: {}", e)),
    }

    Ok(token)
}

pub fn publish(dir: &Path) -> Result<(), String> {
    let token = match read_token() {
        Some(t) => t,
        None => prompt_token()?,
    };

    // Resolve nickname for scoped name
    let nickname = read_nickname()
        .ok_or("not logged in — run 'synx login' first (nickname not found)")?;

    let manifest_path = dir.join(MANIFEST_NAME);
    let manifest = Manifest::from_file(&manifest_path)?;
    manifest.validate_for_publish().map_err(|errs| errs.join("\n"))?;

    let full_name = manifest.full_name(&nickname);

    // Pack first
    let (tarball_path, _integrity) = pack(dir)?;
    let tarball_size = fs::metadata(&tarball_path)
        .map_err(|e| format!("cannot stat tarball: {}", e))?
        .len();

    if tarball_size > MAX_PACKAGE_SIZE {
        let _ = fs::remove_file(&tarball_path);
        return Err(format!(
            "package too large ({} bytes, max {} bytes)",
            tarball_size, MAX_PACKAGE_SIZE
        ));
    }

    let tarball_data = fs::read(&tarball_path)
        .map_err(|e| format!("cannot read tarball: {}", e))?;

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
    let mut req = ureq::post(&url)
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

    // Clean up tarball
    let _ = fs::remove_file(&tarball_path);

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
    println!("1. Visit https://dev.aperturesyndicate.com");
    println!("2. Log in with your Syndicate account");
    println!("3. Create an API key and copy it");
    println!();
    print!("Paste your API key: ");
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
    let nickname = match ureq::get(&url)
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
            match ureq::get(&url)
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
            match ureq::get(&url)
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
    let lock = LockFile::read(project_root);
    if lock.entries.is_empty() {
        println!("No packages to update.");
        return Ok(());
    }

    let names: Vec<String> = match package {
        Some(name) => {
            if lock.find(name).is_none() {
                return Err(format!("package '{}' is not in synx.lock", name));
            }
            vec![name.to_string()]
        }
        None => lock.entries.iter().map(|e| e.name.clone()).collect(),
    };

    for name in &names {
        match install(project_root, name, None) {
            Ok(()) => {}
            Err(e) => eprintln!("warning: cannot update {}: {}", name, e),
        }
    }

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────

fn parse_package_spec<'a>(spec: &'a str, version: Option<&'a str>) -> (String, Option<String>) {
    if let Some(ver) = version {
        return (spec.to_string(), Some(ver.to_string()));
    }
    // Check for @scope/name@version format
    if let Some(at_pos) = spec[1..].find('@') {
        let pos = at_pos + 1;
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
                "version conflict for '{}': locked at {}, but {} requested",
                name, entry.version, requested
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
        let result = pack(&pkg_dir);
        assert!(result.is_ok(), "pack failed: {:?}", result.err());
        let (path, integrity) = result.unwrap();
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
