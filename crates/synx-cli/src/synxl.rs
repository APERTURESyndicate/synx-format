//! `synx synxl` — SYNXL ("SYNX Lines") dataset commands.
//!
//! SYNXL is the record-stream counterpart of JSONL and CSV, specified in
//! `docs/spec/SYNXL-1-NORMATIVE.md`. Section references below (`§12.1`, `§15.3`)
//! are to that document.
//!
//! Every `.synxl` input is read through [`SynxlStreamReader`] (§15.1), so live
//! memory is one record no matter how large the file is — that is the whole
//! point of §13 dropping SYNX's 16 MiB whole-file cap. The JSONL and CSV sides
//! stream too: each is read twice (once to infer the schema, once to write),
//! never held whole.
//!
//! Exit codes: `0` success, `1` the document violates the specification, `2` an
//! I/O failure. The split matters because a failed read says nothing about the
//! dataset, and a caller that retries on `2` must not retry on `1`.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;

use clap::Subcommand;
use synx_core::synxl::{
    records_to_ndjson, write_lines, Diagnostic, FieldDecl, FieldList, SynxlOptions,
    SynxlStreamError, SynxlStreamReader,
};
use synx_core::{to_json, Value};

// ─── Command surface ─────────────────────────────────────────

#[derive(Subcommand)]
pub enum SynxlCommand {
    /// Parse a .synxl dataset and print its canonical JSON projection
    Parse {
        /// Path to the .synxl dataset
        file: PathBuf,
        /// Projection: json (§12.1 array) or ndjson (§12.2, one object per line)
        #[arg(short, long, default_value = "json")]
        format: String,
        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Also enforce declared [constraints] (§8.4, off by default)
        #[arg(long)]
        validate: bool,
        /// Also write diagnostics (§11.2) here, one JSON object per line
        #[arg(long)]
        diagnostics_out: Option<PathBuf>,
    },

    /// Validate a .synxl dataset (exit 0 = ok, 1 = spec violation, 2 = I/O error)
    Validate {
        /// Path to the .synxl dataset
        file: PathBuf,
        /// Enforce declared [constraints] as well (§8.4, off by default)
        #[arg(long)]
        constraints: bool,
        /// Treat diagnostics (§11.2) as errors too
        #[arg(long)]
        strict: bool,
        /// Also write diagnostics (§11.2) here, one JSON object per line
        #[arg(long)]
        diagnostics_out: Option<PathBuf>,
    },

    /// Convert between .synxl and jsonl/csv
    ///
    /// Supported directions: jsonl→synxl, csv→synxl, synxl→jsonl, synxl→csv.
    /// Block fields (§5.3) have no CSV representation: synxl→csv refuses them
    /// unless --block-json is given, which writes every block cell as a JSON
    /// value inside the cell (an unset one stays empty).
    Convert {
        /// Path to the input file (.synxl, .jsonl/.ndjson, or .csv)
        file: PathBuf,
        /// Source format: synxl, jsonl, csv (default: from the file extension)
        #[arg(long)]
        from: Option<String>,
        /// Target format: synxl, jsonl, csv (default: the counterpart of --from)
        #[arg(long)]
        to: Option<String>,
        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// CSV field delimiter (a single character)
        #[arg(long, default_value = ",")]
        delimiter: String,
        /// synxl→csv: serialize every [block] cell as JSON instead of failing
        #[arg(long)]
        block_json: bool,
        /// Also write diagnostics (§11.2) here, one JSON object per line
        #[arg(long)]
        diagnostics_out: Option<PathBuf>,
    },

    /// Split a .synxl dataset into shards of N records
    ///
    /// Every shard is a standalone document: it repeats the `!synxl 1` prologue
    /// and the `!fields` line in effect at the split point (§15.3). Records are
    /// re-serialized in canonical form (§14), so each shard's JSON projection
    /// equals the matching slice of the source's.
    Split {
        /// Path to the .synxl dataset
        file: PathBuf,
        /// Records per shard
        #[arg(short = 'n', long)]
        records: usize,
        /// Directory to write the shards into (default: the input's directory)
        #[arg(short, long)]
        output_dir: Option<PathBuf>,
        /// Shard file name prefix (default: the input's file stem)
        #[arg(long)]
        prefix: Option<String>,
        /// Also write diagnostics (§11.2) here, one JSON object per line
        #[arg(long)]
        diagnostics_out: Option<PathBuf>,
    },
}

pub fn run(command: SynxlCommand) {
    let diagnostics_out = match &command {
        SynxlCommand::Parse { diagnostics_out, .. }
        | SynxlCommand::Validate { diagnostics_out, .. }
        | SynxlCommand::Convert { diagnostics_out, .. }
        | SynxlCommand::Split { diagnostics_out, .. } => diagnostics_out.clone(),
    };
    if let Some(path) = diagnostics_out {
        if let Err(e) = open_diagnostics(&path) {
            eprintln!("error: {}", e.message());
            exit(e.code());
        }
    }

    // `-` reads stdin; spooled here so every subcommand below sees a plain,
    // re-readable path. The guard lives to the end of `run`.
    let spool = {
        let source = match &command {
            SynxlCommand::Parse { file, .. }
            | SynxlCommand::Validate { file, .. }
            | SynxlCommand::Convert { file, .. }
            | SynxlCommand::Split { file, .. } => file.clone(),
        };
        match StdinSpool::take(&source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}", e.message());
                exit(e.code());
            }
        }
    };
    let resolve = |file: PathBuf| match &spool {
        Some(s) => s.path.clone(),
        None => file,
    };

    match command {
        SynxlCommand::Parse { file, format, output, validate, .. } => {
            cmd_parse(resolve(file), &format, output, validate)
        }
        SynxlCommand::Validate { file, constraints, strict, .. } => {
            cmd_validate(resolve(file), constraints, strict)
        }
        SynxlCommand::Convert { file, from, to, output, delimiter, block_json, .. } => {
            cmd_convert(resolve(file), from, to, output, &delimiter, block_json)
        }
        SynxlCommand::Split { file, records, output_dir, prefix, .. } => {
            cmd_split(resolve(file), records, output_dir, prefix)
        }
    }
    flush_diagnostics();
}

// ─── Errors and exit codes ───────────────────────────────────

/// A command failure, split the way [`SynxlStreamError`] splits it: a malformed
/// document is a fact about the caller's data (exit 1), a failed read or write
/// is a fact about the machine (exit 2) and says nothing about the document.
#[derive(Debug)]
enum CmdError {
    Format(String),
    Io(String),
}

impl CmdError {
    fn message(&self) -> &str {
        match self {
            CmdError::Format(m) | CmdError::Io(m) => m,
        }
    }

    fn code(&self) -> i32 {
        match self {
            CmdError::Format(_) => 1,
            CmdError::Io(_) => 2,
        }
    }
}

impl From<SynxlStreamError> for CmdError {
    fn from(e: SynxlStreamError) -> Self {
        match e {
            SynxlStreamError::Format(f) => CmdError::Format(f.to_string()),
            // Invalid UTF-8 is a fact about the document (§3.1), not about the
            // machine: reporting it as an I/O failure tells a caller that
            // retries on exit 2 to keep retrying a file that will never parse.
            SynxlStreamError::Io(err) if err.kind() == io::ErrorKind::InvalidData => {
                CmdError::Format(err.to_string())
            }
            SynxlStreamError::Io(err) => {
                CmdError::Io(format!("I/O error while reading SYNXL: {}", err))
            }
        }
    }
}

fn write_err(e: io::Error) -> CmdError {
    CmdError::Io(format!("cannot write output: {}", e))
}

fn read_err(e: io::Error) -> CmdError {
    CmdError::Io(format!("cannot read input: {}", e))
}

/// §11.1 — a hard error yields no partial result, so a half-written output file
/// is removed rather than left behind looking complete.
fn fail(label: &str, err: CmdError, cleanup: &[&PathBuf]) -> ! {
    for path in cleanup {
        let _ = fs::remove_file(path);
    }
    flush_diagnostics();
    eprintln!("error: {}: {}", label, err.message());
    exit(err.code());
}

// ─── synx synxl parse ────────────────────────────────────────

fn cmd_parse(file: PathBuf, format: &str, output: Option<PathBuf>, validate: bool) {
    let ndjson = match format {
        "json" => false,
        "ndjson" | "jsonl" => true,
        other => {
            eprintln!("error: unsupported projection '{}' (use json or ndjson)", other);
            exit(1);
        }
    };
    let label = file.display().to_string();
    let path = file.clone();
    let mut reopen = move || open_reader(&path).map(|r| Box::new(r) as Box<dyn BufRead>);

    let result = (|| {
        // §11.1 — stdout cannot be rolled back, so the document is checked in
        // full before anything is emitted. Without this a mid-file hard error
        // leaves a *valid-looking* truncated NDJSON stream behind, which a
        // pipeline that loses the exit status cannot tell from a complete one.
        if output.is_none() {
            precheck(&mut reopen)?;
        }
        let input = open_reader(&file)?;
        let mut out = AtomicOutput::open(&output)?;
        let count = synxl_to_json(input, &label, ndjson, validate, &mut out)?;
        out.commit()?;
        Ok(count)
    })();

    if let Err(e) = result {
        fail(&label, e, &[]);
    }
}

