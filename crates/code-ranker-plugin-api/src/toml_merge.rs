//! Generic TOML table **inheritance merge** — the primitive both the language
//! plugins (`defaults.toml ⊕ [base] ⊕ <lang>.toml`) and the CLI (built-in defaults
//! ⊕ a project `code-ranker.toml`) layer config with. Lives in
//! `code-ranker-plugin-api` so neither consumer reaches into a sibling crate.
//!
//! ## Merge semantics ([`deep_merge`])
//!
//! For each key of `overlay` applied onto `base`:
//! - **table vs table** → recurse (per-key deep merge).
//! - **`[[presets]]` array of tables** → merge **by `id`**: an overlay preset with
//!   an `id` already present in the base replaces that entry in place; a new `id`
//!   is appended.
//! - **array patched by an op-table** (`{add,remove,replace,clear,prepend,…}`) →
//!   the inherited list is **mutated in place** (see [`crate::list_override`]); a
//!   plain array still replaces it wholesale.
//! - **any other value** (scalar, plain array, table-vs-non-table) → the overlay
//!   value **replaces** the base value outright.
//!
//! Keys present only in one side are kept as-is.

use crate::list_override::{is_list_op_table, patch_value_list};
use toml::{Table, Value};

/// Deep-merge `overlay` onto `base` (see module docs for the rules).
pub fn deep_merge(mut base: Table, overlay: Table) -> Table {
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
                Value::Table(t) if is_list_op_table(t) => {
                    let patched = patch_value_list(ba, &ov);
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
pub fn merge_presets(mut base: Vec<Value>, overlay: Vec<Value>) -> Vec<Value> {
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
