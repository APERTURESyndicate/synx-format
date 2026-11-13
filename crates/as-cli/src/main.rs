//! APERTURESyndicate unified CLI (`as`)
//!
//! Delegates subcommands to product-specific tools under shared authentication.
//! `as synx <cmd>` → `synx <cmd>`
//! `as login`      → shared credentials for all AS services

use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};

use clap::{Parser, Subcommand};

const CREDENTIALS_DIR_NAME: &str = "aperturesyndicate";

#[derive(Parser)]
#[command(
    name = "as",
    version,
    about = "APERTURESyndicate unified CLI",
    long_about = "Single entry point for all APERTURESyndicate tools.\nhttps://aperturesyndicate.com"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// SYNX commands — delegates to the synx CLI
    Synx {
        /// Arguments to pass to synx
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Log in to APERTURESyndicate (shared credentials for all tools)
    Login,

    /// Log out — remove stored credentials
    Logout,

    /// Show the current logged-in identity
    Whoami,

    /// Show credential and config paths
    Config,
}

fn credentials_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CREDENTIALS_DIR_NAME)
}

fn credentials_path() -> PathBuf {
    credentials_dir().join("credentials.toml")
}

fn read_token() -> Option<String> {
    let text = fs::read_to_string(credentials_path()).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table
        .get("default")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("token"))
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
    "https://synx.aperturesyndicate.com/api".to_string()
}

/// The text of `credentials.toml` with `token` and `registry` set.
///
/// `existing` is the file as it is now: its other keys (the `nickname` that
/// `synx login` saved, which `synx publish` needs) are kept.
///
/// safety: the values were pasted between quotes with `format!`, so a token
/// holding `"` or a line break rewrote the file — `registry` included, which
/// is where the token is sent next.
fn credentials_toml(existing: Option<&str>, token: &str, registry: &str) -> String {
    let mut table: toml::Table = existing.and_then(|text| text.parse().ok()).unwrap_or_default();
    let section = table
        .entry("default")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !section.is_table() {
        *section = toml::Value::Table(toml::Table::new());
    }
    if let Some(section) = section.as_table_mut() {
        section.insert("token".into(), toml::Value::String(token.to_string()));
        section.insert("registry".into(), toml::Value::String(registry.to_string()));
    }
    table.to_string()
}

/// Writes a file only its owner can read.
///
/// safety: the file was created with the default mode and narrowed
/// afterwards; on a shared Unix machine the token was readable in between.
fn write_private(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content)
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)
    }
}

fn save_token(token: &str) -> Result<(), String> {
    let dir = credentials_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;

    let path = credentials_path();
    let existing = fs::read_to_string(&path).ok();
    let content = credentials_toml(existing.as_deref(), token, &registry_url());
    write_private(&path, content.as_bytes())
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

fn remove_token() -> Result<(), String> {
    let path = credentials_path();
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("cannot remove {}: {}", path.display(), e))?;
    }
    Ok(())
}

/// Find the `synx` binary — same directory as self, then PATH
fn find_synx() -> PathBuf {
    if let Ok(self_path) = std::env::current_exe() {
        if let Some(dir) = self_path.parent() {
            let candidate = dir.join(if cfg!(windows) { "synx.exe" } else { "synx" });
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("synx")
}

fn delegate_synx(args: &[String]) -> ! {
    let synx = find_synx();
    let status = Command::new(&synx)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: cannot run '{}': {}", synx.display(), e);
            eprintln!("hint: ensure 'synx' is installed and in PATH");
            process::exit(127);
        });
    process::exit(status.code().unwrap_or(1));
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Synx { args } => {
            delegate_synx(&args);
        }

        Commands::Login => {
            let registry = registry_url();
            println!("APERTURESyndicate Login");
            println!();
            println!("1. Visit: {}/auth/cli-token", registry);
            println!("2. Log in with your Syndicate account");
            println!("3. Copy the token and paste it below:");
            println!();

            let mut token = String::new();
            std::io::stdin()
                .read_line(&mut token)
                .unwrap_or_else(|e| {
                    eprintln!("error: cannot read input: {}", e);
                    process::exit(1);
                });
            let token = token.trim();

            if token.is_empty() {
                eprintln!("error: no token provided");
                process::exit(1);
            }

            match save_token(token) {
                Ok(()) => {
                    println!("Logged in ✓");
                    println!("Token saved to {}", credentials_path().display());
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            }
        }

        Commands::Logout => match remove_token() {
            Ok(()) => println!("Logged out ✓"),
            Err(e) => {
                eprintln!("error: {}", e);
                process::exit(1);
            }
        },

        Commands::Whoami => match read_token() {
            Some(_) => {
                println!("(authenticated — token stored at {})", credentials_path().display());
            }
            None => {
                println!("Not logged in.");
                println!("Run 'as login' to authenticate.");
            }
        },

        Commands::Config => {
            println!("credentials: {}", credentials_path().display());
            println!("config:      {}", credentials_dir().join("config.toml").display());
            println!("registry:    {}", registry_url());
            println!("logged in:   {}", if read_token().is_some() { "yes" } else { "no" });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::credentials_toml;

    fn section(text: &str) -> toml::Table {
        let table: toml::Table = text.parse().expect("valid TOML");
        table["default"].as_table().expect("[default]").clone()
    }

    #[test]
    fn a_token_cannot_rewrite_the_registry() {
        let token = "abc\"\nregistry = \"https://evil.example/api";
        let text = credentials_toml(None, token, "https://synx.aperturesyndicate.com/api");
        let default = section(&text);
        assert_eq!(default["token"].as_str(), Some(token));
        assert_eq!(default["registry"].as_str(), Some("https://synx.aperturesyndicate.com/api"));
    }

    #[test]
    fn logging_in_again_keeps_the_saved_nickname() {
        let existing = "[default]\ntoken = \"old\"\nnickname = \"kai\"\nregistry = \"https://r/api\"\ncreated = \"2026-01-01T00:00:00Z\"\n";
        let default = section(&credentials_toml(Some(existing), "new", "https://r/api"));
        assert_eq!(default["token"].as_str(), Some("new"));
        assert_eq!(default["nickname"].as_str(), Some("kai"));
    }

    #[test]
    fn a_broken_credentials_file_is_replaced() {
        let default = section(&credentials_toml(Some("not toml ["), "t", "https://r/api"));
        assert_eq!(default["token"].as_str(), Some("t"));
        let default = section(&credentials_toml(Some("default = 5\n"), "t", "https://r/api"));
        assert_eq!(default["token"].as_str(), Some("t"));
    }
}