// ─── synx synxl validate ─────────────────────────────────────

fn cmd_validate(file: PathBuf, constraints: bool, strict: bool) {
    let label = file.display().to_string();
    let result = (|| -> Result<(usize, usize), CmdError> {
        let input = open_reader(&file)?;
        let opts = SynxlOptions { validate: constraints };
        let mut reader = SynxlStreamReader::with_options(input, opts)?;
        let mut records = 0usize;
        let mut diagnostics = 0usize;
        while let Some(item) = reader.next() {
            // §11.1 — a hard error aborts the document; no partial result.
            let rec = item?;
            diagnostics += rec.diagnostics.len();
            report_diagnostics(&label, &rec.diagnostics);
            records += 1;
        }
        diagnostics += reader.trailing_diagnostics().len();
        report_diagnostics(&label, reader.trailing_diagnostics());
        Ok((records, diagnostics))
    })();

    let (records, diagnostics) = match result {
        Ok(v) => v,
        Err(e) => fail(&label, e, &[]),
    };

    if strict && diagnostics > 0 {
        eprintln!("error: {}: {} diagnostic(s) with --strict", label, diagnostics);
        exit(1);
    }
    println!("ok: {} ({} records, {} diagnostics)", label, records, diagnostics);
}

// ─── synx synxl convert ──────────────────────────────────────

fn cmd_convert(
    file: PathBuf,
    from: Option<String>,
    to: Option<String>,
    output: Option<PathBuf>,
    delimiter: &str,
    block_json: bool,
) {
    let from = match from {
        Some(f) => normalize_format(&f),
        None => match file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref()
        {
            Some("synxl") => "synxl".to_string(),
            Some("jsonl") | Some("ndjson") => "jsonl".to_string(),
            Some("csv") => "csv".to_string(),
            _ => {
                eprintln!(
                    "error: cannot infer source format from {} (pass --from synxl|jsonl|csv)",
                    file.display()
                );
                exit(1);
            }
        },
    };
    // synxl is the pivot: without --to, convert to or from it.
    let to = match to {
        Some(t) => normalize_format(&t),
        None => {
            if from == "synxl" {
                "jsonl".to_string()
            } else {
                "synxl".to_string()
            }
        }
    };

    let delim = {
        let mut chars = delimiter.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => c,
            _ => {
                eprintln!("error: --delimiter must be a single character");
                exit(1);
            }
        }
    };

    let label = file.display().to_string();
    let path = file.clone();
    // The two directions into SYNXL infer the schema from the whole input
    // before the first record can be written, so they read the file twice
    // rather than hold it.
    let mut reopen = move || open_reader(&path).map(|r| Box::new(r) as Box<dyn BufRead>);

    let result = (|| {
        // §11.1 — see `cmd_parse`: a stdout target gets a full pre-flight so a
        // hard error never leaves a partial projection behind.
        if output.is_none() && from == "synxl" {
            precheck(&mut reopen)?;
        }
        let mut out = AtomicOutput::open(&output)?;
        let count = match (from.as_str(), to.as_str()) {
            ("jsonl", "synxl") => jsonl_to_synxl(&mut reopen, &mut out)?,
            ("csv", "synxl") => csv_to_synxl(&mut reopen, delim, &mut out)?,
            ("synxl", "jsonl") => {
                synxl_to_json(open_reader(&file)?, &label, true, false, &mut out)?
            }
            ("synxl", "csv") => {
                synxl_to_csv(open_reader(&file)?, &label, delim, block_json, &mut out)?
            }
            (f, t) => {
                eprintln!(
                    "error: unsupported conversion '{}' → '{}' (supported: jsonl→synxl, csv→synxl, synxl→jsonl, synxl→csv)",
                    f, t
                );
                exit(1);
            }
        };
        out.commit()?;
        Ok(count)
    })();

    if let Err(e) = result {
        fail(&label, e, &[]);
    }
}

// ─── synx synxl split ────────────────────────────────────────

fn cmd_split(file: PathBuf, records: usize, output_dir: Option<PathBuf>, prefix: Option<String>) {
    if records == 0 {
        eprintln!("error: --records must be at least 1");
        exit(1);
    }
    let label = file.display().to_string();

    let dir = output_dir.unwrap_or_else(|| match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    });
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("error: cannot create {}: {}", dir.display(), e);
        exit(2);
    }
    let prefix = prefix.unwrap_or_else(|| {
        file.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shard".to_string())
    });
    // The prefix names a file, not a path. Left unchecked it is a traversal
    // primitive whenever it comes from somewhere automated — a batch name in
    // CI, say — since it is interpolated straight into the shard path and the
    // shard is created with plain truncation.
    if prefix.is_empty()
        || prefix.contains('/')
        || prefix.contains('\\')
        || prefix.contains(':')
        || prefix == ".."
        || prefix == "."
    {
        eprintln!(
            "error: --prefix must be a plain file-name prefix (no `/`, `\\`, `:`, and not `.` or `..`)"
        );
        exit(1);
    }
    let shard_path = |index: usize| dir.join(format!("{}-{:05}.synxl", prefix, index));

    let mut shards = 0usize;
    // Every shard this run created. §11.1 forbids a partial result, and a
    // finished-looking shard is exactly that, so an abort removes all of them —
    // not just the one that was mid-write.
    let created: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());

    let result = (|| {
        let input = open_reader(&file)?;
        split_shards(
            input,
            &label,
            records,
            |index| {
                let path = shard_path(index);
                let handle = File::create(&path)
                    .map_err(|e| CmdError::Io(format!("cannot write {}: {}", path.display(), e)))?;
                created.borrow_mut().push(path);
                Ok(BufWriter::new(handle))
            },
            |index, mut writer, count| {
                writer.flush().map_err(write_err)?;
                println!("{} ({} records)", shard_path(index).display(), count);
                shards += 1;
                Ok(())
            },
        )
    })();

    match result {
        Ok(total) => {
            // Shards left by an earlier, longer split of the same file would
            // otherwise be picked up by a glob over the directory and read as
            // extra records — duplicates of data this run already wrote.
            let mut stale = 0usize;
            for index in (shards + 1)..=(shards + 1_000) {
                let path = shard_path(index);
                if !path.exists() {
                    break;
                }
                if fs::remove_file(&path).is_ok() {
                    stale += 1;
                }
            }
            if stale > 0 {
                println!("removed {} stale shard(s) from a previous split", stale);
            }
            println!("{}: {} records → {} shard(s)", label, total, shards);
        }
        Err(e) => {
            let partial = created.borrow();
            let cleanup: Vec<&PathBuf> = partial.iter().collect();
            fail(&label, e, &cleanup);
        }
    }
}

/// Split a stream into shards of `per_shard` records.
///
/// §15.3 — a shard is a valid SYNXL document only if it begins with a prologue
/// and the field list in effect at the split point, so both go into every
/// shard. The header written is [`FieldList::source`] — the author's own
/// `!fields` line — whenever the record's canonical rendering agrees with the
/// declared shape; when a value forces a §14.3 promotion, the promoted
/// rendering is written instead, since a header that disagrees with the record
/// body underneath it would produce a shard that parses to different data.
fn split_shards<R: BufRead, W: Write>(
    input: R,
    label: &str,
    per_shard: usize,
    mut open: impl FnMut(usize) -> Result<W, CmdError>,
    mut close: impl FnMut(usize, W, usize) -> Result<(), CmdError>,
) -> Result<usize, CmdError> {
    let mut reader = SynxlStreamReader::new(input)?;
    // Per field list: (canonical header with no promotion, header text to emit).
    let mut headers: Vec<(String, String)> = Vec::new();
    // The open shard: (index, writer, records so far, canonical header in effect).
    let mut current: Option<(usize, W, usize, String)> = None;
    let mut shard_no = 0usize;
    let mut total = 0usize;

    while let Some(item) = reader.next() {
        let rec = item?;
        // §11.2 — split is the one subcommand after which the original is
        // usually deleted, so swallowing a `RecordTruncated` here is where a
        // lost megabyte becomes unrecoverable and unreported.
        report_diagnostics(label, &rec.diagnostics);

        while headers.len() < reader.field_lists().len() {
            let fl = &reader.field_lists()[headers.len()];
            headers.push((canonical_fields_line(fl.fields())?, emit_header(fl)?));
        }

        let fl = &reader.field_lists()[rec.field_list];
        let (fields_line, body) = render_record(fl.fields(), &rec.value)?;
        let (declared, source) = &headers[rec.field_list];
        let header = if fields_line == *declared { source.clone() } else { fields_line.clone() };

        match current.as_mut() {
            Some((_, writer, count, active)) if *count < per_shard => {
                // §4.2 — a new field list may appear at any point; re-declare it
                // inside the shard exactly where the source did.
                if *active != fields_line {
                    writer.write_all(header.as_bytes()).map_err(write_err)?;
                    writer.write_all(b"\n").map_err(write_err)?;
                    *active = fields_line;
                }
                writer.write_all(body.as_bytes()).map_err(write_err)?;
                *count += 1;
            }
            _ => {
                if let Some((index, writer, count, _)) = current.take() {
                    close(index, writer, count)?;
                }
                shard_no += 1;
                let mut writer = open(shard_no)?;
                writer.write_all(b"!synxl 1\n").map_err(write_err)?;
                writer.write_all(header.as_bytes()).map_err(write_err)?;
                writer.write_all(b"\n").map_err(write_err)?;
                writer.write_all(body.as_bytes()).map_err(write_err)?;
                current = Some((shard_no, writer, 1, fields_line));
            }
        }
        total += 1;
    }

    report_diagnostics(label, reader.trailing_diagnostics());

    if let Some((index, writer, count, _)) = current.take() {
        close(index, writer, count)?;
    }
    Ok(total)
}

