use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use synx_core::{
    metadata_to_json_schema, validate_serde_json, validate_with_json_schema, Synx, Value, Options,
    Mode,
};

#[derive(Parser)]
#[command(
    name = "synx",
    version,
    about = "SYNX CLI — The Active Data Format",
    long_about = "Parse, validate, convert, diff, query, and compile .synx files.\nhttps://github.com/APERTURESyndicate/synx-format"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a .synx file and output JSON
    Parse {
        file: PathBuf,
        /// Resolve !active markers
        #[arg(long)]
        active: bool,
    },

    /// Validate a .synx file (exit 0 = ok, exit 1 = error)
    Validate {
        file: PathBuf,
        /// Treat warnings as errors
        #[arg(long)]
        strict: bool,
        /// After parsing, validate the data tree against this JSON Schema file
        #[arg(long, value_name = "FILE")]
        json_schema: Option<PathBuf>,
        /// Build JSON Schema from this file's own !active constraints and validate the tree
        #[arg(long)]
        self_schema: bool,
    },

    /// Print JSON Schema (draft 2020-12) inferred from !active [constraints] in a .synx file
    Schema {
        file: PathBuf,
    },

    /// Validate a JSON instance file against a JSON Schema file (out-of-the-box JSON Schema check)
    JsonValidate {
        instance: PathBuf,
        schema: PathBuf,
    },

    /// Convert between formats (synx→json, json→synx)
    Convert {
        file: PathBuf,
        /// Output format: json or synx
        #[arg(short, long, default_value = "json")]
        format: String,
        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Resolve !active markers before converting
        #[arg(long)]
        active: bool,
    },

    /// Parse a !tool SYNX file and output the tool call as JSON
    Tool {
        file: PathBuf,
    },

    /// Compile .synx to binary .synxb
    Compile {
        file: PathBuf,
        /// Write output to file (default: <input>.synxb)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Resolve all markers before compiling
        #[arg(long)]
        resolved: bool,
    },

    /// Decompile .synxb back to .synx text
    Decompile {
        file: PathBuf,
        /// Write output to file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Structural diff between two .synx files
    Diff {
        file_a: PathBuf,
        file_b: PathBuf,
    },

    /// Query a value by dot-path (e.g. server.host, items.0)
    Query {
        /// Dot-separated path (e.g. server.host, inventory.0)
        path: String,
        file: PathBuf,
        /// Resolve !active markers before querying
        #[arg(long)]
        active: bool,
    },

    /// Reformat a .synx file into canonical form
    Format {
        file: PathBuf,
        /// Write result back to the file (in-place)
        #[arg(short, long)]
        write: bool,
    },
}

fn read_file(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {}", path.display(), e);
        process::exit(1);
    })
}

fn parse_file(text: &str, active: bool, base_path: Option<String>) -> Value {
    let mut result = synx_core::parse(text);
    if active && result.mode == Mode::Active {
        let opts = Options {
            base_path,
            ..Default::default()
        };
        synx_core::resolve(&mut result, &opts);
    }
    result.root
}

fn parse_to_map(text: &str, active: bool, base_path: Option<String>) -> HashMap<String, Value> {
    match parse_file(text, active, base_path) {
        Value::Object(map) => map,
        _ => HashMap::new(),
    }
}

fn value_to_json(val: &Value) -> String {
    synx_core::to_json(val)
}

fn write_output(data: &str, output: &Option<PathBuf>) {
    match output {
        Some(path) => {
            fs::write(path, data).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {}", path.display(), e);
                process::exit(1);
            });
        }
        None => print!("{}", data),
    }
}

