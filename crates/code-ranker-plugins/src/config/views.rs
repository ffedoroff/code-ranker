//! Level-descriptor views over a merged config: the `[edge_kinds]`,
//! `[node_kinds]`, `[node_attributes]` and `[edge_attributes]` tables a plugin
//! turns into its `levels()` spec, plus the `edge_kind_id` / `attr_key` lookups
//! the structure builder tags edges/attrs with.

use code_ranker_plugin_api::level::{AttributeSpec, EdgeKindSpec, NodeKindSpec};
use std::collections::BTreeMap;
use toml::Table;

/// Read the `[edge_kinds]` table from a merged config as `kind → EdgeKindSpec`
/// (empty if absent). The map key is the edge-kind identifier the structure
/// builder tags edges with (`uses`, `contains`, …); the value is its
/// label/description/flow. Shared kinds (`uses`) live in `defaults.toml`;
/// language-specific kinds (Rust's `contains`/`reexports`/`super`) in the
/// `<lang>.toml`, merged over the base.
pub fn edge_kinds(cfg: &Table) -> BTreeMap<String, EdgeKindSpec> {
    cfg.get("edge_kinds")
        .cloned()
        .map(|v| v.try_into().expect("[edge_kinds] shape"))
        .unwrap_or_default()
}

/// The identifier string the structure builder must tag a given edge kind with.
///
/// The identifier *is* the `[edge_kinds.<key>]` table key, so this looks `key`
/// up in the merged config and returns it only when that kind is declared — the
/// builder never tags an edge with a kind the level descriptor (built from the
/// same `[edge_kinds]`) does not also publish. A missing key is an authoring bug
/// (the kind was tagged but not described), surfaced by the `.expect` callers.
pub fn edge_kind_id<'a>(cfg: &'a Table, key: &'a str) -> Option<&'a str> {
    cfg.get("edge_kinds")?.as_table()?.get(key)?;
    Some(key)
}

/// The identifier string the structure builder must tag a node attribute with.
///
/// Mirrors [`edge_kind_id`] for node attributes: the identifier *is* the
/// `[node_attributes.<key>]` table key, so this looks `key` up in the merged
/// config and returns it only when that attribute is declared — the builder never
/// inserts an attr with a key the level descriptor (built from the same
/// `[node_attributes]`) does not also publish. A missing key is an authoring bug,
/// surfaced by the `.expect` callers.
pub fn attr_key<'a>(cfg: &'a Table, key: &'a str) -> Option<&'a str> {
    cfg.get("node_attributes")?.as_table()?.get(key)?;
    Some(key)
}

/// Read the `[node_kinds]` table from a merged config as `kind → NodeKindSpec`
/// (empty if absent). Used for the function-level unit kinds (`function`,
/// `method`, …); the shared ones live in `defaults.toml`, language-specific ones
/// (ECMAScript's `arrow`/`generator`) in the `<lang>.toml`.
pub fn node_kinds(cfg: &Table) -> BTreeMap<String, NodeKindSpec> {
    cfg.get("node_kinds")
        .cloned()
        .map(|v| v.try_into().expect("[node_kinds] shape"))
        .unwrap_or_default()
}

/// Read the `[node_attributes]` table from a merged config as `key →
/// AttributeSpec` (empty if absent). These are the plugin-emitted STRUCTURAL
/// node attributes' display specs (`path`/`loc`/`visibility`/`external` shared in
/// `defaults.toml`; language-specific ones like Rust's `crate`/`items`/`unsafe`
/// in the `<lang>.toml`) — display DATA, not code (README §3).
pub fn node_attributes(cfg: &Table) -> BTreeMap<String, AttributeSpec> {
    cfg.get("node_attributes")
        .cloned()
        .map(|v| v.try_into().expect("[node_attributes] shape"))
        .unwrap_or_default()
}

/// Read the `[edge_attributes]` table from a merged config as `key →
/// AttributeSpec` (empty if absent).
pub fn edge_attributes(cfg: &Table) -> BTreeMap<String, AttributeSpec> {
    cfg.get("edge_attributes")
        .cloned()
        .map(|v| v.try_into().expect("[edge_attributes] shape"))
        .unwrap_or_default()
}
