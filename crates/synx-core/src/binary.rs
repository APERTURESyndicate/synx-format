//! SYNX Binary Format (.synxb) — compact binary serialization with string interning.
//!
//! Layout:
//! ```text
//! HEADER       5 bytes magic "SYNXB"
//!              1 byte  version (currently 1)
//!              1 byte  flags   (bit 0: active, bit 1: locked,
//!                               bit 2: has_metadata, bit 3: resolved,
//!                               bit 4: tool, bit 5: schema, bit 6: llm)
//! STRING_TABLE varint count
//!              for each: varint len + UTF-8 bytes
//! VALUE_TREE   Root Value (recursive, strings as table indices)
//! [METADATA]   if flag bit 2 set
//! [INCLUDES]   if flag bit 2 set
//! ```
//!
//! Type tags (1 byte):
//!   0x00 Null
//!   0x01 Bool(false)
//!   0x02 Bool(true)
//!   0x03 Int       + zigzag varint
//!   0x04 Float     + 8 bytes LE
//!   0x05 String    + varint string_table_index
//!   0x06 Array     + varint count + values
//!   0x07 Object    + varint count + (varint key_index + value) pairs
//!   0x08 Secret    + varint string_table_index

use crate::value::{
    Constraints, IncludeDirective, Map, Meta, MetaMap, Mode, ParseResult, Value,
};
use std::collections::HashMap;

const MAGIC: &[u8; 5] = b"SYNXB";
const FORMAT_VERSION: u8 = 1;

/// Upper bound on the decompressed payload a `.synxb` decode will materialise.
///
/// safety: `decompile` receives an untrusted byte slice. The declared
/// uncompressed size is a `u32` (up to 4 GiB) and the deflate stream can expand
/// far beyond its own length (a "zip bomb"), so decompression MUST be bounded
/// rather than trusting the header. 256 MiB comfortably exceeds any tree a
/// 16 MiB `.synx` source (SYNX §3) produces while refusing the pathological case.
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;

/// Depth limit for the recursive value decoder. A hostile `.synxb` can nest
/// ARRAY/OBJECT tags arbitrarily even though `parse` caps text at 128, so the
/// decoder refuses anything deeper than twice that. 3.7.3 set 1024, which is
/// itself deep enough to overflow a 2 MiB test thread in a Windows debug build
/// (and sits close to the 1 MiB main-thread stack there): the guard overflowed
/// the stack it was written to protect.
const MAX_DECODE_DEPTH: usize = 256;

/// Most memory the tree decoded from one file may take: the index of its
/// strings, every value, key and text it holds, and its declarations.
///
/// safety: a value is one byte on the wire and 72 in memory, a string is
/// stored once and copied by every reference, and deflate shrinks a run of
/// either a thousandfold: unbudgeted, a file of a few kilobytes decoded to
/// gigabytes. Everything is charged at what it occupies, and the storage of
/// a counted list, object or table is charged before it is reserved — a
/// count in the file is never memory the file has not paid for.
const MAX_DECODED_TREE_BYTES: usize = 192 * 1024 * 1024;

// What the pieces of a decoded tree are charged, in bytes — what they occupy
// on a 64-bit target. They are plain numbers so that every target and every
// port refuses a file at the same place. A copied string is charged its
// UTF-8 bytes on top.
/// One string of the table.
const TABLE_ENTRY_COST: usize = 16;
/// A list item: its slot.
const ARRAY_SLOT_COST: usize = 64;
/// An object entry: the value (64), the key (24), its hash and index slot (24).
const OBJECT_ENTRY_COST: usize = 112;
/// A marker, a marker argument or an `enum` value.
const TEXT_ITEM_COST: usize = 24;
/// One key's declarations, as an entry of its object's table.
const META_ENTRY_COST: usize = 232;
/// One object's table of declarations, as an entry of the document's.
const META_PATH_COST: usize = 112;
/// An `!include`.
const INCLUDE_COST: usize = 48;

/// What decoding has built so far, in bytes.
#[derive(Default)]
struct DecodeBudget {
    spent: usize,
}

impl DecodeBudget {
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.spent = self.spent.saturating_add(bytes);
        if self.spent > MAX_DECODED_TREE_BYTES {
            return Err(format!("decoded document exceeds {} bytes — refusing to decode", MAX_DECODED_TREE_BYTES));
        }
        Ok(())
    }

    /// Storage for `count` items read from the file, paid for: each takes at
    /// least `wire` of the `remaining` bytes and `cost` bytes in memory.
    ///
    /// safety: counts are varints straight from the file. `u64::MAX` aborted
    /// the process inside `with_capacity`; capped at the bytes that remain,
    /// an 8 KB file still reserved 930 MiB for an object it did not hold.
    /// More items than the rest of the file can spell is a truncated file,
    /// said at once; the reservation of the rest is exact and charged first.
    fn reserve(&mut self, count: usize, remaining: usize, wire: usize, cost: usize) -> Result<usize, String> {
        if count > remaining / wire {
            return Err("unexpected end of data".into());
        }
        self.charge(count.saturating_mul(cost))?;
        Ok(count)
    }

    /// An owned copy of a table string.
    ///
    /// safety: every reference copies its string, so one megabyte of text and
    /// three thousand two-byte references were 3 GiB.
    fn text(&mut self, s: &str) -> Result<String, String> {
        self.charge(s.len())?;
        Ok(s.to_string())
    }

    /// A list of table strings: `count` references of one varint each.
    fn texts(&mut self, data: &[u8], pos: &mut usize, st: &StringTableReader, count: usize) -> Result<Vec<String>, String> {
        let room = self.reserve(count, data.len().saturating_sub(*pos), 1, TEXT_ITEM_COST)?;
        let mut out = Vec::with_capacity(room);
        for _ in 0..count {
            let idx = decode_index(data, pos)?;
            out.push(self.text(st.get(idx)?)?);
        }
        Ok(out)
    }
}

/// A count or length: a varint that must fit `usize`.
fn decode_len(data: &[u8], pos: &mut usize) -> Result<usize, String> {
    usize::try_from(decode_varint(data, pos)?).map_err(|_| "length does not fit this platform".to_string())
}

/// A string-table index: a varint that must fit `u32`.
fn decode_index(data: &[u8], pos: &mut usize) -> Result<u32, String> {
    u32::try_from(decode_varint(data, pos)?).map_err(|_| "string index out of range".to_string())
}

