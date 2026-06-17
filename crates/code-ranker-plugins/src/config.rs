//! Shared config-inheritance layer for the language plugins.
//!
//! Every language ships a `<lang>.toml` that **inherits** the common
//! `defaults.toml` (see [`DEFAULTS`]). [`load`] deep-merges the two into one
//! [`toml::Table`] from which a plugin drives its `levels()` spec overrides,
//! `presets()`, `thresholds()` and the metric-engine node-kind tables — so the
//! per-language Rust stays thin (wiring only) and everything that *can* be data
//! lives in TOML.
//!
//! The merge is generic and language-agnostic, so python / js / ts can adopt the
//! same `defaults.toml` + `<lang>.toml` pattern later without touching this code.
//!
//! ## Merge semantics ([`deep_merge`])
//!
//! Base is `defaults.toml`, overlay is `<lang>.toml`. For each key:
//! - **table vs table** → recurse (per-key deep merge).
//! - **`[[presets]]` array of tables** → merge **by `id`**: a language preset
//!   with an `id` already present in the base replaces that entry in place;
//!   a new `id` is appended. (This lets a language extend the shared catalog
//!   and override individual entries without restating the whole list.)
//! - **array patched by an op-table** (`{add,remove,replace,clear,prepend}`) →
//!   the inherited list is **mutated in place** (see `crate::list_override`);
//!   a plain array still replaces it wholesale.
//! - **any other value** (scalar, plain array, table-vs-non-table) → the
//!   language value **replaces** the base value outright.
//!
//! Keys present only in one side are kept as-is.

use code_ranker_plugin_api::level::{AttributeSpec, EdgeKindSpec, NodeKindSpec};
use code_ranker_plugin_api::plugin::Preset;
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::Table;
use toml::Value;

/// The common base every language inherits. Minimal for now — the merge
/// mechanism is the point; languages still carry their own diffs.
pub const DEFAULTS: &str = include_str!("defaults.toml");

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
fn deep_merge(mut base: Table, overlay: Table) -> Table {
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
fn merge_presets(mut base: Vec<Value>, overlay: Vec<Value>) -> Vec<Value> {
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

/// One `[[presets]]` entry as read from config. Mirrors the data shape of the
/// CLI's generic preset catalog; the plugin turns it into a
/// `code_ranker_plugin_api::plugin::Preset`, deriving `doc_url` from a `slug`.
#[derive(Debug, Clone, Deserialize)]
pub struct PresetCfg {
    pub id: String,
    pub title: String,
    pub sort_metric: String,
    #[serde(default)]
    pub connections: Vec<String>,
    pub slug: String,
    pub prompt: String,
}

/// One `[specs.<key>]` entry: per-language overrides applied over the central
/// `builtin.toml` attribute specs. Only the fields a language tweaks are set;
/// the rest are left untouched on the inherited spec.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpecOverride {
    #[serde(default)]
    pub description: Option<String>,
}

/// Read the `[[presets]]` array from a merged config (empty if absent).
pub fn presets(cfg: &Table) -> Vec<PresetCfg> {
    cfg.get("presets")
        .cloned()
        .map(|v| v.try_into().expect("[[presets]] shape"))
        .unwrap_or_default()
}

/// Read a top-level string key from a merged config.
fn string_field<'a>(cfg: &'a Table, key: &str) -> Option<&'a str> {
    cfg.get(key)?.as_str()
}

/// Build the fully-resolved [`Preset`] list from a merged config: the common
/// catalog (from `defaults.toml`) plus any language-specific presets, in that
/// order (the merge-by-`id` already yields it). Each `doc_url` resolves to
/// `{doc_base}/{doc_lang}/{slug}.md` and `label` is the `id`.
///
/// `doc_base` (the host/repo prefix, common) lives in `defaults.toml`; each
/// `<lang>.toml` supplies `doc_lang` (its principle-corpus language). If either
/// is absent the `doc_url` is left `None`.
pub fn resolved_presets(cfg: &Table) -> Vec<Preset> {
    let base = string_field(cfg, "doc_base");
    let lang = string_field(cfg, "doc_lang");
    presets(cfg)
        .into_iter()
        .map(|p| Preset {
            doc_url: base
                .zip(lang)
                .map(|(b, l)| format!("{b}/{l}/{}.md", p.slug)),
            label: p.id.clone(),
            id: p.id,
            title: p.title,
            prompt: p.prompt,
            sort_metric: p.sort_metric,
            connections: p.connections,
        })
        .collect()
}

/// Read the `[thresholds]` table from a merged config as `key → (info, warning)`
/// (empty if absent).
pub fn thresholds(cfg: &Table) -> BTreeMap<String, ThresholdCfg> {
    cfg.get("thresholds")
        .cloned()
        .map(|v| v.try_into().expect("[thresholds] shape"))
        .unwrap_or_default()
}

/// One `[thresholds.<key>]` row.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ThresholdCfg {
    pub info: f64,
    pub warning: f64,
}

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

/// Read the `[units]` table from a merged config as `key → id-string`
/// (empty if absent). These are the display `kind` id strings a dialect's
/// `fn_kind` emits for a function unit (`method`/`fn`/`function`/`arrow`/…);
/// the classification LOGIC (the parent/kind checks) stays in the dialect, only
/// the emitted id is data. The map key is the dialect's internal classification
/// slot (`method`, `default`, `arrow`, `generator`), the value the id string.
pub fn units(cfg: &Table) -> BTreeMap<String, String> {
    cfg.get("units")
        .cloned()
        .map(|v| v.try_into().expect("[units] shape"))
        .unwrap_or_default()
}

/// Read a top-level array-of-strings `key` from a merged config as
/// `Vec<String>` (empty if absent or not an array of strings). Used for the
/// DATA lists a language's file collection / import resolution / project
/// detection drive on — `extensions`, `resolution_order`, `detect_markers`,
/// `skip_dirs` — which are data, not code (see `languages/README.md` §3). The
/// collection / resolution / detection LOGIC stays in Rust; the list it walks
/// is config. Order is preserved verbatim (it matters for `resolution_order`).
pub fn string_list(cfg: &Table, key: &str) -> Vec<String> {
    cfg.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Read a `[<section>]` sub-table from a merged config as `key → string`
/// (empty if absent). Used for free-form vocab tables a structure builder keys
/// on — e.g. a language's `[structure]` tree-sitter node-kind strings.
pub fn string_table(cfg: &Table, section: &str) -> BTreeMap<String, String> {
    cfg.get(section)
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Read the `[specs]` table from a merged config as `key → override`
/// (empty if absent).
pub fn spec_overrides(cfg: &Table) -> BTreeMap<String, SpecOverride> {
    cfg.get("specs")
        .cloned()
        .map(|v| v.try_into().expect("[specs] shape"))
        .unwrap_or_default()
}

/// Apply a config's `[specs.<key>]` description overrides over the central
/// builtin metric specs — the shared body of every plugin's `metric_specs`. A
/// language refines a metric's description (e.g. enumerating the exact Halstead
/// operators/operands it counts) without restating the rest of the spec; an
/// override whose key isn't a known metric is ignored.
pub fn apply_spec_overrides(
    mut defaults: BTreeMap<String, AttributeSpec>,
    cfg: &Table,
) -> BTreeMap<String, AttributeSpec> {
    for (key, ov) in spec_overrides(cfg) {
        if let Some(spec) = defaults.get_mut(&key)
            && let Some(desc) = ov.description
        {
            spec.description = Some(desc);
        }
    }
    defaults
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod config_tests;