/// The `!fields` line to write for a list: the author's own bytes when it has
/// them, a canonical rendering when the list was built programmatically.
fn emit_header(fl: &FieldList) -> Result<String, CmdError> {
    let source = fl.source().trim();
    if source.is_empty() {
        canonical_fields_line(fl.fields())
    } else {
        Ok(source.to_string())
    }
}

/// Canonical §14 rendering of one record: its `!fields` line, and its body —
/// the record line plus any block lines.
fn render_record(fields: &[FieldDecl], value: &Value) -> Result<(String, String), CmdError> {
    let rendered = write_lines(fields, std::slice::from_ref(value))
        .map_err(|e| CmdError::Format(e.to_string()))?;
    let mut parts = rendered.splitn(3, '\n');
    let _prologue = parts.next().unwrap_or("");
    let fields_line = parts.next().unwrap_or("").to_string();
    let body = parts.next().unwrap_or("").to_string();
    Ok((fields_line, body))
}

fn canonical_fields_line(fields: &[FieldDecl]) -> Result<String, CmdError> {
    let rendered = write_lines(fields, &[]).map_err(|e| CmdError::Format(e.to_string()))?;
    Ok(rendered.lines().nth(1).unwrap_or("!fields").to_string())
}

// ─── synxl → json / ndjson / csv ─────────────────────────────

fn synxl_to_json(
    input: impl BufRead,
    label: &str,
    ndjson: bool,
    validate: bool,
    out: &mut dyn Write,
) -> Result<usize, CmdError> {
    let mut reader = SynxlStreamReader::with_options(input, SynxlOptions { validate })?;
    let mut count = 0usize;
    if !ndjson {
        out.write_all(b"[").map_err(write_err)?;
    }
    while let Some(item) = reader.next() {
        let rec = item?;
        report_diagnostics(label, &rec.diagnostics);
        // §12.2 — the NDJSON line of a single record is exactly its canonical
        // JSON object, so one helper covers both projections.
        let json = records_to_ndjson(std::slice::from_ref(&rec.value));
        if ndjson {
            out.write_all(json.as_bytes()).map_err(write_err)?;
        } else {
            if count > 0 {
                out.write_all(b",").map_err(write_err)?;
            }
            out.write_all(json.trim_end_matches('\n').as_bytes()).map_err(write_err)?;
        }
        count += 1;
    }
    report_diagnostics(label, reader.trailing_diagnostics());
    if !ndjson {
        out.write_all(b"]\n").map_err(write_err)?;
    }
    Ok(count)
}

fn synxl_to_csv(
    input: impl BufRead,
    label: &str,
    delim: char,
    block_json: bool,
    out: &mut dyn Write,
) -> Result<usize, CmdError> {
    let mut reader = SynxlStreamReader::new(input)?;
    let mut header: Option<(usize, Vec<String>, Vec<bool>)> = None;
    let mut count = 0usize;
    // Cells defused against spreadsheet formula execution. Reported at the end
    // rather than applied in silence: the export is no longer byte-identical to
    // the source values, and the operator has to know that.
    let mut neutralised = 0usize;

    while let Some(item) = reader.next() {
        let rec = item?;
        report_diagnostics(label, &rec.diagnostics);

        if header.is_none() {
            let fl = &reader.field_lists()[rec.field_list];
            // §5.3 — a block field holds an embedded SYNX document; a CSV cell
            // holds a scalar. Refusing beats silently emptying the column.
            let blocks: Vec<&str> = fl
                .fields()
                .iter()
                .filter(|f| f.block)
                .map(|f| f.name.as_str())
                .collect();
            if !blocks.is_empty() && !block_json {
                return Err(CmdError::Format(format!(
                    "field(s) `{}` are declared [block] (§5.3) and have no CSV representation; \
re-run with --block-json to write them as JSON inside the cell",
                    blocks.join("`, `")
                )));
            }
            let names: Vec<String> = fl.fields().iter().map(|f| f.name.clone()).collect();
            let block_flags: Vec<bool> = fl.fields().iter().map(|f| f.block).collect();
            write_csv_row(out, &names, delim, &mut neutralised)?;
            header = Some((rec.field_list, names, block_flags));
        }

        let (fl_index, names, block_flags) = header.as_ref().expect("header set above");
        // §4.2 — a document may switch field lists mid-file; CSV has exactly
        // one header row, so the schema change cannot be represented.
        if *fl_index != rec.field_list {
            return Err(CmdError::Format(format!(
                "record {} (line {}) switches to a second field list; CSV has a single header row",
                rec.index, rec.line
            )));
        }
        let map = rec.value.as_object();
        let cells: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let value = map.and_then(|m| m.get(n)).unwrap_or(&Value::Null);
                // Under --block-json every block cell is JSON, including a
                // plain string one — a column whose encoding depends on the
                // row is not something a consumer can decode.
                if block_flags[i] && !value.is_null() {
                    to_json(value)
                } else {
                    csv_cell(value)
                }
            })
            .collect();
        write_csv_row(out, &cells, delim, &mut neutralised)?;
        count += 1;
    }
    report_diagnostics(label, reader.trailing_diagnostics());
    // A dataset with a schema but no rows is a valid document (§4.2). Emitting
    // nothing at all would make it indistinguishable from a failed conversion,
    // and a consumer reading the CSV back would find no columns.
    if header.is_none() {
        if let Some(fl) = reader.field_lists().first() {
            let names: Vec<String> = fl.fields().iter().map(|f| f.name.clone()).collect();
            if !names.is_empty() {
                write_csv_row(out, &names, delim, &mut neutralised)?;
            }
        }
    }
    if neutralised > 0 {
        eprintln!(
            "warning: {}: {} cell(s) began with `=`, `+`, `-`, `@`, TAB or CR and were prefixed with a tab so a spreadsheet does not execute them as formulas",
            label, neutralised
        );
    }
    Ok(count)
}

fn csv_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => {
            if f.is_finite() {
                f.to_string()
            } else {
                String::new()
            }
        }
        Value::String(s) => s.clone(),
        // Mirrors the JSON projection, which never leaks a secret.
        Value::Secret(_) => "[SECRET]".to_string(),
        Value::Object(_) | Value::Array(_) => to_json(value),
    }
}

fn write_csv_row(
    out: &mut dyn Write,
    cells: &[String],
    delim: char,
    neutralised: &mut usize,
) -> Result<(), CmdError> {
    let mut line = String::new();
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            line.push(delim);
        }
        let (text, was_formula) = csv_escape(cell, delim);
        if was_formula {
            *neutralised += 1;
        }
        line.push_str(&text);
    }
    line.push('\n');
    out.write_all(line.as_bytes()).map_err(write_err)
}

