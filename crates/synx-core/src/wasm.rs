//! WASM Marker Runtime — loads and executes custom markers from .wasm modules.
//!
//! ## Guest ABI v1
//!
//! WASM marker modules must export:
//! - `memory`: linear memory
//! - `synx_alloc(size: i32) -> i32`: allocate bytes, return pointer
//! - `synx_markers() -> i64`: return packed (ptr << 32 | len) to JSON array of marker names
//! - `synx_apply(in_ptr: i32, in_len: i32) -> i64`: process marker, return packed (ptr << 32 | len)
//!
//! Input to `synx_apply` (JSON bytes):
//!   `{"marker":"name","value":"some value","args":["arg1","arg2"]}`
//!
//! Output from `synx_apply` (JSON bytes):
//!   `{"value":"result"}` or `{"error":"message"}`

use crate::value::Value;
use std::collections::HashMap;

/// Maximum WASM module file size (2 MiB).
const MAX_WASM_MODULE_SIZE: usize = 2 * 1024 * 1024;
/// Maximum execution fuel (instruction count limit to prevent infinite loops).
const MAX_FUEL: u64 = 10_000_000;
/// Maximum combined input + output size for a single marker call.
const MAX_IO_SIZE: usize = 256 * 1024;

/// Capability flags — what a WASM marker module is allowed to do.
#[derive(Debug, Clone, Default)]
pub struct WasmCapabilities {
    /// Allow reading environment variables (via host import).
    pub env_read: bool,
    /// Allow file system reads (sandboxed to package dir).
    pub fs_read: bool,
    /// Allow network requests (currently always false, reserved for future).
    pub network: bool,
    /// Custom fuel limit (overrides MAX_FUEL if set).
    pub fuel_limit: Option<u64>,
}

impl WasmCapabilities {
    /// Parse capabilities from a manifest `permissions` field.
    /// Format: `permissions env_read, fs_read`
    pub fn from_manifest_line(line: &str) -> Self {
        let mut caps = WasmCapabilities::default();
        for part in line.split(',') {
            match part.trim() {
                "env_read" => caps.env_read = true,
                "fs_read" => caps.fs_read = true,
                "network" => caps.network = true,
                _ => {}
            }
        }
        caps
    }
}

/// A loaded WASM marker module.
pub struct WasmMarkerModule {
    engine: wasmi::Engine,
    module: wasmi::Module,
    /// Marker names this module provides (e.g., ["rate", "cache", "retry"]).
    markers: Vec<String>,
    /// Capability permissions for this module.
    pub capabilities: WasmCapabilities,
}

/// Runtime that manages loaded WASM marker modules.
pub struct WasmMarkerRuntime {
    modules: Vec<WasmMarkerModule>,
    /// marker_name → index into modules
    dispatch: HashMap<String, usize>,
}

