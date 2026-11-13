//! What `pack`, `install`, `update` and `uninstall` must refuse.
//!
//! The registry in these tests is a listener on 127.0.0.1 answering from a
//! table; nothing leaves the machine.

use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synx-pkgsec-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
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

const MANIFEST: &str = "name @t/pkg\nversion 1.0.0\ndescription d\nauthor a\nlicense MIT\n";

fn archive_names(tarball: &Path) -> Vec<String> {
    let file = fs::File::open(tarball).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut names: Vec<String> = archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    names.sort();
    names
}

// ─── pack ─────────────────────────────────────────────────

#[test]
fn pack_without_files_and_without_src_refuses_instead_of_sweeping_the_project() {
    let dir = scratch("pack-nosrc");
    write(&dir.join("synx-pkg.synx"), MANIFEST);
    write(&dir.join("config/secrets.synx"), "db_password hunter2\n");
    write(&dir.join("deploy/prod/app.synx"), "stripe_key sk_live_PROBE\n");

    let err = pack_report(&dir).unwrap_err();
    assert!(err.contains("nothing to pack"), "{}", err);
    assert!(err.contains("src/"), "{}", err);
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tar.gz"))
        .collect();
    assert!(leftovers.is_empty(), "an archive was written");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_without_files_takes_src_only() {
    let dir = scratch("pack-src");
    write(&dir.join("synx-pkg.synx"), MANIFEST);
    write(&dir.join("README.md"), "# pkg\n");
    write(&dir.join("src/main.synx"), "k v\n");
    write(&dir.join("src/nested/more.synx"), "a 1\n");
    write(&dir.join("src/.env"), "TOKEN=abc\n");
    write(&dir.join("src/server.key"), "PRIVATE\n");
    write(&dir.join("src/.git/config"), "x\n");
    write(&dir.join("config/secrets.synx"), "db_password hunter2\n");
    write(&dir.join("deploy/prod/app.synx"), "stripe_key sk_live_PROBE\n");

    let report = pack_report(&dir).unwrap();
    assert_eq!(
        archive_names(&report.tarball),
        vec!["README.md", "src/main.synx", "src/nested/more.synx", "synx-pkg.synx"]
    );
    let left_out: Vec<&str> = report.skipped.iter().map(|(p, _)| p.as_str()).collect();
    assert!(left_out.contains(&"src/.env"), "{:?}", left_out);
    assert!(left_out.contains(&"src/server.key"), "{:?}", left_out);
    assert!(left_out.contains(&"src/.git"), "{:?}", left_out);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_does_not_follow_links() {
    let dir = scratch("pack-link");
    let package = dir.join("package");
    let outside = dir.join("outside");
    write(&package.join("synx-pkg.synx"), MANIFEST);
    write(&package.join("src/main.synx"), "k v\n");
    write(&outside.join("id_token.synx"), "token outside-secret\n");
    if !link_dir(&outside, &package.join("src").join("linked")) {
        eprintln!("SKIP: cannot create a directory link here");
        return;
    }
    let report = pack_report(&package).unwrap();
    assert_eq!(archive_names(&report.tarball), vec!["src/main.synx", "synx-pkg.synx"]);
    assert!(report.skipped.iter().any(|(p, why)| p == "src/linked" && *why == "a link"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_refuses_files_entries_that_leave_the_package() {
    let dir = scratch("pack-escape");
    write(&dir.join("outside.synx"), "token outside\n");
    let package = dir.join("package");
    write(&package.join("src/main.synx"), "k v\n");
    for listed in ["../outside.synx", "/etc/passwd", "src/../../outside.synx"] {
        write(&package.join("synx-pkg.synx"), &format!("{}files\n  - {}\n", MANIFEST, listed));
        let err = pack_report(&package).unwrap_err();
        assert!(err.starts_with("files: unsafe path"), "{}: {}", listed, err);
    }
    write(&package.join("synx-pkg.synx"), &format!("{}main ../outside.synx\n", MANIFEST));
    let err = pack_report(&package).unwrap_err();
    assert!(err.starts_with("main: unsafe path"), "{}", err);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_refuses_a_name_or_version_that_moves_the_archive() {
    let dir = scratch("pack-name");
    write(&dir.join("src/main.synx"), "k v\n");
    for (name, version) in [("../../evil", "1.0.0"), ("@t/pkg", "1.0.0/../../x"), ("@t/..", "1.0.0")] {
        write(
            &dir.join("synx-pkg.synx"),
            &format!("name {}\nversion {}\ndescription d\nauthor a\nlicense MIT\n", name, version),
        );
        assert!(pack_report(&dir).is_err(), "{} {}", name, version);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_leaves_other_archives_alone_and_packs_the_manifest_once() {
    let dir = scratch("pack-keep");
    write(
        &dir.join("synx-pkg.synx"),
        "name tools\nversion 1.0.0\ndescription d\nauthor a\nlicense MIT\nfiles\n  - synx-pkg.synx\n  - src\n",
    );
    write(&dir.join("src/main.synx"), "k v\n");
    write(&dir.join("tools-prod-backup.tar.gz"), "mine");
    write(&dir.join("toolshed-logs.tar.gz"), "mine");

    let report = pack_report(&dir).unwrap();
    assert_eq!(fs::read_to_string(dir.join("tools-prod-backup.tar.gz")).unwrap(), "mine");
    assert_eq!(fs::read_to_string(dir.join("toolshed-logs.tar.gz")).unwrap(), "mine");
    assert_eq!(archive_names(&report.tarball), vec!["src/main.synx", "synx-pkg.synx"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_reports_literal_secrets_and_listed_credential_files() {
    let dir = scratch("pack-literal");
    write(
        &dir.join("synx-pkg.synx"),
        &format!("{}files\n  - src\n  - deploy.key\n", MANIFEST),
    );
    write(&dir.join("deploy.key"), "PRIVATE\n");
    write(
        &dir.join("src/main.synx"),
        "!active\napi_key:secret sk-live-123\nfrom_env:secret:env API_KEY\ndb\n  password[required]:secret hunter2\n",
    );
    write(&dir.join("src/static.synx"), "token:secret abc\nplain value\n");

    let report = pack_report(&dir).unwrap();
    assert_eq!(
        report.secret_literals,
        vec!["src/main.synx: api_key", "src/main.synx: db.password", "src/static.synx: token"]
    );
    assert_eq!(report.listed_secret_files, vec!["deploy.key"]);
    assert!(report.secret_literals.iter().all(|l| !l.contains("hunter2") && !l.contains("sk-live")));
    let _ = fs::remove_dir_all(&dir);
}

// ─── install --path, uninstall ────────────────────────────

#[test]
fn install_local_refuses_a_manifest_name_that_leaves_synx_packages() {
    for name in ["../keepme", "...", "@x/...", "@x/../../keepme", "nul"] {
        let dir = scratch("local-name");
        let project = dir.join("project");
        write(&project.join("keepme/precious.txt"), "precious");
        write(&project.join("synx_packages/@other/pkg/synx-pkg.synx"), MANIFEST);
        let evil = dir.join("evil");
        write(&evil.join("synx-pkg.synx"), &format!("name {}\nversion 1.0.0\n", name));
        write(&evil.join("payload.txt"), "payload");

        let err = install_local(&project, &evil).unwrap_err();
        assert!(err.contains("invalid package name"), "{}: {}", name, err);
        assert_eq!(fs::read_to_string(project.join("keepme/precious.txt")).unwrap(), "precious");
        assert!(project.join("synx_packages/@other/pkg/synx-pkg.synx").is_file(), "{}", name);
        assert!(!project.join(LOCK_FILE).exists(), "{}: synx.lock was written", name);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn install_local_from_the_installed_directory_keeps_the_package() {
    let dir = scratch("local-self");
    let installed = dir.join("synx_packages/@t/pkg");
    write(&installed.join("synx-pkg.synx"), MANIFEST);
    write(&installed.join("src/main.synx"), "k v\n");

    install_local(&dir, &installed).unwrap();
    assert_eq!(fs::read_to_string(installed.join("src/main.synx")).unwrap(), "k v\n");
    assert!(!dir.join("synx_packages/.staging").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_local_from_the_project_root_does_not_copy_into_itself() {
    let dir = scratch("local-root");
    write(&dir.join("synx-pkg.synx"), MANIFEST);
    write(&dir.join("src/main.synx"), "k v\n");

    install_local(&dir, &dir).unwrap();
    let installed = dir.join("synx_packages/@t/pkg");
    assert!(installed.join("src/main.synx").is_file());
    assert!(!installed.join("synx_packages").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn uninstall_refuses_names_that_leave_synx_packages() {
    let dir = scratch("uninstall");
    let project = dir.join("project");
    write(&project.join("victim/precious.txt"), "precious");
    write(&dir.join("victim/precious.txt"), "precious");
    write(&project.join("synx_packages/@x/pkg/synx-pkg.synx"), MANIFEST);
    let absolute = format!("{}/@abs", dir.join("victim").display());
    for name in ["@x/../../victim", "@x/../../../victim", "@x/..", "@..", absolute.as_str()] {
        assert!(uninstall(&project, name).is_err(), "{}", name);
    }
    assert!(project.join("victim/precious.txt").is_file());
    assert!(dir.join("victim/precious.txt").is_file());
    assert!(project.join("synx_packages/@x/pkg/synx-pkg.synx").is_file());

    uninstall(&project, "@x/pkg").unwrap();
    assert!(!project.join("synx_packages/@x").exists());
    let _ = fs::remove_dir_all(&dir);
}

// ─── registry: install, update ────────────────────────────

type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

struct FakeRegistry {
    /// `http://127.0.0.1:<port>`
    origin: String,
    routes: Routes,
    hits: Arc<Mutex<Vec<String>>>,
}

impl FakeRegistry {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let routes: Routes = Arc::new(Mutex::new(HashMap::new()));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let (thread_routes, thread_hits) = (routes.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut header = String::new();
                while reader.read_line(&mut header).map_or(false, |n| n > 2) {
                    header.clear();
                }
                let path = request_line.split(' ').nth(1).unwrap_or("").to_string();
                thread_hits.lock().unwrap().push(path.clone());
                let (status, body) = thread_routes
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"{\"error\":\"not found\"}".to_vec()));
                let mut stream = reader.into_inner();
                let head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        FakeRegistry { origin, routes, hits }
    }

    fn api(&self) -> String {
        format!("{}/api", self.origin)
    }

    fn serve(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.to_string(), (status, body.into()));
    }

    /// Serves `archive` as version `version` of `@t/pkg` and returns its hash.
    fn serve_version(&self, version: &str, archive: &[u8]) -> String {
        let integrity = sha256_bytes(archive);
        let file = format!("/dl/pkg-{}.tar.gz", version);
        self.serve(&file, 200, archive.to_vec());
        let meta = serde_json::json!({
            "version": version,
            "tarballUrl": format!("{}{}", self.origin, file),
            "integrity": integrity,
        });
        self.serve(&format!("/api/packages/t/pkg/{}", version), 200, meta.to_string());
        integrity
    }

    fn was_asked(&self, path: &str) -> bool {
        self.hits.lock().unwrap().iter().any(|p| p == path)
    }
}

fn archive(build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>)) -> Vec<u8> {
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(enc);
    build(&mut builder);
    builder.into_inner().unwrap().finish().unwrap()
}

fn entry(
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

fn plain_package(marker: &str) -> Vec<u8> {
    archive(|b| {
        entry(b, "synx-pkg.synx", tar::EntryType::Regular, None, MANIFEST.as_bytes());
        entry(b, "src/main.synx", tar::EntryType::Regular, None, format!("marker {}\n", marker).as_bytes());
    })
}

fn installed_marker(project: &Path) -> String {
    fs::read_to_string(project.join("synx_packages/@t/pkg/src/main.synx")).unwrap_or_default()
}

#[test]
fn install_unpacks_a_plain_package_and_locks_its_hash() {
    let project = scratch("install-ok");
    let registry = FakeRegistry::start();
    let good = plain_package("good");
    let integrity = registry.serve_version("1.0.0", &good);
    let list = serde_json::json!({ "versions": [
        { "version": "1.0.0", "tarballUrl": format!("{}/dl/pkg-1.0.0.tar.gz", registry.origin), "integrity": integrity }
    ]});
    registry.serve("/api/packages/t/pkg", 200, list.to_string());

    install_from(&registry.api(), &project, "@t/pkg", None).unwrap();
    assert_eq!(installed_marker(&project), "marker good\n");
    let lock = LockFile::read(&project);
    let entry = lock.find("@t/pkg").unwrap();
    assert_eq!((entry.version.as_str(), entry.integrity.as_str()), ("1.0.0", integrity.as_str()));
    assert!(!project.join("synx_packages/.staging").exists());
    let _ = fs::remove_dir_all(&project);
}

#[test]
fn install_refuses_an_archive_that_differs_from_the_lock() {
    let project = scratch("install-lock");
    let registry = FakeRegistry::start();
    let good = plain_package("good");
    let good_integrity = registry.serve_version("1.0.0", &good);
    install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap();

    // The registry (or its bucket) now serves another archive under the same version.
    registry.serve_version("1.0.0", &plain_package("swapped"));
    let err = install_from(&registry.api(), &project, "@t/pkg", None).unwrap_err();
    assert!(err.contains("integrity differs from synx.lock"), "{}", err);
    assert_eq!(installed_marker(&project), "marker good\n");
    assert_eq!(LockFile::read(&project).find("@t/pkg").unwrap().integrity, good_integrity);

    // An explicit version is held to the lock as well.
    let err = install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap_err();
    assert!(err.contains("integrity differs from synx.lock"), "{}", err);
    let _ = fs::remove_dir_all(&project);
}

#[test]
fn install_without_a_version_takes_the_locked_one() {
    let project = scratch("install-pin");
    let registry = FakeRegistry::start();
    registry.serve_version("1.0.0", &plain_package("one"));
    let two = registry.serve_version("2.0.0", &plain_package("two"));
    let list = serde_json::json!({ "versions": [
        { "version": "2.0.0", "tarballUrl": format!("{}/dl/pkg-2.0.0.tar.gz", registry.origin), "integrity": two }
    ]});
    registry.serve("/api/packages/t/pkg", 200, list.to_string());

    install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap();
    fs::remove_dir_all(project.join("synx_packages")).unwrap();

    install_from(&registry.api(), &project, "@t/pkg", None).unwrap();
    assert_eq!(installed_marker(&project), "marker one\n");
    assert_eq!(LockFile::read(&project).find("@t/pkg").unwrap().version, "1.0.0");
    assert!(!registry.was_asked("/api/packages/t/pkg"), "the latest version was looked up");
    let _ = fs::remove_dir_all(&project);
}

#[test]
fn install_refuses_hostile_archives_and_keeps_the_installed_package() {
    let dir = scratch("install-hostile");
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    write(&dir.join("outside.txt"), "untouched");
    let registry = FakeRegistry::start();
    registry.serve_version("1.0.0", &plain_package("good"));
    install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap();
    fs::remove_file(project.join(LOCK_FILE)).unwrap();

    let hostile: Vec<(&str, Vec<u8>)> = vec![
        ("symlink", archive(|b| entry(b, "src/main.synx", tar::EntryType::Symlink, Some("../../../../outside.txt"), b""))),
        ("hardlink", archive(|b| entry(b, "x", tar::EntryType::Link, Some("../../../outside.txt"), b""))),
        ("dotdot", archive(|b| entry(b, "../../../outside.txt", tar::EntryType::Regular, None, b"overwritten"))),
        ("device", archive(|b| entry(b, "src/NUL", tar::EntryType::Regular, None, b"x"))),
        ("not an archive", b"plain text, not gzip".to_vec()),
    ];
    for (label, bytes) in hostile {
        registry.serve_version("1.0.0", &bytes);
        let err = install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap_err();
        assert!(err.starts_with("extract error"), "{}: {}", label, err);
        assert_eq!(installed_marker(&project), "marker good\n", "{}", label);
        assert_eq!(fs::read_to_string(dir.join("outside.txt")).unwrap(), "untouched", "{}", label);
        assert!(!project.join(LOCK_FILE).exists(), "{}: synx.lock was written", label);
        assert!(!project.join("synx_packages/.staging").exists(), "{}: staging left behind", label);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_stops_at_the_download_cap() {
    let project = scratch("install-cap");
    let registry = FakeRegistry::start();
    registry.serve_version("1.0.0", &vec![0u8; MAX_PACKAGE_SIZE as usize + 1]);
    let err = install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap_err();
    assert!(err.contains("package too large"), "{}", err);
    assert!(!project.join("synx_packages/@t").exists());
    let _ = fs::remove_dir_all(&project);
}

#[test]
fn install_refuses_names_versions_and_answers_that_do_not_match() {
    let project = scratch("install-args");
    let registry = FakeRegistry::start();
    for name in ["@a/../../x", "@a/b/c", "nope", "", "é", "@a/..", "@con/x"] {
        assert!(install_from(&registry.api(), &project, name, None).is_err(), "{:?}", name);
    }
    assert!(install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0\n@evil/pkg")).is_err());
    assert!(registry.hits.lock().unwrap().is_empty(), "the registry was asked");

    // Asked for 1.0.0, answered with 9.9.9.
    let meta = serde_json::json!({ "version": "9.9.9", "tarballUrl": format!("{}/dl/x", registry.origin), "integrity": "" });
    registry.serve("/api/packages/t/pkg/1.0.0", 200, meta.to_string());
    let err = install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap_err();
    assert!(err.contains("when 1.0.0 was asked for"), "{}", err);
    let _ = fs::remove_dir_all(&project);
}

#[test]
fn update_stays_in_the_major_skips_local_packages_and_fails_loudly() {
    let project = scratch("update");
    let registry = FakeRegistry::start();
    registry.serve_version("1.0.0", &plain_package("1.0.0"));
    install_from(&registry.api(), &project, "@t/pkg", Some("1.0.0")).unwrap();
    let mut lock = LockFile::read(&project);
    lock.upsert("@t/local", "1.0.0", LOCAL_INTEGRITY);
    lock.write(&project).unwrap();

    let v12 = registry.serve_version("1.2.0", &plain_package("1.2.0"));
    let list = |versions: &[(&str, bool)]| {
        let items: Vec<serde_json::Value> = versions
            .iter()
            .map(|(v, yanked)| {
                serde_json::json!({
                    "version": v, "yanked": yanked, "integrity": if *v == "1.2.0" { v12.as_str() } else { "" },
                    "tarballUrl": format!("{}/dl/pkg-{}.tar.gz", registry.origin, v),
                })
            })
            .collect();
        serde_json::json!({ "versions": items }).to_string()
    };
    registry.serve("/api/packages/t/pkg", 200, list(&[("2.0.0", false), ("1.3.0", true), ("1.2.0", false), ("1.1.0", false)]));

    update_from(&registry.api(), &project, None).unwrap();
    assert_eq!(installed_marker(&project), "marker 1.2.0\n");
    let lock = LockFile::read(&project);
    assert_eq!(lock.find("@t/pkg").unwrap().version, "1.2.0");
    assert_eq!(lock.find("@t/local").unwrap().integrity, LOCAL_INTEGRITY);
    assert!(!registry.was_asked("/api/packages/t/local"), "a local package was looked up in the registry");

    // A second run has nothing to do; a package the registry does not know is an error.
    update_from(&registry.api(), &project, None).unwrap();
    let mut lock = LockFile::read(&project);
    lock.upsert("@t/gone", "1.0.0", "sha256-00");
    lock.write(&project).unwrap();
    let err = update_from(&registry.api(), &project, None).unwrap_err();
    assert!(err.contains("1 package(s) not updated"), "{}", err);
    let _ = fs::remove_dir_all(&project);
}

// ─── pure checks ──────────────────────────────────────────

fn picked(integrity: &str, signature: &str, public_key: &str) -> RegistryVersion {
    RegistryVersion {
        version: "1.0.0".into(),
        tarball_url: String::new(),
        integrity: integrity.into(),
        signature: signature.into(),
        public_key: public_key.into(),
        yanked: false,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[test]
fn the_lock_pins_the_publisher_key() {
    let data = b"archive bytes";
    let key = synx_core::signing::SigningKey::generate();
    let other = synx_core::signing::SigningKey::generate();
    let signed = picked("", &hex(&key.sign(data)), &key.verify_key().to_hex());
    let entry = |signer: &str| LockEntry {
        name: "@t/pkg".into(),
        version: "0.9.0".into(),
        integrity: "sha256-old".into(),
        signer: signer.into(),
    };

    // First install: the key is returned to be pinned.
    let (_, signer) = verify_download("@t/pkg", &signed, data, None).unwrap();
    assert_eq!(signer, key.verify_key().to_hex());
    // Same key later: accepted.
    assert!(verify_download("@t/pkg", &signed, data, Some(&entry(&signer))).is_ok());
    // Another key, even with a valid signature of its own: refused.
    let resigned = picked("", &hex(&other.sign(data)), &other.verify_key().to_hex());
    let err = verify_download("@t/pkg", &resigned, data, Some(&entry(&signer))).unwrap_err();
    assert!(err.contains("publisher key for @t/pkg changed"), "{}", err);
    // A pinned key and no signature: refused.
    let err = verify_download("@t/pkg", &picked("", "", ""), data, Some(&entry(&signer))).unwrap_err();
    assert!(err.contains("sent no signature"), "{}", err);
    // A signature that does not match the archive: refused.
    assert!(verify_download("@t/pkg", &signed, b"other bytes", None).is_err());
    // The neutral point as the key, R equal to it and S = 0: valid for any bytes.
    let neutral_key = format!("01{}", "00".repeat(31));
    let neutral_sig = format!("01{}{}", "00".repeat(31), "00".repeat(32));
    // (Refused here by the second check, or by the engine once it verifies strictly.)
    assert!(verify_download("@t/pkg", &picked("", &neutral_sig, &neutral_key), data, None).is_err());
    // Garbage from the registry is an error, not a panic.
    for (sig, pk) in [("zz", "zz"), ("aé1", "aé1"), ("abc", &*key.verify_key().to_hex()), ("ab", "+f")] {
        assert!(verify_download("@t/pkg", &picked("", sig, pk), data, None).is_err(), "{} {}", sig, pk);
    }
}

#[test]
fn a_registry_answer_is_checked_against_the_question() {
    let meta = serde_json::json!({ "version": "1.0.0\n@evil/pkg", "tarballUrl": "https://x/y" });
    assert!(pick_version(&meta, None).is_err());
    let meta = serde_json::json!({ "versions": [{ "version": "2.0.0", "yanked": true }, { "version": "1.5.0" }] });
    assert_eq!(pick_version(&meta, None).unwrap().version, "1.5.0");
    assert_eq!(pick_version(&meta, Some("2.0.0")).unwrap().version, "2.0.0");
    assert!(pick_version(&meta, Some("3.0.0")).is_err());
    assert!(pick_version(&serde_json::json!({}), None).is_err());
}

#[test]
fn download_addresses_must_be_https() {
    let https = "https://synx.aperturesyndicate.com/api";
    assert!(check_download_url("https://media.example/pkg.tar.gz", https).is_ok());
    assert!(check_download_url("http://media.example/pkg.tar.gz", https).is_err());
    assert!(check_download_url("file:///etc/passwd", https).is_err());
    assert!(check_download_url("", https).is_err());
    assert!(check_download_url("http://127.0.0.1:9/pkg.tar.gz", "http://127.0.0.1:9/api").is_ok());
}

#[test]
fn a_package_argument_never_panics() {
    for spec in ["", "é", "@", "@@", "ж@1", "@a/b@", "@a/b@1.0.0"] {
        let _ = parse_package_spec(spec, None);
    }
    assert_eq!(parse_package_spec("@a/b@1.0.0", None), ("@a/b".to_string(), Some("1.0.0".to_string())));
    assert_eq!(parse_package_spec("é@2.0.0", None), ("é".to_string(), Some("2.0.0".to_string())));
}

#[test]
fn credentials_are_written_as_toml_strings() {
    let text = credentials_toml("tok\"en", "nick\"\nregistry = \"https://evil.example/api", "https://r.example/api", "now");
    let table: toml::Table = text.parse().expect("valid TOML");
    let section = table["default"].as_table().unwrap();
    assert_eq!(section["token"].as_str(), Some("tok\"en"));
    assert_eq!(section["registry"].as_str(), Some("https://r.example/api"));
    assert_eq!(section["nickname"].as_str(), Some("nick\"\nregistry = \"https://evil.example/api"));
}

#[test]
fn lock_entries_keep_their_signer() {
    let mut lock = LockFile::default();
    lock.upsert("@t/a", "1.0.0", "sha256-aa");
    lock.set_signer("@t/a", "abcd");
    lock.upsert("@t/a", "1.1.0", "sha256-bb");
    let dir = scratch("lock");
    lock.write(&dir).unwrap();
    let read = LockFile::read(&dir);
    let entry = read.find("@t/a").unwrap();
    assert_eq!((entry.version.as_str(), entry.signer.as_str()), ("1.1.0", "abcd"));
    assert!(entry.is_pinned());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn search_queries_are_encoded() {
    assert_eq!(url_encode("a&limit=9999 #x"), "a%26limit%3D9999%20%23x");
    assert_eq!(url_encode("текст"), "%D1%82%D0%B5%D0%BA%D1%81%D1%82");
}
