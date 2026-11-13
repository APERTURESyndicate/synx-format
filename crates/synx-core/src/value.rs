//! SYNX value types, metadata, and options.

use std::collections::HashMap;

/// Hasher for every SYNX object and metadata map.
///
/// perf: foldhash is several times faster than SipHash on short keys and is
/// randomly seeded per map, so hostile keys cannot be pre-computed to collide.
pub type MapHasher = foldhash::fast::RandomState;

/// An object: keys in source order.
///
/// spec: order is load-bearing — active-mode markers resolve in the order the
/// document declares them (`total:calc price + tax` after `tax:calc …`), as
/// in every other implementation. A `HashMap` made that order random per run.
pub type Map<V = Value> = indexmap::IndexMap<std::string::String, V, MapHasher>;

/// SYNX value types.
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Value {
    String(std::string::String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
    Object(Map),
    /// Secret value — displays, prints and serializes as `[SECRET]`; the real
    /// value is read with [`Value::as_secret`] (or [`Value::as_str`]).
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_masked"))]
    Secret(std::string::String),
}

/// safety: a derived `Serialize` wrote the secret itself, so anything built
/// on serde — a JSON dump, the WASM binding's objects — carried the plain
/// text that `to_json` and `Debug` mask.
#[cfg(feature = "serde")]
fn serialize_masked<S: serde::Serializer>(_secret: &str, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str("[SECRET]")
}

// Hand-written so `{:?}` in a log line never prints a secret.
impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => f.debug_tuple("String").field(s).finish(),
            Value::Int(i) => f.debug_tuple("Int").field(i).finish(),
            Value::Float(x) => f.debug_tuple("Float").field(x).finish(),
            Value::Bool(b) => f.debug_tuple("Bool").field(b).finish(),
            Value::Null => f.write_str("Null"),
            Value::Array(a) => f.debug_tuple("Array").field(a).finish(),
            Value::Object(o) => f.debug_tuple("Object").field(o).finish(),
            Value::Secret(_) => f.write_str("Secret([SECRET])"),
        }
    }
}

impl Value {
    /// The text of a string — and of a secret, in the clear: do not log it.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::Secret(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_secret(&self) -> Option<&str> {
        match self {
            Value::Secret(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_number_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => write!(f, "{}", s),
            // safety: `{}` is how values reach logs. It printed the secret.
            Value::Secret(_) => write!(f, "[SECRET]"),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(fl) => {
                let s = fl.to_string();
                if s.contains('.') { write!(f, "{}", s) } else { write!(f, "{}.0", s) }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Null => write!(f, "null"),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Object(_) => write!(f, "[Object]"),
        }
    }
}

impl std::ops::Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        match self {
            Value::Object(map) => map.get(key).expect("key not found"),
            _ => panic!("not an object"),
        }
    }
}

/// File mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Static,
    Active,
}

/// Metadata for a single key (markers, args, constraints).
#[derive(Clone, PartialEq)]
pub struct Meta {
    pub markers: Vec<String>,
    pub args: Vec<String>,
    pub type_hint: Option<String>,
    pub constraints: Option<Constraints>,
}

// safety: hand-written like `Value`'s. `key:secret:default:hunter2` keeps the
// secret's value as the argument of `:default`, in `markers`; a derived
// `Debug` put it into every `{:?}` of a parse result.
impl std::fmt::Debug for Meta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let secret = self.markers.iter().any(|m| m == "secret");
        let default_at = self.markers.iter().position(|m| m == "default").filter(|_| secret);
        let shown: Vec<&str> = match default_at {
            Some(at) if at + 1 < self.markers.len() => {
                self.markers[..=at].iter().map(String::as_str).chain(std::iter::once("[SECRET]")).collect()
            }
            _ => self.markers.iter().map(String::as_str).collect(),
        };
        f.debug_struct("Meta")
            .field("markers", &shown)
            .field("args", &self.args)
            .field("type_hint", &self.type_hint)
            .field("constraints", &self.constraints)
            .finish()
    }
}

/// Constraints from [min:3, max:30, required, type:int].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Constraints {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub type_name: Option<String>,
    pub required: bool,
    pub readonly: bool,
    pub pattern: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

