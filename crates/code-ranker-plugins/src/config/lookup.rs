//! Generic data-list lookups over a merged config: the `[units]` id table, the
//! top-level string arrays (`extensions`, `resolution_order`, …) and free-form
//! `[<section>]` string sub-tables a language's structure builder keys on. Pure
//! data accessors — the collection / resolution / detection LOGIC stays in Rust.

use std::collections::BTreeMap;
use toml::Table;

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

/// File-walk ignore toggles fed to the shared file walker [`crate::walk::collect`]
/// from the CLI's `[ignore]` config (via [`code_ranker_plugin_api::plugin::PluginInput`]).
/// The walk LOGIC stays in Rust; *which* ignore sources it honours is data.
#[derive(Clone, Copy, Debug)]
pub struct IgnoreCfg {
    /// Respect `.gitignore` (+ global gitignore + `.git/info/exclude`).
    pub gitignore: bool,
    /// Respect `.ignore` files.
    pub ignore_files: bool,
    /// Skip hidden files / directories (dotfiles).
    pub hidden: bool,
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