impl std::fmt::Debug for WasmMarkerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmMarkerRuntime")
            .field("markers", &self.dispatch.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Unpack a packed i64 (ptr << 32 | len) into (ptr, len).
fn unpack_ptr_len(packed: i64) -> (u32, u32) {
    let ptr = ((packed >> 32) & 0xFFFF_FFFF) as u32;
    let len = (packed & 0xFFFF_FFFF) as u32;
    (ptr, len)
}

impl WasmMarkerModule {
    /// Load a WASM marker module from bytes.
    pub fn from_bytes(wasm_bytes: &[u8], capabilities: WasmCapabilities) -> Result<Self, String> {
        if wasm_bytes.len() > MAX_WASM_MODULE_SIZE {
            return Err(format!(
                "WASM module too large ({} bytes, max {})",
                wasm_bytes.len(),
                MAX_WASM_MODULE_SIZE
            ));
        }

        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        let engine = wasmi::Engine::new(&config);

        let module = wasmi::Module::new(&engine, wasm_bytes)
            .map_err(|e| format!("WASM compile error: {}", e))?;

        // Discover markers by instantiating and calling synx_markers()
        let markers = Self::discover_markers(&engine, &module)?;

        Ok(WasmMarkerModule {
            engine,
            module,
            markers,
            capabilities,
        })
    }

    /// Instantiate the module and call synx_markers() to discover available markers.
    fn discover_markers(engine: &wasmi::Engine, module: &wasmi::Module) -> Result<Vec<String>, String> {
        let mut store = wasmi::Store::new(engine, ());
        store.set_fuel(MAX_FUEL).map_err(|e| format!("fuel error: {}", e))?;

        let linker = wasmi::Linker::<()>::new(engine);
        let instance = linker
            .instantiate_and_start(&mut store, module)
            .map_err(|e| format!("WASM instantiation error: {}", e))?;

        let synx_markers = instance
            .get_typed_func::<(), i64>(&store, "synx_markers")
            .map_err(|e| format!("missing synx_markers export: {}", e))?;

        let packed = synx_markers
            .call(&mut store, ())
            .map_err(|e| format!("synx_markers() call failed: {}", e))?;

        let (ptr, len) = unpack_ptr_len(packed);

        if len == 0 || len as usize > MAX_IO_SIZE {
            return Err("synx_markers() returned invalid length".to_string());
        }

        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| "WASM module missing memory export".to_string())?;

        let data = memory.data(&store);
        let start = ptr as usize;
        let end = start + len as usize;
        if end > data.len() {
            return Err("synx_markers() returned out-of-bounds pointer".to_string());
        }

        let json_bytes = &data[start..end];
        let json_str = std::str::from_utf8(json_bytes)
            .map_err(|_| "synx_markers() returned invalid UTF-8".to_string())?;

        let names: Vec<String> = serde_json::from_str(json_str)
            .map_err(|e| format!("synx_markers() returned invalid JSON: {}", e))?;

        if names.is_empty() {
            return Err("synx_markers() returned empty list".to_string());
        }

        Ok(names)
    }

    /// Apply a marker by calling synx_apply() in the WASM module.
    pub fn apply(&self, marker: &str, value: &Value, args: &[String]) -> Result<Value, String> {
        let fuel = self.capabilities.fuel_limit.unwrap_or(MAX_FUEL);
        let mut store = wasmi::Store::new(&self.engine, ());
        store.set_fuel(fuel).map_err(|e| format!("fuel error: {}", e))?;

        let linker = wasmi::Linker::<()>::new(&self.engine);
        let instance = linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(|e| format!("WASM instantiation error: {}", e))?;

        // Serialize input
        let value_json = value_to_json_string(value);
        let args_json: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let input = serde_json::json!({
            "marker": marker,
            "value": value_json,
            "args": args_json,
        });
        let input_bytes = input.to_string().into_bytes();

        if input_bytes.len() > MAX_IO_SIZE {
            return Err("marker input too large".to_string());
        }

        // Get exports
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| "WASM module missing memory export".to_string())?;

        let synx_alloc = instance
            .get_typed_func::<i32, i32>(&store, "synx_alloc")
            .map_err(|e| format!("missing synx_alloc export: {}", e))?;

        let synx_apply = instance
            .get_typed_func::<(i32, i32), i64>(&store, "synx_apply")
            .map_err(|e| format!("missing synx_apply export: {}", e))?;

        // Allocate and write input into WASM memory
        let in_ptr = synx_alloc
            .call(&mut store, input_bytes.len() as i32)
            .map_err(|e| format!("synx_alloc failed: {}", e))?;

        memory
            .write(&mut store, in_ptr as usize, &input_bytes)
            .map_err(|e| format!("memory write failed: {}", e))?;

        // Call synx_apply
        let packed = synx_apply
            .call(&mut store, (in_ptr, input_bytes.len() as i32))
            .map_err(|e| format!("synx_apply failed: {}", e))?;

        let (out_ptr, out_len) = unpack_ptr_len(packed);

        if out_len == 0 || out_len as usize > MAX_IO_SIZE {
            return Err("synx_apply returned invalid length".to_string());
        }

        // Read output from WASM memory
        let data = memory.data(&store);
        let start = out_ptr as usize;
        let end = start + out_len as usize;
        if end > data.len() {
            return Err("synx_apply returned out-of-bounds pointer".to_string());
        }

        let out_bytes = &data[start..end];
        let out_str = std::str::from_utf8(out_bytes)
            .map_err(|_| "synx_apply returned invalid UTF-8".to_string())?;

        // Parse output JSON
        let output: serde_json::Value = serde_json::from_str(out_str)
            .map_err(|e| format!("synx_apply returned invalid JSON: {}", e))?;

        if let Some(err) = output.get("error").and_then(|v| v.as_str()) {
            return Err(format!("WASM marker '{}' error: {}", marker, err));
        }

        match output.get("value") {
            Some(v) => Ok(json_to_value(v)),
            None => Err("synx_apply output missing 'value' field".to_string()),
        }
    }
}

