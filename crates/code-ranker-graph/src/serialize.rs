//! Canonical (deterministic) JSON serialization. Object keys come out
//! alphabetically (`serde_json::Value` is backed by a `BTreeMap`) and the
//! `nodes`/`edges` arrays are sorted by a stable key, so unchanged input is
//! byte-identical across runs. Generic over any `Serialize` — depends on
//! nothing else in the crate.

use serde::Serialize;

/// Serialize to canonical pretty JSON: object keys come out alphabetically
/// (`serde_json::Value` is backed by a `BTreeMap`), and the `nodes`/`edges`
/// arrays are sorted by a stable key so unchanged input is byte-identical.
pub fn to_canonical_string_pretty<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string_pretty(&v)
}

/// Compact counterpart of [`to_canonical_string_pretty`].
pub fn to_canonical_string<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string(&v)
}

fn canonicalize_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                canonicalize_value(item);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                canonicalize_value(val);
            }
            if let Some(serde_json::Value::Array(nodes)) = map.get_mut("nodes") {
                nodes.sort_by_key(|a| json_str(a, "id"));
            }
            if let Some(serde_json::Value::Array(edges)) = map.get_mut("edges") {
                edges.sort_by(|a, b| {
                    json_str(a, "source")
                        .cmp(&json_str(b, "source"))
                        .then_with(|| json_str(a, "target").cmp(&json_str(b, "target")))
                        .then_with(|| json_str(a, "kind").cmp(&json_str(b, "kind")))
                });
            }
        }
        _ => {}
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}