fn query_value<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            Value::Array(arr) => {
                let idx: usize = segment.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse { file, active } => {
            let text = read_file(&file);
            let base = file.parent().map(|p| p.to_string_lossy().into_owned());
            let root = parse_file(&text, active, base);
            println!("{}", value_to_json(&root));
        }

        Commands::Validate {
            file,
            strict,
            json_schema,
            self_schema,
        } => {
            let text = read_file(&file);
            let base = file.parent().map(|p| p.to_string_lossy().into_owned());
            let mut result = synx_core::parse(&text);

            let has_active = result.mode == Mode::Active;
            let mut errors: Vec<String> = Vec::new();

            for line in text.lines() {
                if line.contains('\t') {
                    errors.push("tab character found (SYNX uses spaces)".into());
                }
            }

            if has_active {
                let opts = Options {
                    base_path: base,
                    ..Default::default()
                };
                synx_core::resolve(&mut result, &opts);
                if let Value::Object(map) = &result.root {
                    for (key, val) in map {
                        if let Value::String(s) = val {
                            if s.ends_with("_ERR")
                                || s.contains("_ERR:")
                                || s.contains("CONSTRAINT_ERR:")
                            {
                                if strict {
                                    errors.push(format!("{}: {}", key, s));
                                }
                            }
                        }
                    }
                }
            }

            if self_schema {
                if !has_active {
                    errors.push("--self-schema requires !active with [constraints] in the file".into());
                } else {
                    let sch = metadata_to_json_schema(&result.metadata);
                    match validate_with_json_schema(&result.root, &sch) {
                        Ok(()) => {}
                        Err(e) => errors.extend(e),
                    }
                }
            }

            if let Some(ref schema_path) = json_schema {
                let sch_raw = read_file(schema_path);
                match serde_json::from_str::<serde_json::Value>(&sch_raw) {
                    Ok(sch) => match validate_with_json_schema(&result.root, &sch) {
                        Ok(()) => {}
                        Err(e) => errors.extend(e),
                    },
                    Err(e) => errors.push(format!("{}: {}", schema_path.display(), e)),
                }
            }

            if errors.is_empty() {
                println!("ok: {}", file.display());
            } else {
                for e in &errors {
                    eprintln!("error: {}", e);
                }
                process::exit(1);
            }
        }

        Commands::Schema { file } => {
            let text = read_file(&file);
            let result = synx_core::parse(&text);
            let sch = metadata_to_json_schema(&result.metadata);
            println!("{}", serde_json::to_string_pretty(&sch).unwrap_or_else(|e| {
                eprintln!("error: {}", e);
                process::exit(1);
            }));
        }

        Commands::JsonValidate { instance, schema } => {
            let inst_raw = read_file(&instance);
            let sch_raw = read_file(&schema);
            let inst: serde_json::Value = match serde_json::from_str(&inst_raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: invalid JSON {}: {}", instance.display(), e);
                    process::exit(1);
                }
            };
            let sch: serde_json::Value = match serde_json::from_str(&sch_raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: invalid JSON Schema file {}: {}", schema.display(), e);
                    process::exit(1);
                }
            };
            match validate_serde_json(&inst, &sch) {
                Ok(()) => println!("ok: {} against {}", instance.display(), schema.display()),
                Err(errs) => {
                    for e in errs {
                        eprintln!("error: {}", e);
                    }
                    process::exit(1);
                }
            }
        }

        Commands::Convert { file, format, output, active } => {
            let text = read_file(&file);
            let base = file.parent().map(|p| p.to_string_lossy().into_owned());

            match format.as_str() {
                "json" => {
                    let root = parse_file(&text, active, base);
                    let json = value_to_json(&root);
                    write_output(&json, &output);
                }
                "synx" => {
                    let val: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("error: invalid JSON: {}", e);
                            process::exit(1);
                        }
                    };
                    let synx_text = Synx::stringify(&val);
                    write_output(&synx_text, &output);
                }
                other => {
                    eprintln!("error: unsupported format '{}' (use json or synx)", other);
                    process::exit(1);
                }
            }
        }

        Commands::Tool { file } => {
            let text = read_file(&file);
            let base = file.parent().map(|p| p.to_string_lossy().into_owned());
            let opts = Options { base_path: base, ..Default::default() };
            let map = Synx::parse_tool(&text, &opts);
            let val = Value::Object(map);
            println!("{}", value_to_json(&val));
        }

        Commands::Compile { file, output, resolved } => {
            let text = read_file(&file);
            let data = Synx::compile(&text, resolved);
            let out_path = output.unwrap_or_else(|| file.with_extension("synxb"));
            fs::write(&out_path, &data).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {}", out_path.display(), e);
                process::exit(1);
            });
            println!("{} ({} bytes)", out_path.display(), data.len());
        }

        Commands::Decompile { file, output } => {
            let data = fs::read(&file).unwrap_or_else(|e| {
                eprintln!("error: cannot read {}: {}", file.display(), e);
                process::exit(1);
            });
            match Synx::decompile(&data) {
                Ok(text) => write_output(&text, &output),
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            }
        }

        Commands::Diff { file_a, file_b } => {
            let text_a = read_file(&file_a);
            let text_b = read_file(&file_b);
            let map_a = parse_to_map(&text_a, true, file_a.parent().map(|p| p.to_string_lossy().into_owned()));
            let map_b = parse_to_map(&text_b, true, file_b.parent().map(|p| p.to_string_lossy().into_owned()));
            let result = Synx::diff(&map_a, &map_b);
            let val = synx_core::diff_to_value(&result);
            println!("{}", value_to_json(&val));
        }

        Commands::Query { path, file, active } => {
            let text = read_file(&file);
            let base = file.parent().map(|p| p.to_string_lossy().into_owned());
            let root = parse_file(&text, active, base);
            match query_value(&root, &path) {
                Some(val) => println!("{}", value_to_json(val)),
                None => {
                    eprintln!("error: path '{}' not found", path);
                    process::exit(1);
                }
            }
        }

        Commands::Format { file, write } => {
            let text = read_file(&file);
            let formatted = Synx::format(&text);
            if write {
                fs::write(&file, &formatted).unwrap_or_else(|e| {
                    eprintln!("error: cannot write {}: {}", file.display(), e);
                    process::exit(1);
                });
            } else {
                print!("{}", formatted);
            }
        }
    }
}
