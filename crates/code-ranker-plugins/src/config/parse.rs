//! Config parsing + the `defaults.toml` ⊕ `<lang>.toml` deep-merge.
//!
//! ## Merge semantics ([`deep_merge`])
//!
//! Base is `defaults.toml`, overlay is `<lang>.toml`. For each key:
//! - **table vs table** → recurse (per-key deep merge).
//! - **`[[presets]]` array of tables** → merge **by `id`**: a language preset
//!   with an `id` already present in the base replaces that entry in place;
//!   a new `id` is appended.
//! - **array patched by an op-table** (`{add,remove,replace,clear,prepend}`) →
//!   the inherited list is **mutated in place** (see `crate::list_override`);
//!   a plain array still replaces it wholesale.
//! - **any other value** (scalar, plain array, table-vs-non-table) → the
//!   language value **replaces** the base value outright.
//!
//! Keys present only in one side are kept as-is.

use toml::{Table, Value};

/// The common base every language inherits.
pub const DEFAULTS: &str = include_str!("../defaults.toml");

/// Parse `defaults.toml` and a language's `<lang>.toml` source and deep-merge
/// them (language overrides base). Panics on malformed TOML — both inputs are
/// `include_str!`'d compile-time constants, so a parse failure is a build-time
/// authoring bug, not a runtime condition.
pub fn load(lang_toml: &str) -> Table {
    let base: Table = DEFAULTS.parse().expect("defaults.toml parses");
    let overlay: Table = lang_toml.parse().expect("<lang>.toml parses");
    deep_merge(base, overlay)
}

/// Deep-merge `overlay` onto `base` (see module docs for the rules).
pub(crate) fn deep_merge(mut base: Table, overlay: Table) -> Table {
    for (key, ov) in overlay {
        match base.remove(&key) {
            Some(Value::Table(bt)) => match ov {
                Value::Table(ot) => {
                    base.insert(key, Value::Table(deep_merge(bt, ot)));
                }
                other => {
                    base.insert(key, other);
                }
            },
            Some(Value::Array(ba)) if key == "presets" => {
                if let Value::Array(oa) = ov {
                    base.insert(key, Value::Array(merge_presets(ba, oa)));
                } else {
                    base.insert(key, ov);
                }
            }
            // An inherited list patched by an op-table (`{add,remove,replace,
            // clear,prepend}`) is mutated in place; a plain array replaces it
            // wholesale (the historical behaviour). See `crate::list_override`.
            Some(Value::Array(ba)) => match &ov {
                Value::Table(t) if crate::list_override::is_list_op_table(t) => {
                    let patched = crate::list_override::patch_value_list(ba, &ov);
                    base.insert(key, Value::Array(patched));
                }
                _ => {
                    base.insert(key, ov);
                }
            },
            _ => {
                base.insert(key, ov);
            }
        }
    }
    base
}

/// Merge two `[[presets]]` arrays by the `id` field: an overlay preset whose
/// `id` matches a base entry replaces it in place; a new `id` is appended.
/// Entries without a string `id` are appended verbatim.
pub(crate) fn merge_presets(mut base: Vec<Value>, overlay: Vec<Value>) -> Vec<Value> {
    for ov in overlay {
        let ov_id = preset_id(&ov);
        match ov_id.and_then(|id| base.iter().position(|b| preset_id(b) == Some(id))) {
            Some(pos) => base[pos] = ov,
            None => base.push(ov),
        }
    }
    base
}

fn preset_id(v: &Value) -> Option<&str> {
    v.as_table()?.get("id")?.as_str()
}
