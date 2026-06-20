//! The report list-override DSL: parse a `[report]` section into a
//! [`ReportOverride`] of per-list [`ListPatch`]es, plus the generic op-table
//! primitives [`is_list_op_table`] / [`patch_value_list`] reused by the TOML
//! inheritance merge ([`crate::toml_merge::deep_merge`]).
//!
//! The report's table `columns`, card-featured metrics, and JSON `stats` keys are
//! inherited from the global metric catalog. A config patches an inherited list
//! rather than restating it: a plain array replaces it wholesale, while an
//! **op-table** mutates it in place —
//!
//! ```toml
//! [report]
//! columns = { remove = ["volume", "effort"], add = ["unsafe"] }
//! stats   = { add = ["unsafe"] }
//! card    = { replace = { "sloc" = "unsafe" } }
//! ```
//!
//! The op semantics (`clear` → `remove` → `replace` → `after`/`before` →
//! `prepend` → `add`, then dedup) live in [`ListPatch::apply`]. The orchestrator
//! applies the patch over the catalog list, then prunes to keys present.
//!
//! Lives in `code-ranker-plugin-api` (next to [`ReportOverride`]) so both the
//! language plugins (`<lang>.toml` `[report]`) and the CLI (a project
//! `code-ranker.toml` `[report]`, and its config inheritance merge) use it without
//! reaching into a sibling crate.

use crate::report::{ListPatch, ReportOverride};
use toml::{Table, Value};

/// The op-table keys that mark a `[table]` as a list patch rather than a value.
const LIST_OP_KEYS: [&str; 7] = [
    "add", "remove", "replace", "prepend", "clear", "after", "before",
];

/// True when `t` is a list-op table (carries at least one op key) — i.e. it
/// patches an inherited list rather than replacing it with a value.
pub fn is_list_op_table(t: &Table) -> bool {
    LIST_OP_KEYS.iter().any(|k| t.contains_key(*k))
}

/// Apply an op-table `ov` to a string-list `base` (used by `deep_merge`). A
/// non-string base can't be patched by value, so it is kept unchanged.
pub fn patch_value_list(base: Vec<Value>, ov: &Value) -> Vec<Value> {
    let strs: Option<Vec<String>> = base
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect();
    match strs {
        Some(strs) => list_patch(ov)
            .apply(&strs)
            .into_iter()
            .map(Value::String)
            .collect(),
        None => base,
    }
}

/// Read the `[report]` section of a merged config table as a [`ReportOverride`]
/// (used for a language's `<lang>.toml`, which nests it under `report`).
pub fn report_override(cfg: &Table) -> ReportOverride {
    cfg.get("report")
        .and_then(Value::as_table)
        .map(report_override_section)
        .unwrap_or_default()
}

/// Read a bare `[report]` section table (its `columns` / `card` / `stats` keys)
/// as a [`ReportOverride`]. Used for the project `code-ranker.toml`, where the
/// section is parsed into a table directly.
pub fn report_override_section(report: &Table) -> ReportOverride {
    let patch = |key: &str| report.get(key).map(list_patch).unwrap_or_default();
    ReportOverride {
        columns: patch("columns"),
        card: patch("card"),
        stats: patch("stats"),
        size: patch("size"),
        filter: patch("filter"),
    }
}

/// Extract a `Vec<String>` from a TOML array value (string elements only).
fn value_strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a TOML value into a [`ListPatch`]: a plain array → `replace_all`; an
/// op-table → the corresponding add/remove/replace/clear/prepend ops.
fn list_patch(v: &Value) -> ListPatch {
    match v {
        Value::Array(a) => ListPatch {
            replace_all: Some(
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
            ),
            ..Default::default()
        },
        Value::Table(t) => ListPatch {
            replace_all: None,
            clear: t.get("clear").and_then(Value::as_bool).unwrap_or(false),
            remove: value_strs(t.get("remove")),
            replace: t
                .get("replace")
                .and_then(Value::as_table)
                .map(|rt| {
                    rt.iter()
                        .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            after: anchor_pairs(t.get("after")),
            before: anchor_pairs(t.get("before")),
            prepend: value_strs(t.get("prepend")),
            add: value_strs(t.get("add")),
        },
        _ => ListPatch::default(),
    }
}

/// Parse an anchor → items table (`{ hk = ["tsr", "tsr_big"] }`) for the
/// `after` / `before` positional inserts.
fn anchor_pairs(v: Option<&Value>) -> Vec<(String, Vec<String>)> {
    v.and_then(Value::as_table)
        .map(|t| {
            t.iter()
                .map(|(anchor, items)| (anchor.clone(), value_strs(Some(items))))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "list_override_test.rs"]
mod tests;