fn csv_escape(cell: &str, delim: char) -> (String, bool) {
    // Spreadsheets read a leading `=`, `+`, `-`, `@`, TAB or CR as the start of
    // a formula, and quoting does not stop them — DDE payloads execute on open.
    // The records here come from datasets §16 assumes are untrusted, and
    // exporting one to CSV for a human to open is precisely the workflow, so
    // the cell is prefixed with a tab: the value survives, the formula does not.
    let formula = matches!(cell.as_bytes().first(), Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r'));
    let needs_quote = formula
        || cell.contains(delim)
        || cell.contains('"')
        || cell.contains('\n')
        || cell.contains('\r');
    if needs_quote {
        let escaped = cell.replace('"', "\"\"");
        if formula {
            (format!("\"\t{}\"", escaped), true)
        } else {
            (format!("\"{}\"", escaped), false)
        }
    } else {
        (cell.to_string(), false)
    }
}

// ─── jsonl / csv → synxl ─────────────────────────────────────

/// Reopens the input. Inference needs the whole file before the first record
/// can be written, and reading it twice is what keeps memory at one line.
type Reopen<'a> = &'a mut dyn FnMut() -> Result<Box<dyn BufRead>, CmdError>;

/// What a JSONL column's values imply about its declaration (§5, §8).
///
/// JSON distinguishes `8` from `8.0`, so a hint is only emitted when *every*
/// non-null value in the column has the same kind. A mixed int/float column is
/// left untyped on purpose: automatic casting (§8.1) then reproduces each value
/// exactly, whereas a `(float)` hint would turn every `8` into `8.0`.
#[derive(Clone)]
struct ColumnInfo {
    kinds: u8,
    block: bool,
}

const KIND_INT: u8 = 1;
const KIND_FLOAT: u8 = 2;
const KIND_BOOL: u8 = 4;
const KIND_STRING: u8 = 8;
const KIND_STRUCT: u8 = 16;

impl ColumnInfo {
    fn new() -> Self {
        Self { kinds: 0, block: false }
    }

    fn observe(&mut self, value: &Value) {
        match value {
            // §7.2 — an unset field is null and says nothing about the type.
            Value::Null => {}
            Value::Int(_) => self.kinds |= KIND_INT,
            Value::Float(_) => self.kinds |= KIND_FLOAT,
            Value::Bool(_) => self.kinds |= KIND_BOOL,
            Value::String(s) | Value::Secret(s) => {
                self.kinds |= KIND_STRING;
                if string_needs_block(s) {
                    self.block = true;
                }
            }
            // §5.3 — structure has no inline form.
            Value::Object(_) | Value::Array(_) => {
                self.kinds |= KIND_STRUCT;
                self.block = true;
            }
        }
    }

    fn type_hint(&self) -> Option<&'static str> {
        if self.block {
            return None;
        }
        match self.kinds {
            KIND_INT => Some("int"),
            KIND_FLOAT => Some("float"),
            KIND_BOOL => Some("bool"),
            KIND_STRING => Some("string"),
            _ => None,
        }
    }

    fn to_decl(&self, name: &str) -> FieldDecl {
        make_decl(name, self.type_hint(), self.block)
    }
}

/// §5.3.2 — a `block` field must not carry a type.
fn make_decl(name: &str, hint: Option<&str>, block: bool) -> FieldDecl {
    if block {
        return FieldDecl::new_block(name);
    }
    let mut decl = FieldDecl::new(name);
    decl.type_hint = hint.map(|t| t.to_string());
    decl
}

/// Values a writer would have to promote to a block (§14.3): multi-line text,
/// or text that needs quoting but contains both quote characters (§7.4 has no
/// escapes). A superset of the core writer's rule, so the field list stays
/// stable across records — and so no record can drive the writer into the
/// arity-0 `Unwritable` case, since the column left inline never needs one.
fn string_needs_block(s: &str) -> bool {
    s.contains('\n') || s.contains('\r') || (s.contains('"') && s.contains('\''))
}

/// Emits records under a field list, re-declaring it whenever the writer's
/// rendering changes (§4.2 allows a new `!fields` line at any point).
struct RecordEmitter<'a> {
    out: &'a mut dyn Write,
    current: Option<String>,
    started: bool,
}

impl<'a> RecordEmitter<'a> {
    fn new(out: &'a mut dyn Write) -> Self {
        Self { out, current: None, started: false }
    }

    fn write_record(&mut self, fields: &[FieldDecl], value: &Value) -> Result<(), CmdError> {
        let (fields_line, body) = render_record(fields, value)?;
        self.write_prologue()?;
        if self.current.as_deref() != Some(fields_line.as_str()) {
            self.out.write_all(fields_line.as_bytes()).map_err(write_err)?;
            self.out.write_all(b"\n").map_err(write_err)?;
            self.current = Some(fields_line);
        }
        self.out.write_all(body.as_bytes()).map_err(write_err)
    }

    /// A dataset with a schema but no rows still declares it (§4.2).
    fn write_schema(&mut self, fields: &[FieldDecl]) -> Result<(), CmdError> {
        self.write_prologue()?;
        if self.current.is_none() && !fields.is_empty() {
            let line = canonical_fields_line(fields)?;
            self.out.write_all(line.as_bytes()).map_err(write_err)?;
            self.out.write_all(b"\n").map_err(write_err)?;
            self.current = Some(line);
        }
        Ok(())
    }

    fn write_prologue(&mut self) -> Result<(), CmdError> {
        if !self.started {
            self.out.write_all(b"!synxl 1\n").map_err(write_err)?;
            self.started = true;
        }
        Ok(())
    }
}

/// §5.1 — the field-name production excludes `;`, `[`, `(`, `:` and whitespace.
fn check_field_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("empty field name".to_string());
    }
    if name.len() > 255 {
        return Err(format!("field name is {} bytes, limit is 255 (§13)", name.len()));
    }
    if name
        .chars()
        .any(|c| c == ';' || c == '[' || c == '(' || c == ':' || c.is_whitespace())
    {
        return Err(format!(
            "`{}` is not a valid SYNXL field name (§5.1: no `;` `[` `(` `:` or whitespace)",
            name
        ));
    }
    Ok(())
}

/// §5.3.4 — a field list needs at least one inline field. When every column is
/// structural, synthesize the identifier such a dataset would need anyway
/// rather than silently flattening one of the real columns.
fn ensure_inline_field(names: &[String], fields: &mut Vec<FieldDecl>) -> Option<String> {
    if !fields.is_empty() && fields.iter().all(|f| f.block) {
        let mut name = "_index".to_string();
        while names.iter().any(|n| *n == name) {
            name.push('_');
        }
        fields.insert(0, make_decl(&name, Some("int"), false));
        return Some(name);
    }
    None
}

fn jsonl_to_synxl(reopen: Reopen<'_>, out: &mut dyn Write) -> Result<usize, CmdError> {
    // Pass 1 — infer the schema. Field order is lexicographic: a parsed object
    // is a hash map, so JSON key order is not recoverable, and sorted order is
    // what the canonical JSON projection uses anyway (§12.1).
    let mut columns: BTreeMap<String, ColumnInfo> = BTreeMap::new();
    for (i, line) in reopen()?.lines().enumerate() {
        let line = line.map_err(read_err)?;
        if line.trim().is_empty() {
            continue;
        }
        let value = parse_jsonl_line(&line, i + 1)?;
        if let Value::Object(map) = &value {
            for (key, val) in map {
                check_field_name(key)
                    .map_err(|e| CmdError::Format(format!("line {}: {}", i + 1, e)))?;
                columns
                    .entry(key.clone())
                    .or_insert_with(ColumnInfo::new)
                    .observe(val);
            }
        }
    }

    let names: Vec<String> = columns.keys().cloned().collect();
    let mut fields: Vec<FieldDecl> = columns.iter().map(|(n, c)| c.to_decl(n)).collect();
    let index_field = ensure_inline_field(&names, &mut fields);

    // Pass 2 — write. A second scan costs CPU; holding the file would cost
    // memory proportional to a dataset §13 refuses to bound.
    let mut emitter = RecordEmitter::new(out);
    let mut count = 0usize;
    for (i, line) in reopen()?.lines().enumerate() {
        let line = line.map_err(read_err)?;
        if line.trim().is_empty() {
            continue;
        }
        let mut value = parse_jsonl_line(&line, i + 1)?;
        if let (Some(name), Value::Object(map)) = (&index_field, &mut value) {
            map.insert(name.clone(), Value::Int(count as i64));
        }
        // §12.1 — a field the record does not have MUST NOT appear in its
        // object. Declaring the union of all columns for every record would
        // add `"lang":null` to rows written before that column existed, so the
        // field list is narrowed per record and `RecordEmitter` re-declares it
        // whenever the shape changes (§4.2), exactly as the source did.
        let present: Vec<FieldDecl> = fields
            .iter()
            .filter(|f| {
                value.as_object().map(|m| m.contains_key(&f.name)).unwrap_or(false)
            })
            .cloned()
            .collect();
        if present.is_empty() {
            return Err(CmdError::Format(format!(
                "line {}: object has no fields; SYNXL needs at least one (§5.3.4)",
                i + 1
            )));
        }
        emitter.write_record(&present, &value)?;
        count += 1;
    }
    if count == 0 {
        // A prologue alone is a valid, empty document (§4.2).
        emitter.write_prologue()?;
    }
    Ok(count)
}

fn parse_jsonl_line(line: &str, line_no: usize) -> Result<Value, CmdError> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| CmdError::Format(format!("line {}: invalid JSON: {}", line_no, e)))?;
    match value {
        Value::Object(_) => Ok(value),
        _ => Err(CmdError::Format(format!(
            "line {}: JSONL record must be a JSON object",
            line_no
        ))),
    }
}