/// Map of key → metadata for one object level.
pub type MetaMap = Map<Meta>;

/// Include directive: !include path [alias]
#[derive(Debug, Clone)]
pub struct IncludeDirective {
    pub path: String,
    pub alias: String,
}

/// Use directive: !use @scope/name [as alias]
#[derive(Debug, Clone)]
pub struct UseDirective {
    /// Full package name, e.g. "@aperture/synx-defaults"
    pub package: String,
    /// Namespace alias (defaults to last segment: "synx-defaults")
    pub alias: String,
}

/// Full parse result with metadata.
#[derive(Debug)]
pub struct ParseResult {
    pub root: Value,
    pub mode: Mode,
    pub locked: bool,
    /// `!tool` directive — file describes an LLM tool call or schema.
    pub tool: bool,
    /// `!schema` directive (combined with `!tool`) — file describes tool definitions.
    pub schema: bool,
    /// `!llm` directive — file is an LLM-oriented envelope (semantic hints for tools; data tree unchanged).
    pub llm: bool,
    /// Metadata for each nesting level, keyed by dot-path prefix.
    /// "" = root level, "server" = server sub-object, etc.
    pub metadata: Map<MetaMap>,
    /// !include directives parsed from the file.
    pub includes: Vec<IncludeDirective>,
    /// !use directives parsed from the file (package imports).
    pub uses: Vec<UseDirective>,
    /// One of the parser's resource limits dropped input: the total byte cap,
    /// the indexed-line cap, or a multiline block body cap.
    ///
    /// Callers that treat "parsed without complaint" as "nothing was lost"
    /// need this — SYNXL §11.2 turns it into a `RecordTruncated` diagnostic
    /// rather than discarding the overflow in silence.
    pub truncated: bool,
}

/// Options for active mode resolution.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub env: Option<HashMap<String, String>>,
    pub region: Option<String>,
    pub lang: Option<String>,
    pub base_path: Option<String>,
    /// Maximum depth for nested :include / :import / :watch file reads (default: 16).
    pub max_include_depth: Option<usize>,
    /// Internal counter — do not set manually.
    pub _include_depth: usize,
    /// Path to installed packages directory (default: "./synx_packages").
    pub packages_path: Option<String>,
    /// Loaded WASM marker runtime (populated by engine from `type markers` packages).
    #[cfg(feature = "wasm")]
    pub wasm_runtime: Option<std::sync::Arc<crate::wasm::WasmMarkerRuntime>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // B-027: printing a secret, in any form, never shows its text.
    #[test]
    fn a_secret_prints_as_its_mask() {
        let s = Value::Secret("hunter2".into());
        assert_eq!(format!("{s}"), "[SECRET]");
        assert!(!format!("{s:?}").contains("hunter2"));
        let mut map = Map::default();
        map.insert("pass".into(), s.clone());
        let tree = Value::Object(map);
        assert!(!format!("{tree}").contains("hunter2"));
        assert!(!format!("{tree:?}").contains("hunter2"));
        assert_eq!(s.as_secret(), Some("hunter2"));
        #[cfg(feature = "serde")]
        assert_eq!(serde_json::to_string(&tree).unwrap(), r#"{"pass":"[SECRET]"}"#);
    }

    // B-088: the default of a secret is its value; `{:?}` of the declarations
    // (and of a whole parse result) does not print it.
    #[test]
    fn a_secret_default_is_not_printed_with_its_declaration() {
        let parsed = crate::parse("!active\npass:secret:default:hunter2\nkey:secret:env:default:hun:ter2 KEY\nport:env:default:8080 PORT\n");
        let printed = format!("{:?}", parsed);
        assert!(!printed.contains("hunter2") && !printed.contains("ter2"), "{printed}");
        assert!(printed.contains(r#"["secret", "default", "[SECRET]"]"#), "{printed}");
        // What is not secret prints as it is, and the declarations are intact.
        assert!(printed.contains(r#"["env", "default", "8080"]"#), "{printed}");
        assert_eq!(parsed.metadata[""]["pass"].markers, ["secret", "default", "hunter2"]);
    }
}
