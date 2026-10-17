//! Canonical JSON and the edit-stack hash (SPEC §6.6.1).

use crate::edit::EditOp;
use serde_json::Value;
use std::fmt::Write;

/// Canonical JSON: object keys sorted, no whitespace, floats with exactly six decimals
/// (negative zero normalized), integers as written.
pub(crate) fn canonical_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().unwrap_or(0.0);
                let mut text = format!("{f:.6}");
                if text == "-0.000000" {
                    text = "0.000000".to_owned();
                }
                out.push_str(&text);
            } else {
                out.push_str(&n.to_string());
            }
        }
        Value::String(s) => {
            out.push_str(&Value::String(s.clone()).to_string());
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_json(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}:", Value::String(key.clone()));
                if let Some(v) = map.get(key) {
                    canonical_json(v, out);
                }
            }
            out.push('}');
        }
    }
}

pub(crate) fn canonical_ops_json(ops: &[EditOp]) -> String {
    let value = serde_json::to_value(ops).unwrap_or(Value::Null);
    let mut out = String::new();
    canonical_json(&value, &mut out);
    out
}

pub(crate) fn edit_stack_hash(ops: &[EditOp]) -> String {
    blake3::hash(canonical_ops_json(ops).as_bytes())
        .to_hex()
        .to_string()
}
