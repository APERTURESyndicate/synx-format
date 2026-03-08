//! SYNX WebAssembly binding — parse SYNX in the browser.
//! Returns JSON strings by default (most efficient across WASM boundary).
//! Also provides parse_object for direct JsValue output via serde_wasm_bindgen.

use wasm_bindgen::prelude::*;
use synx_core::{self, Mode, Options};

/// Parse a SYNX string and return a JSON string.
#[wasm_bindgen]
pub fn parse(text: &str) -> String {
    let result = synx_core::parse(text);
    synx_core::to_json(&result.root)
}

/// Parse a SYNX string and return a JS object directly (no JSON.parse needed).
#[wasm_bindgen]
pub fn parse_object(text: &str) -> JsValue {
    let result = synx_core::parse(text);
    serde_wasm_bindgen::to_value(&result.root).unwrap_or(JsValue::NULL)
}

/// Parse a SYNX string with engine resolution and return a JSON string.
/// Note: :include and :env markers have limited support in the browser.
#[wasm_bindgen]
pub fn parse_active(text: &str) -> String {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        synx_core::resolve(&mut result, &Options::default());
    }
    synx_core::to_json(&result.root)
}

/// Parse with engine resolution and return a JS object directly.
#[wasm_bindgen]
pub fn parse_active_object(text: &str) -> JsValue {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        synx_core::resolve(&mut result, &Options::default());
    }
    serde_wasm_bindgen::to_value(&result.root).unwrap_or(JsValue::NULL)
}

/// Convert a JSON string (representing a SYNX data structure) back to SYNX format.
#[wasm_bindgen]
pub fn stringify(json: &str) -> String {
    match serde_json::from_str::<synx_core::Value>(json) {
        Ok(val) => synx_core::Synx::stringify(&val),
        Err(_) => String::new(),
    }
}

/// Reformat a SYNX string into canonical form (sorted keys, normalized indentation).
#[wasm_bindgen]
pub fn format(text: &str) -> String {
    synx_core::Synx::format(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_parse_and_parse_active() {
        let json = parse("name John\nage 25\n");
        assert!(json.contains("\"name\":\"John\""));

        let active = parse_active("!active\nname John\n");
        assert!(active.contains("\"name\":\"John\""));
    }

    #[test]
    fn smoke_stringify_and_format() {
        let synx = stringify("{\"name\":\"John\",\"age\":25}");
        assert!(synx.contains("name John"));
        assert!(synx.contains("age 25"));

        let formatted = format("b 2\na 1\n");
        assert!(formatted.contains("a 1"));
        assert!(formatted.contains("b 2"));
    }
}