impl WasmMarkerRuntime {
    /// Create a new empty runtime.
    pub fn new() -> Self {
        WasmMarkerRuntime {
            modules: Vec::new(),
            dispatch: HashMap::new(),
        }
    }

    /// Load a WASM marker module and register its markers.
    pub fn load_module(&mut self, wasm_bytes: &[u8], capabilities: WasmCapabilities) -> Result<Vec<String>, String> {
        let module = WasmMarkerModule::from_bytes(wasm_bytes, capabilities)?;
        let idx = self.modules.len();
        let names = module.markers.clone();

        for name in &names {
            if self.dispatch.contains_key(name) {
                return Err(format!("marker '{}' already registered by another module", name));
            }
            self.dispatch.insert(name.clone(), idx);
        }

        self.modules.push(module);
        Ok(names)
    }

    /// Check if a marker name is handled by a WASM module.
    pub fn has_marker(&self, name: &str) -> bool {
        self.dispatch.contains_key(name)
    }

    /// Apply a custom WASM marker.
    pub fn apply_marker(&self, marker: &str, value: &Value, args: &[String]) -> Result<Value, String> {
        let idx = self.dispatch.get(marker)
            .ok_or_else(|| format!("no WASM module provides marker '{}'", marker))?;
        self.modules[*idx].apply(marker, value, args)
    }

    /// List all registered WASM marker names.
    pub fn marker_names(&self) -> Vec<&str> {
        self.dispatch.keys().map(|s| s.as_str()).collect()
    }
}

/// Convert a SYNX Value to a JSON string for passing to WASM.
fn value_to_json_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            let mut buf = ryu::Buffer::new();
            buf.format(*f).to_string()
        }
        Value::String(s) | Value::Secret(s) => s.clone(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_json_string).collect();
            format!("[{}]", items.join(","))
        }
        Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, value_to_json_string(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
    }
}

/// Convert a serde_json::Value back to a SYNX Value.
fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        // WASM module output strings are preserved as-is, no auto-casting.
        // The module explicitly chose the type — e.g. ":pad" returns "000042" as a string.
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::Array(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(map) => {
            let mut m = HashMap::new();
            for (k, val) in map {
                m.insert(k.clone(), json_to_value(val));
            }
            Value::Object(m)
        }
    }
}