const FLAG_ACTIVE: u8 = 0b0000_0001;
const FLAG_LOCKED: u8 = 0b0000_0010;
const FLAG_HAS_META: u8 = 0b0000_0100;
const FLAG_RESOLVED: u8 = 0b0000_1000;
const FLAG_TOOL: u8 = 0b0001_0000;
const FLAG_SCHEMA: u8 = 0b0010_0000;
const FLAG_LLM: u8 = 0b0100_0000;

const TAG_NULL: u8 = 0x00;
const TAG_FALSE: u8 = 0x01;
const TAG_TRUE: u8 = 0x02;
const TAG_INT: u8 = 0x03;
const TAG_FLOAT: u8 = 0x04;
const TAG_STRING: u8 = 0x05;
const TAG_ARRAY: u8 = 0x06;
const TAG_OBJECT: u8 = 0x07;
const TAG_SECRET: u8 = 0x08;

// ─── Varint Encoding (LEB128 unsigned) ───────────────────────────────────────

fn encode_varint(out: &mut Vec<u8>, mut val: u64) {
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn decode_varint(data: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() {
            return Err("unexpected end of data in varint".into());
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift >= 64 {
            return Err("varint overflow".into());
        }
    }
}

// Zigzag encode i64 → u64
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

// Zigzag decode u64 → i64
fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

// ─── String Table ────────────────────────────────────────────────────────────

struct StringTable {
    strings: Vec<String>,
    index: HashMap<String, u32>,
    /// How many times each string is referenced, by first-seen index.
    uses: Vec<u32>,
}

impl StringTable {
    fn new() -> Self {
        Self { strings: Vec::new(), index: HashMap::new(), uses: Vec::new() }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.index.get(s) {
            self.uses[idx as usize] += 1;
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), idx);
        self.uses.push(1);
        idx
    }

    /// Renumber the table by descending use count (ties keep first-seen
    /// order), so the most referenced strings get one-byte varints and the
    /// tree's index stream is far more regular. The format is unchanged — a
    /// decoder reads indices against whatever table it is given.
    ///
    /// perf: on a 4 MB document the compiled file was 60 % larger with the
    /// table in document order than with this order.
    fn order_by_use(&mut self) {
        let mut order: Vec<u32> = (0..self.strings.len() as u32).collect();
        order.sort_by(|&a, &b| self.uses[b as usize].cmp(&self.uses[a as usize]).then(a.cmp(&b)));
        let mut strings = Vec::with_capacity(self.strings.len());
        for (new_idx, &old) in order.iter().enumerate() {
            let s = std::mem::take(&mut self.strings[old as usize]);
            *self.index.get_mut(s.as_str()).expect("interned") = new_idx as u32;
            strings.push(s);
        }
        self.strings = strings;
    }

    fn collect_value(&mut self, val: &Value) {
        match val {
            Value::String(s) | Value::Secret(s) => { self.intern(s); }
            Value::Array(arr) => {
                for item in arr { self.collect_value(item); }
            }
            Value::Object(map) => {
                for (key, val) in map {
                    self.intern(key);
                    self.collect_value(val);
                }
            }
            _ => {}
        }
    }

    fn collect_metadata(&mut self, metadata: &Map<MetaMap>) {
        for (path, meta_map) in metadata {
            self.intern(path);
            for (key, meta) in meta_map {
                self.intern(key);
                for m in &meta.markers { self.intern(m); }
                for a in &meta.args { self.intern(a); }
                if let Some(ref th) = meta.type_hint { self.intern(th); }
                if let Some(ref c) = meta.constraints {
                    if let Some(ref tn) = c.type_name { self.intern(tn); }
                    if let Some(ref pat) = c.pattern { self.intern(pat); }
                    if let Some(ref ev) = c.enum_values {
                        for v in ev { self.intern(v); }
                    }
                }
            }
        }
    }

    fn collect_includes(&mut self, includes: &[IncludeDirective]) {
        for inc in includes {
            self.intern(&inc.path);
            self.intern(&inc.alias);
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        encode_varint(out, self.strings.len() as u64);
        for s in &self.strings {
            encode_varint(out, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
    }
}

/// perf: the strings stay in the payload; a table of owned copies doubled
/// every text of the file before the first value was read.
struct StringTableReader<'a> {
    strings: Vec<&'a str>,
}

impl<'a> StringTableReader<'a> {
    fn decode(data: &'a [u8], pos: &mut usize, budget: &mut DecodeBudget) -> Result<Self, String> {
        let count = decode_len(data, pos)?;
        // safety: 64 MiB of zero bytes is sixty-four million empty strings.
        let room = budget.reserve(count, data.len().saturating_sub(*pos), 1, TABLE_ENTRY_COST)?;
        let mut strings = Vec::with_capacity(room);
        for _ in 0..count {
            let len = decode_len(data, pos)?;
            // safety: `*pos + len` wrapped for a length near `usize::MAX`, the
            // check passed and the slice below panicked — on a 17-byte file.
            if len > data.len().saturating_sub(*pos) {
                return Err("unexpected end of data in string table".into());
            }
            let end = *pos + len;
            let s = std::str::from_utf8(&data[*pos..end])
                .map_err(|e| format!("invalid UTF-8 in string table: {}", e))?;
            *pos = end;
            strings.push(s);
        }
        Ok(Self { strings })
    }

    fn get(&self, idx: u32) -> Result<&'a str, String> {
        self.strings.get(idx as usize)
            .copied()
            .ok_or_else(|| format!("string index {} out of bounds (size {})", idx, self.strings.len()))
    }
}

// ─── Value encoding (with string table) ─────────────────────────────────────

/// `in_order` writes an object's entries as the document has them instead of
/// sorted by key.
///
/// spec: a file that still carries its markers is resolved after it is read
/// back, and markers resolve in document order. Sorted, `total:calc price +
/// tax` could come back ahead of `tax:calc …` and fail. A decoder reads the
/// entries in whatever order they arrive, so the layout is unchanged — and a
/// static or resolved file, where order decides nothing, is written exactly
/// as before.
fn encode_value(out: &mut Vec<u8>, val: &Value, st: &StringTable, in_order: bool) {
    match val {
        Value::Null => out.push(TAG_NULL),
        Value::Bool(false) => out.push(TAG_FALSE),
        Value::Bool(true) => out.push(TAG_TRUE),
        Value::Int(n) => {
            out.push(TAG_INT);
            encode_varint(out, zigzag_encode(*n));
        }
        Value::Float(f) => {
            out.push(TAG_FLOAT);
            out.extend_from_slice(&f.to_le_bytes());
        }
        Value::String(s) => {
            out.push(TAG_STRING);
            encode_varint(out, st.index[s] as u64);
        }
        Value::Array(arr) => {
            out.push(TAG_ARRAY);
            encode_varint(out, arr.len() as u64);
            for item in arr {
                encode_value(out, item, st, in_order);
            }
        }
        Value::Object(map) => {
            out.push(TAG_OBJECT);
            encode_varint(out, map.len() as u64);
            if in_order {
                for (key, val) in map {
                    encode_varint(out, st.index[key.as_str()] as u64);
                    encode_value(out, val, st, in_order);
                }
            } else {
                let mut entries: Vec<(&str, &Value)> =
                    map.iter().map(|(k, v)| (k.as_str(), v)).collect();
                entries.sort_unstable_by_key(|(k, _)| *k);
                for (key, val) in entries {
                    encode_varint(out, st.index[key] as u64);
                    encode_value(out, val, st, in_order);
                }
            }
        }
        Value::Secret(s) => {
            out.push(TAG_SECRET);
            encode_varint(out, st.index[s] as u64);
        }
    }
}

fn decode_value(data: &[u8], pos: &mut usize, st: &StringTableReader, budget: &mut DecodeBudget) -> Result<Value, String> {
    decode_value_depth(data, pos, st, budget, 0)
}

fn decode_value_depth(
    data: &[u8],
    pos: &mut usize,
    st: &StringTableReader,
    budget: &mut DecodeBudget,
    depth: usize,
) -> Result<Value, String> {
    if depth > MAX_DECODE_DEPTH {
        return Err(format!("nesting exceeds {} — refusing to decode", MAX_DECODE_DEPTH));
    }
    if *pos >= data.len() {
        return Err("unexpected end of data".into());
    }
    let tag = data[*pos];
    *pos += 1;
    match tag {
        TAG_NULL => Ok(Value::Null),
        TAG_FALSE => Ok(Value::Bool(false)),
        TAG_TRUE => Ok(Value::Bool(true)),
        TAG_INT => {
            let raw = decode_varint(data, pos)?;
            Ok(Value::Int(zigzag_decode(raw)))
        }
        TAG_FLOAT => {
            if *pos + 8 > data.len() {
                return Err("unexpected end of data in float".into());
            }
            let bytes: [u8; 8] = data[*pos..*pos + 8]
                .try_into()
                .map_err(|_| "float decode error")?;
            *pos += 8;
            Ok(Value::Float(f64::from_le_bytes(bytes)))
        }
        TAG_STRING => {
            let idx = decode_index(data, pos)?;
            Ok(Value::String(budget.text(st.get(idx)?)?))
        }
        TAG_ARRAY => {
            let count = decode_len(data, pos)?;
            let room = budget.reserve(count, data.len().saturating_sub(*pos), 1, ARRAY_SLOT_COST)?;
            let mut arr = Vec::with_capacity(room);
            for _ in 0..count {
                arr.push(decode_value_depth(data, pos, st, budget, depth + 1)?);
            }
            Ok(Value::Array(arr))
        }
        TAG_OBJECT => {
            let count = decode_len(data, pos)?;
            let room = budget.reserve(count, data.len().saturating_sub(*pos), 2, OBJECT_ENTRY_COST)?;
            let mut map = Map::with_capacity_and_hasher(room, Default::default());
            for _ in 0..count {
                let key_idx = decode_index(data, pos)?;
                let key = budget.text(st.get(key_idx)?)?;
                let val = decode_value_depth(data, pos, st, budget, depth + 1)?;
                map.insert(key, val);
            }
            Ok(Value::Object(map))
        }
        TAG_SECRET => {
            let idx = decode_index(data, pos)?;
            Ok(Value::Secret(budget.text(st.get(idx)?)?))
        }
        _ => Err(format!("unknown type tag: 0x{:02x}", tag)),
    }
}

// ─── Metadata encoding ──────────────────────────────────────────────────────

fn encode_constraints(out: &mut Vec<u8>, c: &Constraints, st: &StringTable) {
    let mut bits: u8 = 0;
    if c.min.is_some() { bits |= 0x01; }
    if c.max.is_some() { bits |= 0x02; }
    if c.type_name.is_some() { bits |= 0x04; }
    if c.required { bits |= 0x08; }
    if c.readonly { bits |= 0x10; }
    if c.pattern.is_some() { bits |= 0x20; }
    if c.enum_values.is_some() { bits |= 0x40; }
    out.push(bits);

    if let Some(min) = c.min { out.extend_from_slice(&min.to_le_bytes()); }
    if let Some(max) = c.max { out.extend_from_slice(&max.to_le_bytes()); }
    if let Some(ref tn) = c.type_name { encode_varint(out, st.index[tn] as u64); }
    if let Some(ref pat) = c.pattern { encode_varint(out, st.index[pat] as u64); }
    if let Some(ref ev) = c.enum_values {
        encode_varint(out, ev.len() as u64);
        for v in ev { encode_varint(out, st.index[v] as u64); }
    }
}

fn decode_constraints(data: &[u8], pos: &mut usize, st: &StringTableReader, budget: &mut DecodeBudget) -> Result<Constraints, String> {
    if *pos >= data.len() {
        return Err("unexpected end of data in constraints".into());
    }
    let bits = data[*pos];
    *pos += 1;
    let mut c = Constraints::default();

    if bits & 0x01 != 0 {
        if *pos + 8 > data.len() { return Err("truncated min".into()); }
        let bytes: [u8; 8] = data[*pos..*pos + 8].try_into().map_err(|_| "min decode")?;
        c.min = Some(f64::from_le_bytes(bytes));
        *pos += 8;
    }
    if bits & 0x02 != 0 {
        if *pos + 8 > data.len() { return Err("truncated max".into()); }
        let bytes: [u8; 8] = data[*pos..*pos + 8].try_into().map_err(|_| "max decode")?;
        c.max = Some(f64::from_le_bytes(bytes));
        *pos += 8;
    }
    if bits & 0x04 != 0 {
        let idx = decode_index(data, pos)?;
        c.type_name = Some(budget.text(st.get(idx)?)?);
    }
    if bits & 0x08 != 0 { c.required = true; }
    if bits & 0x10 != 0 { c.readonly = true; }
    if bits & 0x20 != 0 {
        let idx = decode_index(data, pos)?;
        c.pattern = Some(budget.text(st.get(idx)?)?);
    }
    if bits & 0x40 != 0 {
        let count = decode_len(data, pos)?;
        c.enum_values = Some(budget.texts(data, pos, st, count)?);
    }
    Ok(c)
}

fn encode_meta(out: &mut Vec<u8>, meta: &Meta, st: &StringTable) {
    encode_varint(out, meta.markers.len() as u64);
    for m in &meta.markers { encode_varint(out, st.index[m] as u64); }
    encode_varint(out, meta.args.len() as u64);
    for a in &meta.args { encode_varint(out, st.index[a] as u64); }
    if let Some(ref th) = meta.type_hint {
        out.push(1);
        encode_varint(out, st.index[th] as u64);
    } else {
        out.push(0);
    }
    if let Some(ref c) = meta.constraints {
        out.push(1);
        encode_constraints(out, c, st);
    } else {
        out.push(0);
    }
}

fn decode_meta(data: &[u8], pos: &mut usize, st: &StringTableReader, budget: &mut DecodeBudget) -> Result<Meta, String> {
    let marker_count = decode_len(data, pos)?;
    let markers = budget.texts(data, pos, st, marker_count)?;

    let arg_count = decode_len(data, pos)?;
    let args = budget.texts(data, pos, st, arg_count)?;

    if *pos >= data.len() { return Err("unexpected end in meta".into()); }
    let has_th = data[*pos];
    *pos += 1;
    let type_hint = if has_th != 0 {
        let idx = decode_index(data, pos)?;
        Some(budget.text(st.get(idx)?)?)
    } else { None };

    if *pos >= data.len() { return Err("unexpected end in meta".into()); }
    let has_c = data[*pos];
    *pos += 1;
    let constraints = if has_c != 0 { Some(decode_constraints(data, pos, st, budget)?) } else { None };

    Ok(Meta { markers, args, type_hint, constraints })
}

/// Declarations are written in document order, like the objects they belong
/// to (see [`encode_value`]): which of two type declarations for one field
/// name wins, and the order `:inherit` runs in, follow it.
fn encode_metadata(out: &mut Vec<u8>, metadata: &Map<MetaMap>, st: &StringTable) {
    encode_varint(out, metadata.len() as u64);
    for (key_path, meta_map) in metadata {
        encode_varint(out, st.index[key_path.as_str()] as u64);
        encode_varint(out, meta_map.len() as u64);
        for (field_key, meta) in meta_map {
            encode_varint(out, st.index[field_key.as_str()] as u64);
            encode_meta(out, meta, st);
        }
    }
}

fn decode_metadata(data: &[u8], pos: &mut usize, st: &StringTableReader, budget: &mut DecodeBudget) -> Result<Map<MetaMap>, String> {
    let outer_count = decode_len(data, pos)?;
    let room = budget.reserve(outer_count, data.len().saturating_sub(*pos), 2, META_PATH_COST)?;
    let mut metadata = Map::with_capacity_and_hasher(room, Default::default());
    for _ in 0..outer_count {
        let path_idx = decode_index(data, pos)?;
        let key_path = budget.text(st.get(path_idx)?)?;
        let inner_count = decode_len(data, pos)?;
        // An entry is a key and four bytes of declaration at the least.
        let room = budget.reserve(inner_count, data.len().saturating_sub(*pos), 5, META_ENTRY_COST)?;
        let mut meta_map = Map::with_capacity_and_hasher(room, Default::default());
        for _ in 0..inner_count {
            let key_idx = decode_index(data, pos)?;
            let field_key = budget.text(st.get(key_idx)?)?;
            let meta = decode_meta(data, pos, st, budget)?;
            meta_map.insert(field_key, meta);
        }
        metadata.insert(key_path, meta_map);
    }
    Ok(metadata)
}

fn encode_includes(out: &mut Vec<u8>, includes: &[IncludeDirective], st: &StringTable) {
    encode_varint(out, includes.len() as u64);
    for inc in includes {
        encode_varint(out, st.index[&inc.path] as u64);
        encode_varint(out, st.index[&inc.alias] as u64);
    }
}

fn decode_includes(data: &[u8], pos: &mut usize, st: &StringTableReader, budget: &mut DecodeBudget) -> Result<Vec<IncludeDirective>, String> {
    let count = decode_len(data, pos)?;
    let room = budget.reserve(count, data.len().saturating_sub(*pos), 2, INCLUDE_COST)?;
    let mut includes = Vec::with_capacity(room);
    for _ in 0..count {
        let path_idx = decode_index(data, pos)?;
        let alias_idx = decode_index(data, pos)?;
        includes.push(IncludeDirective {
            path: budget.text(st.get(path_idx)?)?,
            alias: budget.text(st.get(alias_idx)?)?,
        });
    }
    Ok(includes)
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Compile a `ParseResult` into compact binary `.synxb` format.
///
/// Uses a string interning table so every unique string is stored once,
/// then deflate-compresses the payload for maximum compactness.
/// If `resolved` is true, metadata and includes are stripped.
pub fn compile(result: &ParseResult, resolved: bool) -> Vec<u8> {
    let mut st = StringTable::new();
    st.collect_value(&result.root);
    let has_meta = !resolved && !result.metadata.is_empty();
    if has_meta {
        st.collect_metadata(&result.metadata);
        st.collect_includes(&result.includes);
    }
    st.order_by_use();

    // Build uncompressed payload
    let mut payload = Vec::with_capacity(1024);
    st.encode(&mut payload);
    encode_value(&mut payload, &result.root, &st, has_meta);
    if has_meta {
        encode_metadata(&mut payload, &result.metadata, &st);
        encode_includes(&mut payload, &result.includes, &st);
    }

    // Compress payload.
    //
    // This is a storage format: favor size over speed. Using level 9 also makes
    // size-reduction expectations stable across platforms/runner images.
    let compressed = miniz_oxide::deflate::compress_to_vec(&payload, 9);

    // Build final output: header + compressed data
    let mut out = Vec::with_capacity(7 + 4 + compressed.len());

    // Header (always uncompressed for magic detection)
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);

    let mut flags: u8 = 0;
    if result.mode == Mode::Active { flags |= FLAG_ACTIVE; }
    if result.locked { flags |= FLAG_LOCKED; }
    if has_meta { flags |= FLAG_HAS_META; }
    if resolved { flags |= FLAG_RESOLVED; }
    if result.tool { flags |= FLAG_TOOL; }
    if result.schema { flags |= FLAG_SCHEMA; }
    if result.llm { flags |= FLAG_LLM; }
    out.push(flags);

    // Uncompressed size (for pre-allocation on decode)
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Compressed payload
    out.extend_from_slice(&compressed);

    out
}

/// Decompile a `.synxb` binary back into a `ParseResult`.
pub fn decompile(data: &[u8]) -> Result<ParseResult, String> {
    if data.len() < 11 {
        return Err("file too small for .synxb header".into());
    }
    if &data[0..5] != MAGIC {
        return Err("invalid .synxb magic (expected SYNXB)".into());
    }
    let version = data[5];
    if version != FORMAT_VERSION {
        return Err(format!("unsupported .synxb version: {} (expected {})", version, FORMAT_VERSION));
    }
    let flags = data[6];

    // Read uncompressed size
    let uncomp_size = u32::from_le_bytes(
        data[7..11].try_into().map_err(|_| "failed to read size")?
    ) as usize;

    // safety: reject an oversized declared size before allocating, then bound
    // the actual inflate so a small crafted stream cannot expand into gigabytes
    // (a decompression bomb) — the length check alone runs only *after* the
    // buffer has already grown.
    if uncomp_size > MAX_DECODED_BYTES {
        return Err(format!("declared size {} exceeds limit {}", uncomp_size, MAX_DECODED_BYTES));
    }
    // safety: the stream may produce the declared size and not a byte more.
    // Bounded only by the 256 MiB cap, a 260 KB file declaring 1 KiB still
    // inflated to the full cap before the size check rejected it.
    let payload = miniz_oxide::inflate::decompress_to_vec_with_limit(&data[11..], uncomp_size + 1)
        .map_err(|e| format!("decompression failed: {:?}", e.status))?;
    if payload.len() != uncomp_size {
        return Err(format!("size mismatch: expected {}, got {}", uncomp_size, payload.len()));
    }

    let mut pos = 0;
    let mut budget = DecodeBudget::default();

    // String table
    let st = StringTableReader::decode(&payload, &mut pos, &mut budget)?;

    // Root value
    let root = decode_value(&payload, &mut pos, &st, &mut budget)?;

    let mode = if flags & FLAG_ACTIVE != 0 { Mode::Active } else { Mode::Static };
    let locked = flags & FLAG_LOCKED != 0;
    let tool = flags & FLAG_TOOL != 0;
    let schema = flags & FLAG_SCHEMA != 0;
    let llm = flags & FLAG_LLM != 0;

    let (metadata, includes) = if flags & FLAG_HAS_META != 0 {
        let meta = decode_metadata(&payload, &mut pos, &st, &mut budget)?;
        let inc = decode_includes(&payload, &mut pos, &st, &mut budget)?;
        (meta, inc)
    } else {
        (Map::default(), Vec::new())
    };

    Ok(ParseResult {
        root,
        mode,
        locked,
        tool,
        schema,
        llm,
        metadata,
        includes,
        uses: Vec::new(),
        // A `.synxb` container carries an already-parsed tree; nothing is
        // dropped on the way back out.
        truncated: false,
    })
}

/// Whether a `.synxb` file holds an already resolved document (header flag).
pub(crate) fn is_resolved(data: &[u8]) -> bool {
    data.len() > 6 && data[6] & FLAG_RESOLVED != 0
}

/// Check if data starts with the `.synxb` magic bytes.
pub fn is_synxb(data: &[u8]) -> bool {
    data.len() >= 5 && &data[0..5] == MAGIC
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_roundtrip() {
        for &val in &[0u64, 1, 127, 128, 300, 16383, 16384, u64::MAX >> 1] {
            let mut buf = Vec::new();
            encode_varint(&mut buf, val);
            let mut pos = 0;
            let decoded = decode_varint(&buf, &mut pos).unwrap();
            assert_eq!(val, decoded, "varint roundtrip failed for {}", val);
        }
    }

    #[test]
    fn test_zigzag_roundtrip() {
        for &val in &[0i64, 1, -1, 42, -42, i64::MAX, i64::MIN] {
            let encoded = zigzag_encode(val);
            let decoded = zigzag_decode(encoded);
            assert_eq!(val, decoded, "zigzag roundtrip failed for {}", val);
        }
    }

    #[test]
    fn test_compile_decompile_static() {
        let mut root = Map::default();
        root.insert("name".to_string(), Value::String("Test".into()));
        root.insert("port".to_string(), Value::Int(8080));

        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Static,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata: Map::default(),
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, false);
        assert!(is_synxb(&binary));

        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, result.root);
        assert_eq!(restored.mode, Mode::Static);
        assert!(!restored.locked);
    }

    #[test]
    fn test_compile_decompile_active_with_metadata() {
        let mut root = Map::default();
        root.insert("host".to_string(), Value::String("0.0.0.0".into()));
        root.insert("port".to_string(), Value::Int(3000));

        let mut meta_map = Map::default();
        meta_map.insert("port".to_string(), Meta {
            markers: vec!["env".to_string()],
            args: vec!["default".to_string(), "3000".to_string()],
            type_hint: Some("int".to_string()),
            constraints: Some(Constraints {
                min: Some(1.0),
                max: Some(65535.0),
                required: true,
                ..Default::default()
            }),
        });

        let mut metadata = Map::default();
        metadata.insert(String::new(), meta_map);

        let includes = vec![IncludeDirective {
            path: "./base.synx".to_string(),
            alias: "base".to_string(),
        }];

        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Active,
            locked: true,
            tool: false,
            schema: false,
            llm: false,
            metadata,
        includes,
        uses: Vec::new(),
        truncated: false,
    };

        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();

        assert_eq!(restored.root, result.root);
        assert_eq!(restored.mode, Mode::Active);
        assert!(restored.locked);
        assert_eq!(restored.metadata.len(), 1);
        let rm = &restored.metadata[""];
        assert_eq!(rm["port"].markers, vec!["env"]);
        assert_eq!(rm["port"].args, vec!["default", "3000"]);
        assert_eq!(rm["port"].type_hint, Some("int".to_string()));
        let c = rm["port"].constraints.as_ref().unwrap();
        assert_eq!(c.min, Some(1.0));
        assert_eq!(c.max, Some(65535.0));
        assert!(c.required);
        assert_eq!(restored.includes.len(), 1);
        assert_eq!(restored.includes[0].path, "./base.synx");
    }

    #[test]
    fn test_compile_resolved_strips_metadata() {
        let mut root = Map::default();
        root.insert("val".to_string(), Value::Int(42));

        let mut meta_map = Map::default();
        meta_map.insert("val".to_string(), Meta {
            markers: vec!["calc".to_string()],
            args: Vec::new(),
            type_hint: None,
            constraints: None,
        });
        let mut metadata = Map::default();
        metadata.insert(String::new(), meta_map);

        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Active,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata,
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, true);
        let restored = decompile(&binary).unwrap();

        assert_eq!(restored.root, result.root);
        assert!(restored.metadata.is_empty());
        assert!(restored.includes.is_empty());
    }

    #[test]
    fn test_is_synxb() {
        assert!(is_synxb(b"SYNXB\x01\x00"));
        assert!(!is_synxb(b"JSON{"));
        assert!(!is_synxb(b"SYN"));
    }

    #[test]
    fn test_invalid_magic() {
        let err = decompile(b"WRONG\x01\x00\x00\x00\x00\x00").unwrap_err();
        assert!(err.contains("invalid .synxb magic"));
    }

    #[test]
    fn test_invalid_version() {
        let err = decompile(b"SYNXB\xFF\x00\x00\x00\x00\x00").unwrap_err();
        assert!(err.contains("unsupported .synxb version"));
    }

    #[test]
    fn test_nested_object_roundtrip() {
        let mut inner = Map::default();
        inner.insert("host".to_string(), Value::String("localhost".into()));
        inner.insert("port".to_string(), Value::Int(5432));

        let mut root = Map::default();
        root.insert("name".to_string(), Value::String("app".into()));
        root.insert("database".to_string(), Value::Object(inner));
        root.insert("tags".to_string(), Value::Array(vec![
            Value::String("prod".into()),
            Value::String("v2".into()),
        ]));

        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Static,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata: Map::default(),
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, result.root);
    }

    #[test]
    fn test_full_roundtrip_parse_compile_decompile() {
        let synx_text = "name TotalWario\nversion 3.0.0\nport 8080\ndebug false\n";
        let parsed = crate::parse(synx_text);
        let binary = compile(&parsed, false);

        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, parsed.root);
        assert_eq!(restored.mode, parsed.mode);
    }

    #[test]
    fn test_large_config_size_reduction() {
        let synx_text = include_str!("../../../benchmarks/config.synx");
        let parsed = crate::parse(synx_text);
        let binary = compile(&parsed, false);
        let ratio = binary.len() as f64 / synx_text.len() as f64;
        // Compression ratios can vary slightly across platforms/toolchains.
        // Keep this test as a regression guard (binary should be *meaningfully* smaller),
        // without making CI brittle.
        assert!(
            ratio < 0.65,
            "binary should be at least 35% smaller: {} bytes vs {} bytes (ratio {:.2})",
            binary.len(), synx_text.len(), ratio
        );
    }

    #[test]
    fn test_large_config_full_roundtrip() {
        let synx_text = include_str!("../../../benchmarks/config.synx");
        let parsed = crate::parse(synx_text);
        let binary = compile(&parsed, false);
        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, parsed.root);
        assert_eq!(restored.mode, parsed.mode);
    }

    #[test]
    fn test_constraints_full_roundtrip() {
        let c_orig = Constraints {
            min: Some(0.0),
            max: Some(100.0),
            type_name: Some("int".to_string()),
            required: true,
            readonly: true,
            pattern: Some(r"^\d+$".to_string()),
            enum_values: Some(vec!["a".into(), "b".into(), "c".into()]),
        };

        let mut meta_map = Map::default();
        meta_map.insert("field".to_string(), Meta {
            markers: Vec::new(),
            args: Vec::new(),
            type_hint: None,
            constraints: Some(c_orig.clone()),
        });
        let mut metadata = Map::default();
        metadata.insert(String::new(), meta_map);

        let mut root = Map::default();
        root.insert("field".to_string(), Value::Int(42));

        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Active,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata,
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();
        let rm = &restored.metadata[""];
        let c = rm["field"].constraints.as_ref().unwrap();
        assert_eq!(c.min, c_orig.min);
        assert_eq!(c.max, c_orig.max);
        assert_eq!(c.type_name, c_orig.type_name);
        assert_eq!(c.required, c_orig.required);
        assert_eq!(c.readonly, c_orig.readonly);
        assert_eq!(c.pattern, c_orig.pattern);
        assert_eq!(c.enum_values, c_orig.enum_values);
    }

    #[test]
    fn test_all_value_types() {
        let mut map = Map::default();
        map.insert("null_val".to_string(), Value::Null);
        map.insert("bool_t".to_string(), Value::Bool(true));
        map.insert("bool_f".to_string(), Value::Bool(false));
        map.insert("int_pos".to_string(), Value::Int(42));
        map.insert("int_neg".to_string(), Value::Int(-100));
        map.insert("int_zero".to_string(), Value::Int(0));
        map.insert("float_val".to_string(), Value::Float(3.14));
        map.insert("string_val".to_string(), Value::String("hello world".into()));
        map.insert("secret_val".to_string(), Value::Secret("s3cr3t".into()));
        map.insert("array_val".to_string(), Value::Array(vec![
            Value::Int(1), Value::String("two".into()), Value::Null,
        ]));

        let result = ParseResult {
            root: Value::Object(map),
            mode: Mode::Static,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata: Map::default(),
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, result.root);
    }

    #[test]
    fn test_empty_object() {
        let result = ParseResult {
            root: Value::Object(Map::default()),
            mode: Mode::Static,
            locked: false,
            tool: false,
            schema: false,
            llm: false,
            metadata: Map::default(),
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };

        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();
        assert_eq!(restored.root, result.root);
    }

    #[test]
    fn test_llm_flag_roundtrip() {
        let mut root = Map::default();
        root.insert("task".to_string(), Value::String("ping".into()));
        let result = ParseResult {
            root: Value::Object(root),
            mode: Mode::Static,
            locked: false,
            tool: false,
            schema: false,
            llm: true,
            metadata: Map::default(),
            includes: Vec::new(),
            uses: Vec::new(),
            truncated: false,
        };
        let binary = compile(&result, false);
        let restored = decompile(&binary).unwrap();
        assert!(restored.llm);
        assert_eq!(restored.root, result.root);
    }

    #[test]
    fn test_synx_api_compile_decompile() {
        use crate::Synx;

        let text = "name Wario\nport 8080\ndebug false\n";
        let binary = Synx::compile(text, false);
        assert!(Synx::is_synxb(&binary));

        let decompiled = Synx::decompile(&binary).unwrap();
        let original = crate::parse(text);
        let reparsed = crate::parse(&decompiled);
        assert_eq!(original.root, reparsed.root);
    }

    // ── Hardening against hostile `.synxb` input (decompile is a decoder for
    //    untrusted bytes; none of these may panic, OOM, or overflow the stack) ──

    /// Build a minimal valid `.synxb` header wrapping a raw (already-deflated) body.
    fn wrap_synxb(uncomp_size: u32, flags: u8, deflated: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.push(flags);
        out.extend_from_slice(&uncomp_size.to_le_bytes());
        out.extend_from_slice(deflated);
        out
    }

    #[test]
    fn test_decompile_rejects_oversized_declared_size() {
        // Declared uncompressed size beyond the cap must be refused before any
        // large allocation, regardless of the (here trivial) body.
        let bomb = wrap_synxb(u32::MAX, 0, &[]);
        assert!(decompile(&bomb).is_err());
    }

    #[test]
    fn test_decompile_bounds_decompression() {
        // A tiny deflate stream that expands past the cap must error, not
        // materialise gigabytes. 4 MiB of zeros compresses to a few KiB.
        let payload = vec![0u8; 4 * 1024 * 1024];
        let deflated = miniz_oxide::deflate::compress_to_vec(&payload, 9);
        // Under-declare the size; the with_limit inflate still refuses to grow
        // past MAX_DECODED_BYTES if a crafted stream tried to.
        let framed = wrap_synxb(payload.len() as u32, 0, &deflated);
        // Valid here (4 MiB < cap) — it must decode without panicking…
        let _ = decompile(&framed);
        // …and a stream whose declared size exceeds the cap is rejected.
        let lie = wrap_synxb((MAX_DECODED_BYTES + 1) as u32, 0, &deflated);
        assert!(decompile(&lie).is_err());
    }

    #[test]
    fn test_decompile_huge_count_does_not_abort() {
        // A string-table count of u64::MAX must produce a clean Err, not abort
        // the process inside `with_capacity`.
        let mut payload = Vec::new();
        encode_varint(&mut payload, u64::MAX); // string table count
        let deflated = miniz_oxide::deflate::compress_to_vec(&payload, 9);
        let framed = wrap_synxb(payload.len() as u32, 0, &deflated);
        assert!(decompile(&framed).is_err());
    }

    #[test]
    fn test_decompile_deep_nesting_does_not_overflow_stack() {
        // Value tree of nothing but ARRAY tags, far past the depth cap.
        let mut payload = Vec::new();
        encode_varint(&mut payload, 0); // empty string table
        for _ in 0..(MAX_DECODE_DEPTH + 500) {
            payload.push(TAG_ARRAY);
            encode_varint(&mut payload, 1); // one child
        }
        payload.push(TAG_NULL);
        let deflated = miniz_oxide::deflate::compress_to_vec(&payload, 9);
        let framed = wrap_synxb(payload.len() as u32, 0, &deflated);
        assert!(decompile(&framed).is_err());
    }

    #[test]
    fn test_decompile_parser_depth_round_trips() {
        // The deepest tree `parse` can produce must still decode.
        fn put(depth: usize, out: &mut Vec<u8>) {
            if depth == 0 {
                out.push(TAG_NULL);
                return;
            }
            out.push(TAG_ARRAY);
            encode_varint(out, 1);
            put(depth - 1, out);
        }
        let mut payload = Vec::new();
        encode_varint(&mut payload, 0); // empty string table
        put(128, &mut payload);
        let deflated = miniz_oxide::deflate::compress_to_vec(&payload, 9);
        let framed = wrap_synxb(payload.len() as u32, 0, &deflated);
        assert!(decompile(&framed).is_ok());
    }

    #[test]
    fn test_decompile_string_index_out_of_bounds_errors() {
        // Empty string table, then a STRING value referencing index 5.
        let mut payload = Vec::new();
        encode_varint(&mut payload, 0);
        payload.push(TAG_STRING);
        encode_varint(&mut payload, 5);
        let deflated = miniz_oxide::deflate::compress_to_vec(&payload, 9);
        let framed = wrap_synxb(payload.len() as u32, 0, &deflated);
        assert!(decompile(&framed).is_err());
    }

    #[test]
    fn test_binary_roundtrip_large_int() {
        // Values beyond 2^53 exercise the full i64 zigzag path.
        let mut root = Map::default();
        root.insert("big".to_string(), Value::Int(9_000_000_000_000_000_007));
        root.insert("neg".to_string(), Value::Int(-9_000_000_000_000_000_007));
        let result = ParseResult {
            root: Value::Object(root), mode: Mode::Static, locked: false, tool: false,
            schema: false, llm: false, metadata: Map::default(), includes: Vec::new(),
            uses: Vec::new(), truncated: false,
        };
        let restored = decompile(&compile(&result, false)).unwrap();
        assert_eq!(restored.root, result.root);
    }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;
    use crate::{parse, resolve, Options, Synx};

    fn frame(payload: &[u8], declared: u32, flags: u8) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(FORMAT_VERSION);
        out.push(flags);
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(payload, 9));
        out
    }

    // S6-03: seventeen bytes panicked the decoder — `*pos + len` wrapped.
    #[test]
    fn a_string_length_near_the_end_of_usize_is_an_error() {
        let mut p = Vec::new();
        encode_varint(&mut p, 1);
        encode_varint(&mut p, u64::MAX);
        p.push(1);
        assert_eq!(decompile(&frame(&p, p.len() as u32, 0)).unwrap_err(), "unexpected end of data in string table");
        let mut p = Vec::new();
        encode_varint(&mut p, 1);
        encode_varint(&mut p, u64::MAX - 3);
        assert!(decompile(&frame(&p, p.len() as u32, 0)).is_err());
    }

    // B-020: an index of 2^32 used to read as index 0.
    #[test]
    fn a_string_index_past_u32_is_an_error() {
        let mut p = Vec::new();
        encode_varint(&mut p, 1);
        encode_varint(&mut p, 4);
        p.extend_from_slice(b"zero");
        p.push(TAG_STRING);
        encode_varint(&mut p, 1 << 32);
        assert_eq!(decompile(&frame(&p, p.len() as u32, 0)).unwrap_err(), "string index out of range");
    }

    // A count from the file is not an allocation request, and the tree has a budget.
    #[test]
    fn counts_and_copies_are_budgeted() {
        // An array that claims 2^62 items and holds three is a truncated file.
        // (What decoding really allocates is measured in `tests/hostile.rs`.)
        let mut p = Vec::new();
        encode_varint(&mut p, 0);
        p.push(TAG_ARRAY);
        encode_varint(&mut p, u64::MAX >> 2);
        p.extend_from_slice(&[TAG_NULL, TAG_TRUE, TAG_FALSE]);
        assert_eq!(decompile(&frame(&p, p.len() as u32, 0)).unwrap_err(), "unexpected end of data");

        // Storage is charged before it is reserved.
        let mut budget = DecodeBudget::default();
        assert_eq!(budget.reserve(1000, 4096, 1, ARRAY_SLOT_COST).unwrap(), 1000);
        assert_eq!(budget.spent, 1000 * ARRAY_SLOT_COST);
        assert_eq!(budget.reserve(5000, 4096, 1, 1).unwrap_err(), "unexpected end of data");
        assert_eq!(budget.reserve(3000, 6000, 2, OBJECT_ENTRY_COST).unwrap(), 3000);
        assert_eq!(budget.reserve(3001, 6000, 2, 1).unwrap_err(), "unexpected end of data");
        assert!(budget.reserve(MAX_DECODED_TREE_BYTES, usize::MAX, 1, 1).unwrap_err().contains("refusing to decode"));

        // A list of nulls that is all there, and one item more than a tree may hold.
        let n = MAX_DECODED_TREE_BYTES / ARRAY_SLOT_COST + 1;
        let mut p = Vec::new();
        encode_varint(&mut p, 0);
        p.push(TAG_ARRAY);
        encode_varint(&mut p, n as u64);
        p.resize(p.len() + n, TAG_NULL);
        let err = decompile(&frame(&p, p.len() as u32, 0)).unwrap_err();
        assert!(err.contains("refusing to decode"), "{}", err);

        // References to one string decode, each a copy.
        let mut p = Vec::new();
        encode_varint(&mut p, 1);
        encode_varint(&mut p, 3);
        p.extend_from_slice(b"abc");
        p.push(TAG_ARRAY);
        encode_varint(&mut p, 50_000);
        for _ in 0..50_000 {
            p.push(TAG_STRING);
            p.push(0);
        }
        let r = decompile(&frame(&p, p.len() as u32, 0)).unwrap();
        assert_eq!(r.root.as_array().unwrap().len(), 50_000);
    }

    // The costs are numbers shared with the ports; here they are held to what
    // the pieces really occupy.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn decode_costs_cover_what_the_pieces_occupy() {
        use std::mem::size_of;
        assert!(TABLE_ENTRY_COST >= size_of::<&str>());
        assert!(ARRAY_SLOT_COST >= size_of::<Value>());
        assert!(OBJECT_ENTRY_COST >= size_of::<Value>() + size_of::<String>() + 16);
        assert!(TEXT_ITEM_COST >= size_of::<String>());
        assert!(META_ENTRY_COST >= size_of::<Meta>() + size_of::<String>() + 16);
        assert!(META_PATH_COST >= size_of::<MetaMap>() + size_of::<String>() + 16);
        assert!(INCLUDE_COST >= size_of::<IncludeDirective>());
    }

    // The stream may not produce more than the header declares.
    #[test]
    fn a_stream_larger_than_declared_is_refused() {
        let payload = vec![0u8; 32 * 1024 * 1024];
        let err = decompile(&frame(&payload, 1024, 0)).unwrap_err();
        assert!(err.starts_with("decompression failed"), "{}", err);
        // The message names the failure, not the megabytes inflated so far.
        assert!(err.len() < 200, "{}", err.len());
    }

    // A file that still has its markers is resolved after it is read, in
    // document order: sorted keys put `alpha` ahead of what it reads.
    #[test]
    fn an_unresolved_file_keeps_document_order() {
        let text = "!active\nprice 100\ntax:calc price * 0.2\ntotal:calc price + tax\nalpha:calc total + 1\n";
        let mut back = decompile(&compile(&parse(text), false)).unwrap();
        let keys: Vec<&str> = back.root.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["price", "tax", "total", "alpha"]);
        resolve(&mut back, &Options::default());
        assert_eq!(back.root.as_object().unwrap()["alpha"], Value::Int(121));

        // A static file is written exactly as before: keys sorted.
        let stat = decompile(&compile(&parse("zeta 1\nalpha 2\n"), false)).unwrap();
        let keys: Vec<&str> = stat.root.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["alpha", "zeta"]);
    }

    // T3-04: the text of an unresolved file has its declarations back.
    #[test]
    fn decompile_writes_the_declarations() {
        let text = "!active\n!include ./base.synx cfg\nport:env:default:8080 NOPE_UNSET_ZZ\nsecret_key:secret abc123\nmax[min:1, max:10] 5\nzip(string) 007\ntier:random 90 10\n  - gold\n  - silver\nprod:inherit base\n  debug false\nservers\n  - host:env H\n    port[type:int] 1\nnotes |\n  two\n  lines\n";
        let out = Synx::decompile(&Synx::compile(text, false)).unwrap();
        for line in [
            "!active",
            "!include ./base.synx cfg",
            "port:env:default:8080 NOPE_UNSET_ZZ",
            "secret_key:secret abc123",
            "max[min:1, max:10] 5",
            "zip(string) 007",
            "tier:random 90 10",
            "prod:inherit base",
            "  - host:env H",
            "    port[type:int] 1",
        ] {
            assert!(out.lines().any(|l| l == line), "missing `{}` in:\n{}", line, out);
        }
        // Read back, it is the same document: tree, declarations, order.
        let a = parse(text);
        let b = parse(&out);
        assert_eq!(a.root, b.root);
        assert_eq!(a.metadata, b.metadata);
        assert_eq!(
            a.root.as_object().unwrap().keys().collect::<Vec<_>>(),
            b.root.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        // And resolves to the same values.
        let opts = Options { env: Some(Default::default()), ..Default::default() };
        let (mut ra, mut rb) = (parse(text), parse(&out));
        resolve(&mut ra, &opts);
        resolve(&mut rb, &opts);
        let pick = |r: &ParseResult, k: &str| r.root.as_object().unwrap()[k].clone();
        for k in ["port", "secret_key", "max", "zip", "servers", "notes"] {
            assert_eq!(pick(&ra, k), pick(&rb, k), "{}", k);
        }
        assert_eq!(pick(&rb, "port"), Value::Int(8080));
        assert_eq!(pick(&rb, "secret_key"), Value::Secret("abc123".into()));
    }

    // A resolved file is a static document: its values are final.
    #[test]
    fn decompile_of_a_resolved_file_is_static() {
        let text = "!active\nname Wario\ngreeting Hello, {name} and {braces}\nn:calc 2 + 3\n";
        let out = Synx::decompile(&Synx::compile(text, true)).unwrap();
        assert!(!out.contains("!active"), "{}", out);
        let back = parse(&out);
        assert_eq!(back.mode, Mode::Static);
        assert_eq!(back.root.as_object().unwrap()["greeting"], Value::String("Hello, Wario and {braces}".into()));
        assert_eq!(back.root.as_object().unwrap()["n"], Value::Int(5));
    }
}