fn csv_to_synxl(reopen: Reopen<'_>, delim: char, out: &mut dyn Write) -> Result<usize, CmdError> {
    // Pass 1 — header plus per-column statistics.
    let mut rows = CsvReader::new(reopen()?, delim);
    let header = match rows.next_row()? {
        Some(h) => h,
        None => {
            let mut emitter = RecordEmitter::new(out);
            emitter.write_prologue()?;
            return Ok(0);
        }
    };
    for (i, name) in header.iter().enumerate() {
        check_field_name(name)
            .map_err(|e| CmdError::Format(format!("CSV column {}: {}", i + 1, e)))?;
        // §5.1 — duplicates are a hard error, so reject them here rather than
        // emitting a document that cannot be read back.
        if header[..i].contains(name) {
            return Err(CmdError::Format(format!("duplicate CSV column name `{}`", name)));
        }
    }

    // A column is `int` only when every non-empty cell parses as one.
    let mut kinds: Vec<CsvKind> = vec![CsvKind::new(); header.len()];
    let mut row_no = 1usize;
    while let Some(row) = rows.next_row()? {
        row_no += 1;
        check_row_arity(&row, &header, row_no)?;
        for (c, cell) in row.iter().enumerate() {
            kinds[c].observe(cell);
        }
    }

    let mut fields: Vec<FieldDecl> = header
        .iter()
        .enumerate()
        .map(|(c, n)| make_decl(n, kinds[c].type_hint(), kinds[c].block))
        .collect();
    let index_field = ensure_inline_field(&header, &mut fields);

    // Pass 2 — write.
    let mut rows = CsvReader::new(reopen()?, delim);
    rows.next_row()?;
    let mut emitter = RecordEmitter::new(out);
    let mut count = 0usize;
    let mut row_no = 1usize;
    while let Some(row) = rows.next_row()? {
        row_no += 1;
        check_row_arity(&row, &header, row_no)?;
        let mut map = synx_core::Map::with_capacity_and_hasher(header.len() + 1, Default::default());
        for (c, name) in header.iter().enumerate() {
            map.insert(name.clone(), kinds[c].cast(&row[c]));
        }
        if let Some(name) = &index_field {
            map.insert(name.clone(), Value::Int(count as i64));
        }
        emitter.write_record(&fields, &Value::Object(map))?;
        count += 1;
    }
    if count == 0 {
        emitter.write_schema(&fields)?;
    }
    Ok(count)
}

fn check_row_arity(row: &[String], header: &[String], row_no: usize) -> Result<(), CmdError> {
    if row.len() != header.len() {
        return Err(CmdError::Format(format!(
            "CSV row {} has {} field(s), header declares {}",
            row_no,
            row.len(),
            header.len()
        )));
    }
    Ok(())
}

/// Column statistics for CSV, whose cells are always text until proven otherwise.
#[derive(Clone)]
struct CsvKind {
    seen: bool,
    ints: bool,
    floats: bool,
    bools: bool,
    block: bool,
}

impl CsvKind {
    fn new() -> Self {
        Self { seen: false, ints: true, floats: true, bools: true, block: false }
    }

    fn observe(&mut self, cell: &str) {
        // An empty cell is null (§7.2) and constrains nothing.
        if cell.is_empty() {
            return;
        }
        self.seen = true;
        if cell.parse::<i64>().is_err() {
            self.ints = false;
        }
        if !cell.parse::<f64>().map(|f| f.is_finite()).unwrap_or(false) {
            self.floats = false;
        }
        if cell != "true" && cell != "false" {
            self.bools = false;
        }
        if string_needs_block(cell) {
            self.block = true;
        }
    }

    /// CSV cells are text, so a column is numeric only when every non-empty
    /// cell parses; a single stray value makes the whole column a string.
    fn type_hint(&self) -> Option<&'static str> {
        if self.block || !self.seen {
            return None;
        }
        if self.ints {
            Some("int")
        } else if self.floats {
            Some("float")
        } else if self.bools {
            Some("bool")
        } else {
            Some("string")
        }
    }

    fn cast(&self, cell: &str) -> Value {
        if cell.is_empty() {
            return Value::Null;
        }
        if self.block {
            return Value::String(cell.to_string());
        }
        if self.ints {
            if let Ok(n) = cell.parse::<i64>() {
                return Value::Int(n);
            }
        } else if self.floats {
            if let Ok(f) = cell.parse::<f64>() {
                return Value::Float(f);
            }
        } else if self.bools {
            return Value::Bool(cell == "true");
        }
        Value::String(cell.to_string())
    }
}

/// Streaming RFC 4180 reader: `""` escapes, embedded newlines inside quotes,
/// CRLF or LF. One row is live at a time, matching the SYNXL side.
struct CsvReader<R: BufRead> {
    inner: R,
    delim: char,
    row: Vec<String>,
    cell: String,
    quoted: bool,
    pending: std::collections::VecDeque<Vec<String>>,
    first: bool,
    eof: bool,
}

impl<R: BufRead> CsvReader<R> {
    fn new(inner: R, delim: char) -> Self {
        Self {
            inner,
            delim,
            row: Vec::new(),
            cell: String::new(),
            quoted: false,
            pending: std::collections::VecDeque::new(),
            first: true,
            eof: false,
        }
    }

    fn next_row(&mut self) -> Result<Option<Vec<String>>, CmdError> {
        loop {
            if let Some(row) = self.pending.pop_front() {
                return Ok(Some(row));
            }
            if self.eof {
                return Ok(None);
            }
            let mut line = String::new();
            // A quoted cell may span lines, so a physical line is a chunk to
            // feed, not a row.
            let read = self.inner.read_line(&mut line).map_err(read_err)?;
            if read == 0 {
                self.eof = true;
                if self.quoted {
                    return Err(CmdError::Format("unterminated quoted CSV field".to_string()));
                }
                if !self.cell.is_empty() || !self.row.is_empty() {
                    let cell = std::mem::take(&mut self.cell);
                    self.row.push(cell);
                    return Ok(Some(std::mem::take(&mut self.row)));
                }
                return Ok(None);
            }
            if self.first {
                self.first = false;
                if let Some(stripped) = line.strip_prefix('\u{feff}') {
                    line = stripped.to_string();
                }
            }
            self.feed(&line);
        }
    }

    fn feed(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if self.quoted {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        self.cell.push('"');
                    } else {
                        self.quoted = false;
                    }
                } else {
                    self.cell.push(c);
                }
                continue;
            }
            if c == '"' && self.cell.is_empty() {
                self.quoted = true;
            } else if c == self.delim {
                let cell = std::mem::take(&mut self.cell);
                self.row.push(cell);
            } else if c == '\r' || c == '\n' {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                let cell = std::mem::take(&mut self.cell);
                self.row.push(cell);
                let row = std::mem::take(&mut self.row);
                self.pending.push_back(row);
            } else {
                self.cell.push(c);
            }
        }
    }
}

// ─── Shared helpers ──────────────────────────────────────────

fn normalize_format(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "ndjson" => "jsonl".to_string(),
        other => other.to_string(),
    }
}

/// The machine-readable diagnostic channel opened by `--diagnostics-out`.
///
/// §11.2 forbids stderr text from being the *only* channel, and for good
/// reason: a consumer that decides whether to accept a batch by counting
/// `CastFailed` should not have to parse an unversioned human-readable line.
static DIAG_SINK: Mutex<Option<BufWriter<File>>> = Mutex::new(None);

/// A `-` input path means stdin, spooled to a temporary file for the duration
/// of the command.
///
/// Every direction here either re-reads its input (schema inference, §11.1
/// pre-flight) or could be asked to, and stdin cannot be rewound. Spooling
/// keeps live memory at one record — which is the §15.1 guarantee — at the
/// cost of disk, rather than holding a dataset §13 deliberately does not bound.
struct StdinSpool {
    path: PathBuf,
}

/// The spool of the running command, for [`exit`] to remove.
static SPOOL_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Ends the process, removing the stdin spool first.
///
/// why: `process::exit` runs no destructors, so every failed
/// `synx synxl … -` left the piped dataset behind in the temp directory.
fn exit(code: i32) -> ! {
    remove_spool();
    process::exit(code)
}

