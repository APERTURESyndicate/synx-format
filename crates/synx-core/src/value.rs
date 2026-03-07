//! SYNX value types, metadata, and options.

use std::collections::HashMap;

/// SYNX value types.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(std::string::String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
    Object(HashMap<std::string::String, Value>),
    /// Secret value — displays as [SECRET], real value accessible via as_secret()
    Secret(std::string::String),
}

impl Value {
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

    pub fn as_object(&self) -> Option<&HashMap<std::string::String, Value>> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut HashMap<std::string::String, Value>> {
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
            Value::String(s) | Value::Secret(s) => write!(f, "{}", s),
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
#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
    pub markers: Vec<String>,
    pub args: Vec<String>,
    pub type_hint: Option<String>,
    pub constraints: Option<Constraints>,
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
pub type MetaMap = HashMap<String, Meta>;

/// Full parse result with metadata.
#[derive(Debug)]
pub struct ParseResult {
    pub root: Value,
    pub mode: Mode,
    pub locked: bool,
    /// Metadata for each nesting level, keyed by dot-path prefix.
    /// "" = root level, "server" = server sub-object, etc.
    pub metadata: HashMap<String, MetaMap>,
}

/// Options for active mode resolution.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub env: Option<HashMap<String, String>>,
    pub region: Option<String>,
    pub lang: Option<String>,
    pub base_path: Option<String>,
}