/// The set of all 26 built-in marker names.
pub const BUILTIN_MARKERS: &[&str] = &[
    "spam", "include", "import", "env", "random", "ref", "i18n", "calc",
    "alias", "secret", "unique", "geo", "template", "split", "join",
    "default", "clamp", "round", "map", "format", "fallback", "once",
    "version", "watch", "prompt", "vision", "audio",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the built example WASM marker module.
    fn upper_wasm_path() -> std::path::PathBuf {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("examples");
        path.push("wasm-marker-upper");
        path.push("target");
        path.push("wasm32-unknown-unknown");
        path.push("release");
        path.push("synx_marker_upper.wasm");
        path
    }

    fn load_upper_module() -> Option<Vec<u8>> {
        let path = upper_wasm_path();
        if path.is_file() {
            std::fs::read(&path).ok()
        } else {
            None
        }
    }

    #[test]
    fn test_wasm_module_load_and_discover_markers() {
        let bytes = match load_upper_module() {
            Some(b) => b,
            None => {
                eprintln!("SKIP: wasm-marker-upper not built");
                return;
            }
        };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        assert_eq!(module.markers, vec!["upper", "lower", "reverse", "base64", "hash", "truncate", "pad", "count"]);
    }

    #[test]
    fn test_wasm_module_apply_upper() {
        let bytes = match load_upper_module() {
            Some(b) => b,
            None => {
                eprintln!("SKIP: wasm-marker-upper not built");
                return;
            }
        };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let input = Value::String("hello world".to_string());
        let result = module.apply("upper", &input, &[]).unwrap();
        assert_eq!(result, Value::String("HELLO WORLD".to_string()));
    }

    #[test]
    fn test_wasm_runtime_dispatch() {
        let bytes = match load_upper_module() {
            Some(b) => b,
            None => {
                eprintln!("SKIP: wasm-marker-upper not built");
                return;
            }
        };
        let mut runtime = WasmMarkerRuntime::new();
        let names = runtime.load_module(&bytes, WasmCapabilities::default()).unwrap();
        assert_eq!(names, vec!["upper", "lower", "reverse", "base64", "hash", "truncate", "pad", "count"]);
        assert!(runtime.has_marker("upper"));
        assert!(runtime.has_marker("lower"));
        assert!(runtime.has_marker("reverse"));
        assert!(runtime.has_marker("hash"));
        assert!(!runtime.has_marker("nonexistent"));

        let result = runtime.apply_marker("upper", &Value::String("test".into()), &[]).unwrap();
        assert_eq!(result, Value::String("TEST".to_string()));
    }

    #[test]
    fn test_wasm_module_too_large() {
        let big = vec![0u8; MAX_WASM_MODULE_SIZE + 1];
        let result = WasmMarkerModule::from_bytes(&big, WasmCapabilities::default());
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.contains("too large"));
    }

    #[test]
    fn test_wasm_invalid_module() {
        let garbage = b"not a wasm module";
        let result = WasmMarkerModule::from_bytes(garbage, WasmCapabilities::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_builtin_markers_complete() {
        assert!(BUILTIN_MARKERS.len() >= 26);
        assert!(BUILTIN_MARKERS.contains(&"env"));
        assert!(BUILTIN_MARKERS.contains(&"calc"));
        assert!(BUILTIN_MARKERS.contains(&"audio"));
        assert!(!BUILTIN_MARKERS.contains(&"upper"));
    }

    #[test]
    fn test_capabilities_from_manifest() {
        let caps = WasmCapabilities::from_manifest_line("env_read, fs_read");
        assert!(caps.env_read);
        assert!(caps.fs_read);
        assert!(!caps.network);

        let empty = WasmCapabilities::from_manifest_line("");
        assert!(!empty.env_read);
        assert!(!empty.fs_read);
    }

    #[test]
    fn test_capabilities_default() {
        let caps = WasmCapabilities::default();
        assert!(!caps.env_read);
        assert!(!caps.fs_read);
        assert!(!caps.network);
        assert!(caps.fuel_limit.is_none());
    }

    #[test]
    fn test_value_to_json() {
        assert_eq!(value_to_json_string(&Value::Null), "null");
        assert_eq!(value_to_json_string(&Value::Bool(true)), "true");
        assert_eq!(value_to_json_string(&Value::Int(42)), "42");
        assert_eq!(value_to_json_string(&Value::String("hi".into())), "hi");
    }

    #[test]
    fn test_wasm_marker_lower() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("lower", &Value::String("HELLO".into()), &[]).unwrap();
        assert_eq!(result, Value::String("hello".to_string()));
    }

    #[test]
    fn test_wasm_marker_reverse() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("reverse", &Value::String("abc".into()), &[]).unwrap();
        assert_eq!(result, Value::String("cba".to_string()));
    }

    #[test]
    fn test_wasm_marker_base64() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("base64", &Value::String("hi".into()), &[]).unwrap();
        assert_eq!(result, Value::String("aGk".to_string()));
    }

    #[test]
    fn test_wasm_marker_hash() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("hash", &Value::String("test".into()), &[]).unwrap();
        // FNV-1a hash is deterministic, just check it's a 16-char hex string
        if let Value::String(s) = &result {
            assert_eq!(s.len(), 16);
            assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        } else {
            panic!("expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_wasm_marker_count() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("count", &Value::String("hello".into()), &[]).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn test_wasm_marker_truncate() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("truncate", &Value::String("abcdefghij".into()), &["5".to_string()]).unwrap();
        assert_eq!(result, Value::String("ab...".to_string()));
    }

    #[test]
    fn test_wasm_marker_pad() {
        let bytes = match load_upper_module() { Some(b) => b, None => return };
        let module = WasmMarkerModule::from_bytes(&bytes, WasmCapabilities::default()).unwrap();
        let result = module.apply("pad", &Value::String("42".into()), &["6".to_string(), "0".to_string()]).unwrap();
        assert_eq!(result, Value::String("000042".to_string()));
    }
}