fn remove_spool() {
    if let Ok(mut guard) = SPOOL_PATH.lock() {
        if let Some(path) = guard.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Creates the spool file: a new file under a name nobody could have
/// prepared, readable by its owner only.
///
/// safety: the name was `synxl-stdin-<pid>.synxl` opened with `File::create`.
/// On a shared temp directory another user could put a link there in advance
/// and have the piped dataset written over any file of the caller, and the
/// spool itself was readable by everyone.
fn create_spool() -> io::Result<(PathBuf, File)> {
    let dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    for attempt in 0..64u32 {
        let tag = nanos.wrapping_mul(2_654_435_761).wrapping_add(attempt);
        let path = dir.join(format!("synxl-stdin-{}-{:08x}.synxl", process::id(), tag));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free spool name"))
}

impl StdinSpool {
    fn take(file: &Path) -> Result<Option<Self>, CmdError> {
        if file.as_os_str() != "-" {
            return Ok(None);
        }
        let (path, file) =
            create_spool().map_err(|e| CmdError::Io(format!("cannot spool stdin: {}", e)))?;
        if let Ok(mut guard) = SPOOL_PATH.lock() {
            *guard = Some(path.clone());
        }
        let mut sink = BufWriter::new(file);
        io::copy(&mut io::stdin().lock(), &mut sink)
            .map_err(|e| CmdError::Io(format!("cannot read stdin: {}", e)))?;
        sink.flush().map_err(write_err)?;
        Ok(Some(Self { path }))
    }
}

impl Drop for StdinSpool {
    fn drop(&mut self) {
        remove_spool();
        let _ = fs::remove_file(&self.path);
    }
}

/// Point the machine-readable channel at `path`, one JSON object per line.
fn open_diagnostics(path: &Path) -> Result<(), CmdError> {
    let file = File::create(path)
        .map_err(|e| CmdError::Io(format!("cannot write {}: {}", path.display(), e)))?;
    *DIAG_SINK.lock().unwrap() = Some(BufWriter::new(file));
    Ok(())
}

fn flush_diagnostics() {
    if let Ok(mut guard) = DIAG_SINK.lock() {
        if let Some(sink) = guard.as_mut() {
            let _ = sink.flush();
        }
    }
}

/// §11.2 — diagnostics are recoverable, so they go to stderr as warnings and
/// leave stdout carrying only data. With `--diagnostics-out` they are also
/// written as NDJSON, which is what a pipeline can actually act on.
fn report_diagnostics(label: &str, diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        eprintln!(
            "warning: {}:{}: record {}: {}: {}",
            label, d.line, d.record_index, d.kind, d.message
        );
    }
    if diagnostics.is_empty() {
        return;
    }
    if let Ok(mut guard) = DIAG_SINK.lock() {
        if let Some(sink) = guard.as_mut() {
            for d in diagnostics {
                let line = format!(
                    "{{\"file\":{},\"record\":{},\"line\":{},\"kind\":{},\"message\":{}}}\n",
                    json_string(label),
                    d.record_index,
                    d.line,
                    json_string(&d.kind.to_string()),
                    json_string(&d.message),
                );
                let _ = sink.write_all(line.as_bytes());
            }
        }
    }
}

/// Minimal RFC 8259 string escaping for the diagnostic channel.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn open_reader(path: &Path) -> Result<BufReader<File>, CmdError> {
    File::open(path)
        .map(|f| BufReader::with_capacity(64 * 1024, f))
        .map_err(|e| CmdError::Io(format!("cannot read {}: {}", path.display(), e)))
}

/// An output that only becomes visible if the whole command succeeds (§11.1).
///
/// A file target is written to a sibling temporary and renamed into place on
/// [`commit`](AtomicOutput::commit); dropping without committing removes the
/// temporary. That keeps a pre-existing `--output` file intact when the parse
/// turns out to be a hard error — truncating it up front and then deleting it,
/// as a plain `File::create` plus cleanup does, destroys data the command was
/// never asked to touch.
///
/// A stdout target cannot be rolled back once bytes have left the process, so
/// the caller pre-flights the input instead (see [`precheck`]).
struct AtomicOutput {
    sink: Box<dyn Write>,
    temp: Option<(PathBuf, PathBuf)>,
}

impl AtomicOutput {
    fn open(output: &Option<PathBuf>) -> Result<Self, CmdError> {
        match output {
            Some(path) => {
                let temp = temp_sibling(path);
                let file = File::create(&temp).map_err(|e| {
                    CmdError::Io(format!("cannot write {}: {}", temp.display(), e))
                })?;
                Ok(Self {
                    sink: Box::new(BufWriter::new(file)),
                    temp: Some((temp, path.clone())),
                })
            }
            None => Ok(Self { sink: Box::new(BufWriter::new(io::stdout())), temp: None }),
        }
    }

    fn commit(mut self) -> Result<(), CmdError> {
        self.sink.flush().map_err(write_err)?;
        if let Some((temp, dest)) = self.temp.take() {
            // Drop the writer before renaming: Windows refuses to rename a file
            // that still has an open handle.
            self.sink = Box::new(io::sink());
            fs::rename(&temp, &dest).map_err(|e| {
                let _ = fs::remove_file(&temp);
                CmdError::Io(format!("cannot write {}: {}", dest.display(), e))
            })?;
        }
        Ok(())
    }
}

impl Write for AtomicOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sink.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}

impl Drop for AtomicOutput {
    fn drop(&mut self) {
        if let Some((temp, _)) = self.temp.take() {
            self.sink = Box::new(io::sink());
            let _ = fs::remove_file(&temp);
        }
    }
}

/// A temporary path next to `path`, so the later rename stays on one filesystem.
fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".synxl-tmp-{}", process::id()));
    path.with_file_name(name)
}

/// Read the document through once, discarding output, so that a hard error
/// (§11.1) surfaces *before* the first byte of a partial result reaches stdout.
///
/// Only needed for an un-rewindable sink; a file target is made atomic by
/// [`AtomicOutput`] instead, and pays no second pass.
fn precheck(reopen: Reopen<'_>) -> Result<(), CmdError> {
    let mut reader = SynxlStreamReader::new(reopen()?)?;
    while let Some(item) = reader.next() {
        item?;
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stdin_spool_is_a_new_private_file() {
        let (first, _a) = create_spool().expect("spool");
        let (second, _b) = create_spool().expect("spool");
        assert_ne!(first, second);
        assert!(first.is_file() && second.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // An existing file is never opened: the name is taken, another is used.
        fs::write(&first, "someone else's file").unwrap();
        let (third, _c) = create_spool().expect("spool");
        assert_ne!(third, first);
        assert_eq!(fs::read_to_string(&first).unwrap(), "someone else's file");
        for path in [first, second, third] {
            let _ = fs::remove_file(path);
        }
    }
    use std::io::{Cursor, Read};
    use synx_core::synxl::parse_lines;

    fn convert<F>(f: F) -> String
    where
        F: FnOnce(&mut dyn Write) -> Result<usize, CmdError>,
    {
        let mut buf: Vec<u8> = Vec::new();
        f(&mut buf).expect("conversion failed");
        String::from_utf8(buf).expect("utf-8")
    }

    /// A `Reopen` over an in-memory document, standing in for reopening a file.
    fn source(text: &str) -> impl FnMut() -> Result<Box<dyn BufRead>, CmdError> {
        let bytes = text.as_bytes().to_vec();
        move || Ok(Box::new(Cursor::new(bytes.clone())) as Box<dyn BufRead>)
    }

    fn to_synxl_from_jsonl(text: &str) -> String {
        let mut open = source(text);
        convert(|w| jsonl_to_synxl(&mut open, w))
    }

    fn to_synxl_from_csv(text: &str, delim: char) -> String {
        let mut open = source(text);
        convert(|w| csv_to_synxl(&mut open, delim, w))
    }

    fn to_ndjson(text: &str) -> String {
        convert(|w| synxl_to_json(Cursor::new(text), "test", true, false, w))
    }

    // ─── jsonl → synxl ───────────────────────────────────

    #[test]
    fn jsonl_infers_conservative_types() {
        let src = "{\"id\":1,\"score\":1.5,\"ok\":true,\"name\":\"Wario\"}\n\
                   {\"id\":2,\"score\":2.25,\"ok\":false,\"name\":\"Waluigi\"}\n";
        let out = to_synxl_from_jsonl(src);
        assert!(out.starts_with("!synxl 1\n"), "{}", out);
        let fields = out.lines().nth(1).unwrap();
        assert!(fields.contains("id(int)"), "{}", fields);
        assert!(fields.contains("score(float)"), "{}", fields);
        assert!(fields.contains("ok(bool)"), "{}", fields);
        assert!(fields.contains("name(string)"), "{}", fields);
    }

    #[test]
    fn jsonl_mixed_int_float_column_stays_untyped() {
        // A `(float)` hint here would turn `8` into `8.0`; leaving the column
        // untyped lets automatic casting (§8.1) reproduce both exactly.
        let src = "{\"score\":9.5}\n{\"score\":8}\n";
        let out = to_synxl_from_jsonl(src);
        assert_eq!(out.lines().nth(1).unwrap(), "!fields score");
        assert_eq!(to_ndjson(&out), src);
    }

    #[test]
    fn jsonl_mixed_column_gets_no_type() {
        let src = "{\"a\":1}\n{\"a\":\"x\"}\n";
        let out = to_synxl_from_jsonl(src);
        assert_eq!(out.lines().nth(1).unwrap(), "!fields a");
        assert_eq!(to_ndjson(&out), "{\"a\":1}\n{\"a\":\"x\"}\n");
    }

    #[test]
    fn jsonl_promotes_multiline_and_composite_to_block() {
        let src = "{\"id\":1,\"text\":\"a\\nb\",\"meta\":{\"k\":\"v\"},\"tags\":[1,2]}\n";
        let out = to_synxl_from_jsonl(src);
        let fields = out.lines().nth(1).unwrap();
        assert!(fields.contains("text[block]"), "{}", fields);
        assert!(fields.contains("meta[block]"), "{}", fields);
        assert!(fields.contains("tags[block]"), "{}", fields);
        assert!(fields.contains("id(int)"), "{}", fields);
        assert_eq!(
            to_ndjson(&out),
            "{\"id\":1,\"meta\":{\"k\":\"v\"},\"tags\":[1,2],\"text\":\"a\\nb\"}\n"
        );
    }

    #[test]
    fn jsonl_round_trips_through_synxl() {
        // Keys are already sorted, so the output is comparable byte-for-byte
        // with the input (§12.1 sorts them anyway).
        let src = "{\"id\":1,\"name\":\"Wario\",\"note\":\"has ; semicolon\"}\n\
                   {\"id\":2,\"name\":\"42\",\"note\":null}\n";
        let synxl = to_synxl_from_jsonl(src);
        // `"42"` must come back a string, not an int.
        assert_eq!(to_ndjson(&synxl), src);
    }

    #[test]
    fn jsonl_all_block_columns_get_synthetic_inline_field() {
        // §5.3.4 — arity must be ≥ 1.
        let src = "{\"meta\":{\"k\":1}}\n{\"meta\":{\"k\":2}}\n";
        let out = to_synxl_from_jsonl(src);
        let fields = out.lines().nth(1).unwrap();
        assert!(fields.starts_with("!fields _index(int)"), "{}", fields);
        assert_eq!(
            to_ndjson(&out),
            "{\"_index\":0,\"meta\":{\"k\":1}}\n{\"_index\":1,\"meta\":{\"k\":2}}\n"
        );
    }

    #[test]
    fn jsonl_rejects_invalid_field_names() {
        let mut buf: Vec<u8> = Vec::new();
        let mut open = source("{\"a b\":1}\n");
        let err = jsonl_to_synxl(&mut open, &mut buf).unwrap_err();
        assert!(err.message().contains("not a valid SYNXL field name"), "{:?}", err);
        assert_eq!(err.code(), 1);
    }

    // ─── csv → synxl ─────────────────────────────────────

    #[test]
    fn csv_infers_types_conservatively() {
        let src = "id,score,flag,name\n1,1.5,true,Wario\n2,2,false,42\n";
        let out = to_synxl_from_csv(src, ',');
        let fields = out.lines().nth(1).unwrap();
        assert!(fields.contains("id(int)"), "{}", fields);
        assert!(fields.contains("score(float)"), "{}", fields);
        assert!(fields.contains("flag(bool)"), "{}", fields);
        assert!(fields.contains("name(string)"), "{}", fields);
        assert_eq!(
            to_ndjson(&out),
            "{\"flag\":true,\"id\":1,\"name\":\"Wario\",\"score\":1.5}\n\
             {\"flag\":false,\"id\":2,\"name\":\"42\",\"score\":2.0}\n"
        );
    }

    #[test]
    fn csv_one_bad_cell_disqualifies_the_column() {
        let src = "n\n1\n2\nx\n";
        let out = to_synxl_from_csv(src, ',');
        assert_eq!(out.lines().nth(1).unwrap(), "!fields n(string)");
        assert_eq!(to_ndjson(&out), "{\"n\":\"1\"}\n{\"n\":\"2\"}\n{\"n\":\"x\"}\n");
    }

    #[test]
    fn csv_multiline_cell_becomes_block() {
        let src = "id,body\n1,\"line one\nline two\"\n";
        let out = to_synxl_from_csv(src, ',');
        let fields = out.lines().nth(1).unwrap();
        assert!(fields.contains("body[block]"), "{}", fields);
        assert_eq!(to_ndjson(&out), "{\"body\":\"line one\\nline two\",\"id\":1}\n");
    }

    #[test]
    fn csv_empty_cell_is_null() {
        let out = to_synxl_from_csv("a,b\n1,\n", ',');
        assert_eq!(to_ndjson(&out), "{\"a\":1,\"b\":null}\n");
    }

    #[test]
    fn csv_rejects_arity_mismatch_and_duplicates() {
        let mut buf: Vec<u8> = Vec::new();
        let mut open = source("a,b\n1\n");
        let err = csv_to_synxl(&mut open, ',', &mut buf).unwrap_err();
        assert!(err.message().contains("CSV row 2"), "{:?}", err);

        let mut buf: Vec<u8> = Vec::new();
        let mut open = source("a,a\n1,2\n");
        let err = csv_to_synxl(&mut open, ',', &mut buf).unwrap_err();
        assert!(err.message().contains("duplicate"), "{:?}", err);
    }

    #[test]
    fn csv_custom_delimiter() {
        let out = to_synxl_from_csv("a;b\n1;2\n", ';');
        assert_eq!(to_ndjson(&out), "{\"a\":1,\"b\":2}\n");
    }

    #[test]
    fn csv_reader_streams_rows_across_line_boundaries() {
        // A quoted cell spanning three physical lines, plus CRLF endings.
        let src = "a,b\r\n1,\"x\ny\nz\"\r\n2,w\r\n";
        let mut rows = CsvReader::new(Cursor::new(src), ',');
        assert_eq!(rows.next_row().unwrap().unwrap(), vec!["a", "b"]);
        assert_eq!(rows.next_row().unwrap().unwrap(), vec!["1", "x\ny\nz"]);
        assert_eq!(rows.next_row().unwrap().unwrap(), vec!["2", "w"]);
        assert!(rows.next_row().unwrap().is_none());
    }

    #[test]
    fn csv_round_trip() {
        let src = "id,name\n1,Wario\n2,\"a, b\"\n";
        let synxl = to_synxl_from_csv(src, ',');
        let back = convert(|w| synxl_to_csv(Cursor::new(&synxl), "test", ',', false, w));
        assert_eq!(back, src);
    }

    // ─── synxl → csv ─────────────────────────────────────

    #[test]
    fn synxl_to_csv_writes_header_and_quotes() {
        let src = "!synxl 1\n!fields id(int) ; name\n1 ; Wario\n2 ; 'a, b'\n";
        let out = convert(|w| synxl_to_csv(Cursor::new(src), "test", ',', false, w));
        assert_eq!(out, "id,name\n1,Wario\n2,\"a, b\"\n");
    }

    #[test]
    fn synxl_to_csv_refuses_block_fields_by_default() {
        let src = "!synxl 1\n!fields id(int) ; notes[block]\n1\n  notes hello\n";
        let mut buf: Vec<u8> = Vec::new();
        let err = synxl_to_csv(Cursor::new(src), "test", ',', false, &mut buf).unwrap_err();
        assert!(err.message().contains("notes"), "{:?}", err);
        assert!(err.message().contains("--block-json"), "{:?}", err);
        assert_eq!(err.code(), 1);
    }

    #[test]
    fn synxl_to_csv_block_json_opt_in() {
        // Every block cell is JSON under the flag — a string one included, so
        // the column has a single decodable encoding. An unset one is empty.
        let src =
            "!synxl 1\n!fields id(int) ; notes[block]\n1\n  notes\n    a 1\n2\n  notes hi\n3\n";
        let out = convert(|w| synxl_to_csv(Cursor::new(src), "test", ',', true, w));
        assert_eq!(out, "id,notes\n1,\"{\"\"a\"\":1}\"\n2,\"\"\"hi\"\"\"\n3,\n");
    }

    #[test]
    fn synxl_to_csv_refuses_a_second_field_list() {
        let src = "!synxl 1\n!fields a\n1\n!fields a ; b\n2 ; 3\n";
        let mut buf: Vec<u8> = Vec::new();
        let err = synxl_to_csv(Cursor::new(src), "test", ',', false, &mut buf).unwrap_err();
        assert!(err.message().contains("second field list"), "{:?}", err);
    }

    // ─── error model ─────────────────────────────────────

    /// A reader that hands out one chunk and then fails.
    struct Failing(bool);

    impl Read for Failing {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "disk gone"));
            }
            self.0 = true;
            let src = b"!synxl 1\n!fields a\n1\n";
            let n = src.len().min(out.len());
            out[..n].copy_from_slice(&src[..n]);
            Ok(n)
        }
    }

    #[test]
    fn a_malformed_document_and_a_broken_reader_are_different_failures() {
        let mut buf: Vec<u8> = Vec::new();
        let err = synxl_to_json(Cursor::new("nope\n"), "test", true, false, &mut buf).unwrap_err();
        assert!(err.message().contains("MissingPrologue"), "{:?}", err);
        assert_eq!(err.code(), 1);

        // The same command against a failing reader: not a spec violation.
        let mut buf: Vec<u8> = Vec::new();
        let err =
            synxl_to_json(BufReader::new(Failing(false)), "test", true, false, &mut buf)
                .unwrap_err();
        assert_eq!(err.code(), 2, "{:?}", err);
        assert!(err.message().contains("I/O error"), "{:?}", err);
    }

    #[test]
    fn unwritable_values_are_reported_not_panicked() {
        // §14.3 — promoting every column leaves arity 0, which the writer now
        // refuses instead of emitting a document that reads back differently.
        let fields = vec![FieldDecl::new_block("a"), FieldDecl::new_block("b")];
        let mut map = synx_core::Map::default();
        map.insert("a".to_string(), Value::Int(1));
        map.insert("b".to_string(), Value::Int(2));
        let mut buf: Vec<u8> = Vec::new();
        let mut emitter = RecordEmitter::new(&mut buf);
        let err = emitter.write_record(&fields, &Value::Object(map)).unwrap_err();
        assert!(err.message().contains("Unwritable"), "{:?}", err);
        assert_eq!(err.code(), 1);
    }

    // ─── split ───────────────────────────────────────────

    fn shards_of(src: &str, n: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        split_shards(
            Cursor::new(src),
            "test",
            n,
            |_| Ok(Vec::<u8>::new()),
            |_, w, _| {
                out.push(String::from_utf8(w).expect("utf-8"));
                Ok(())
            },
        )
        .expect("split failed");
        out
    }

    /// §15.3 across the whole conformance corpus, at several shard sizes.
    ///
    /// "A shard of a SYNXL document is itself a valid SYNXL document only if it
    /// begins with a prologue and the field list in effect at the split point"
    /// — so every shard must parse standalone, and concatenating their
    /// projections must reproduce the original's. Checking it on the corpus
    /// rather than on one hand-written document is what makes it a property of
    /// the splitter instead of a property of one example: the interesting
    /// inputs are the ones with a mid-file field-list change, promoted block
    /// fields and diagnostics, and the corpus already collects those.
    #[test]
    fn split_is_lossless_across_the_conformance_corpus() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests")
            .join("conformance-synxl")
            .join("cases");

        let mut cases: Vec<std::path::PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "synxl"))
            .collect();
        cases.sort();
        assert!(!cases.is_empty(), "no cases found in {}", dir.display());

        let mut failures: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for path in &cases {
            let src = fs::read_to_string(path).expect("read case");
            // Hard-error cases (§11.1) have nothing to shard.
            let Ok(whole) = parse_lines(&src) else { continue };
            let name = path.file_stem().unwrap().to_string_lossy().to_string();

            for per_shard in [1usize, 2, 3, 1000] {
                checked += 1;
                let shards = match std::panic::catch_unwind(|| shards_of(&src, per_shard)) {
                    Ok(s) => s,
                    Err(_) => {
                        failures.push(format!("{name} @ n={per_shard}: split panicked"));
                        continue;
                    }
                };

                let mut rejoined: Vec<Value> = Vec::new();
                let mut bad = false;
                for (i, shard) in shards.iter().enumerate() {
                    match parse_lines(shard) {
                        Ok(doc) => rejoined.extend(doc.records),
                        Err(err) => {
                            failures.push(format!(
                                "{name} @ n={per_shard}: shard {i} does not parse standalone: {err}\n       shard: {}",
                                shard.replace('\n', "\\n")
                            ));
                            bad = true;
                            break;
                        }
                    }
                }
                if bad {
                    continue;
                }

                let rejoined_json = synx_core::synxl::records_to_json_array(&rejoined);
                if rejoined_json != whole.to_json() {
                    failures.push(format!(
                        "{name} @ n={per_shard}: rejoined shards differ from the source\n       got:      {rejoined_json}\n       expected: {}",
                        whole.to_json()
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "\n\n{} of {checked} shard checks failed:\n  - {}\n",
            failures.len(),
            failures.join("\n  - ")
        );
    }

    #[test]
    fn split_shards_carry_prologue_and_field_list() {
        let src = "!synxl 1\n!fields id(int) ; name\n1 ; a\n2 ; b\n3 ; c\n4 ; d\n5 ; e\n";
        let shards = shards_of(src, 2);
        assert_eq!(shards.len(), 3);
        for shard in &shards {
            let mut lines = shard.lines();
            assert_eq!(lines.next(), Some("!synxl 1"));
            // §15.3 — the author's own `!fields` line, via FieldList::source().
            assert_eq!(lines.next(), Some("!fields id(int) ; name"));
            parse_lines(shard).expect("shard is a valid document");
        }
        assert_eq!(parse_lines(&shards[0]).unwrap().len(), 2);
        assert_eq!(parse_lines(&shards[2]).unwrap().len(), 1);
    }

    #[test]
    fn split_preserves_every_record_in_order() {
        let src = "!synxl 1\n!fields id(int) ; name\n1 ; a\n2 ; b\n3 ; c\n4 ; d\n5 ; e\n";
        let whole = parse_lines(src).unwrap().to_ndjson();
        let joined: String = shards_of(src, 2)
            .iter()
            .map(|s| parse_lines(s).unwrap().to_ndjson())
            .collect();
        assert_eq!(joined, whole);
    }

    #[test]
    fn split_uses_the_field_list_in_effect_at_the_cut() {
        // The cut falls after the schema change, so shard 2 must carry the
        // *second* field list, not the first (§15.3).
        let src = "!synxl 1\n!fields id(int) ; name\n1 ; a\n2 ; b\n\
                   !fields id(int) ; name ; extra\n3 ; c ; x\n4 ; d ; y\n";
        let shards = shards_of(src, 3);
        assert_eq!(shards.len(), 2);
        assert!(
            shards[1].starts_with("!synxl 1\n!fields id(int) ; name ; extra\n"),
            "{}",
            shards[1]
        );
        let whole = parse_lines(src).unwrap().to_ndjson();
        let joined: String = shards
            .iter()
            .map(|s| parse_lines(s).unwrap().to_ndjson())
            .collect();
        assert_eq!(joined, whole);
        // Shard 1 spans the schema change, so it re-declares it inline.
        assert_eq!(parse_lines(&shards[0]).unwrap().field_lists.len(), 2);
    }

    #[test]
    fn split_keeps_blocks_with_their_record() {
        let src = "!synxl 1\n!fields id(int) ; notes[block]\n1\n  notes hi\n2\n  notes there\n";
        let shards = shards_of(src, 1);
        assert_eq!(shards.len(), 2);
        for shard in &shards {
            assert_eq!(parse_lines(shard).expect("shard parses").len(), 1);
        }
        assert_eq!(
            parse_lines(&shards[1]).unwrap().to_ndjson(),
            "{\"id\":2,\"notes\":\"there\"}\n"
        );
    }

    #[test]
    fn split_of_an_empty_document_writes_nothing() {
        assert!(shards_of("!synxl 1\n", 10).is_empty());
    }

    // ─── streaming ───────────────────────────────────────

    /// Synthesizes a SYNXL document on the fly, so a test can push more bytes
    /// through than SYNX's (lifted, §13) 16 MiB whole-file cap without ever
    /// allocating them.
    struct GeneratedDoc {
        remaining: usize,
        next_id: usize,
        buf: Vec<u8>,
        pos: usize,
    }

    impl GeneratedDoc {
        fn new(records: usize) -> Self {
            Self {
                remaining: records,
                next_id: 0,
                buf: b"!synxl 1\n!fields id(int) ; payload\n".to_vec(),
                pos: 0,
            }
        }
    }

    impl Read for GeneratedDoc {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.buf.len() {
                if self.remaining == 0 {
                    return Ok(0);
                }
                self.buf = format!("{}; {}\n", self.next_id, "x".repeat(128)).into_bytes();
                self.pos = 0;
                self.next_id += 1;
                self.remaining -= 1;
            }
            let n = (self.buf.len() - self.pos).min(out.len());
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    /// Counts what it was given and keeps only the first bytes, so a shard of
    /// any size costs nothing to check.
    struct CountingSink {
        bytes: usize,
        head: Vec<u8>,
    }

    impl CountingSink {
        fn new() -> Self {
            Self { bytes: 0, head: Vec::new() }
        }
    }

    impl Write for CountingSink {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.bytes += data.len();
            if self.head.len() < 64 {
                let n = data.len().min(64 - self.head.len());
                self.head.extend_from_slice(&data[..n]);
            }
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// 140 000 × ~137 B ≈ 19 MB — past SYNX's 16 MiB whole-input cap, which §13
    /// lifts for SYNXL precisely so this works. Nothing here materialises the
    /// document or a shard.
    const STREAM_RECORDS: usize = 140_000;

    #[test]
    fn split_streams_a_document_larger_than_the_synx_file_cap() {
        let mut written = 0usize;
        let mut shards = 0usize;
        let mut heads: Vec<String> = Vec::new();

        let total = split_shards(
            BufReader::new(GeneratedDoc::new(STREAM_RECORDS)),
            "test",
            50_000,
            |_| Ok(CountingSink::new()),
            |_, sink, count| {
                written += sink.bytes;
                shards += 1;
                heads.push(String::from_utf8_lossy(&sink.head).into_owned());
                assert!(count <= 50_000, "{} records in one shard", count);
                Ok(())
            },
        )
        .expect("streaming split failed");

        assert_eq!(total, STREAM_RECORDS);
        assert_eq!(shards, 3);
        assert!(written > 16 * 1024 * 1024, "only {} bytes written", written);
        for head in &heads {
            // §15.3 — prologue plus the field list in effect, in every shard.
            assert!(head.starts_with("!synxl 1\n!fields id(int) ; payload\n"), "{}", head);
        }
    }

    #[test]
    fn parse_streams_a_document_larger_than_the_synx_file_cap() {
        let mut sink = CountingSink::new();
        let count = synxl_to_json(
            BufReader::new(GeneratedDoc::new(STREAM_RECORDS)),
            "generated",
            true,
            false,
            &mut sink,
        )
        .expect("streaming parse failed");
        assert_eq!(count, STREAM_RECORDS);
        assert!(sink.bytes > 16 * 1024 * 1024, "only {} bytes written", sink.bytes);
        assert!(sink.head.starts_with(b"{\"id\":0,\"payload\":\"xxx"), "{:?}", sink.head);
    }
}
