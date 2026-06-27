//! Thin CLI-side accessors over the plugin registry. The CLI NEVER names a
//! concrete language: plugins self-register via `inventory::submit!` in the
//! `code-ranker-plugins` crate and are collected by `code_ranker_plugin_api::registry`.
//! Everything here works only through the `LanguagePlugin` trait and the plugin's
//! `name()`. Adding a language is a self-contained module in the plugins crate.
//!
//! Multiple plugins matching the auto-detect heuristics is NORMAL (e.g. a
//! project with both Rust sources and Markdown docs). `detect_all` returns all
//! matching plugins sorted; `resolve_plugins` applies the precedence chain.

use anyhow::{Result, bail};
use code_ranker_graph::version::CONFIG_VERSION;
use code_ranker_graph::write_metrics;
use code_ranker_plugin_api::{
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
    principle::Principle,
    toml_merge::deep_merge,
};
use std::collections::BTreeMap;
use std::path::Path;

/// Read a top-level string array from a TOML table (e.g. `extensions = ["rs"]`).
/// Returns an empty `Vec` when the key is absent or is not a string array.
fn toml_string_list(cfg: &toml::Table, key: &str) -> Vec<String> {
    cfg.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Every self-registered language plugin (see `code_ranker_plugin_api::registry`).
/// The CLI links the `code-ranker-plugins` crate (its `deep_merge` / `list_override`
/// are used elsewhere), so every plugin's `inventory::submit!` is collected here.
pub fn registry() -> Vec<&'static dyn LanguagePlugin> {
    code_ranker_plugin_api::registry()
}

/// Comma-separated canonical plugin names (sorted for stable help/error output;
/// the registry's link order is not significant).
pub fn names() -> String {
    let mut names: Vec<&str> = registry().iter().map(|p| p.name()).collect();
    names.sort_unstable();
    names.join(", ")
}

/// Parse the workspace with the named plugin at the `"files"` level, returning
/// the structural graph and the plugin's level descriptors.
/// `cfg` is the effective plugin config (static base ⊕ user overrides).
pub fn analyze(
    name: &str,
    cfg: &toml::Table,
    workspace: &Path,
    input: &PluginInput,
) -> Result<(Graph, Vec<Level>)> {
    let reg = registry();
    match reg.iter().find(|p| p.name() == name) {
        Some(p) => {
            let graph = p.analyze(cfg, workspace, input)?;
            Ok((graph, p.levels(cfg)))
        }
        None => bail!("unknown plugin {name:?}; built-in plugins are: {}", names()),
    }
}

/// Have the matching plugin **measure** its per-language complexity inputs, then
/// write every metric (tier-1 + the tier-2 registry derivations) onto the graph's
/// file nodes here, in the orchestrator. Returns the number of nodes annotated.
/// `cfg` is the effective plugin config.
pub fn annotate_metrics(name: &str, cfg: &toml::Table, graph: &mut Graph) -> usize {
    let reg = registry();
    let Some(p) = reg.iter().find(|p| p.name() == name) else {
        return 0;
    };
    let by_id: BTreeMap<String, MetricInputs> = p.metrics(cfg, graph).into_iter().collect();
    let mut annotated = 0;
    for node in &mut graph.nodes {
        if let Some(inputs) = by_id.get(&node.id) {
            write_metrics(node, inputs);
            annotated += 1;
        }
    }
    annotated
}

/// Ask the matching plugin for function-level metric units (one per sub-file
/// unit), for the optional `functions` level, then write their metrics onto the
/// returned nodes here. Called on the absolute-id graph; returns nodes whose
/// `parent` is the file id. Empty when the plugin ships no function-level support.
/// `cfg` is the effective plugin config.
pub fn function_units(name: &str, cfg: &toml::Table, graph: &Graph) -> Vec<Node> {
    let reg = registry();
    let Some(p) = reg.iter().find(|p| p.name() == name) else {
        return Vec::new();
    };
    p.function_units(cfg, graph)
        .into_iter()
        .map(|(mut node, inputs)| {
            write_metrics(&mut node, &inputs);
            node
        })
        .collect()
}

/// Tool/toolchain versions the matching plugin wants recorded in the snapshot.
/// `cfg` is the effective plugin config.
pub fn versions(
    name: &str,
    cfg: &toml::Table,
    workspace: &Path,
    input: &PluginInput,
) -> Vec<(String, String)> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.versions(cfg, workspace, input))
        .unwrap_or_default()
}

/// Named external-path roots the matching plugin contributes for shortening node
/// ids (e.g. Rust's `cargo` / `registry` / `rustup` / `rust-src`). Language
/// knowledge lives in the plugin; the orchestrator only adds the generic
/// `target` root on top. `cfg` is the effective plugin config.
pub fn roots(name: &str, cfg: &toml::Table, workspace: &Path) -> Vec<(String, String)> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.roots(cfg, workspace))
        .unwrap_or_default()
}

/// The matching plugin's report-list overrides (table `columns` / card / JSON
/// `stats`), applied by the orchestrator over the global catalog lists.
/// `cfg` is the effective plugin config.
pub fn report_overrides(
    name: &str,
    cfg: &toml::Table,
) -> code_ranker_plugin_api::report::ReportOverride {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.report_overrides(cfg))
        .unwrap_or_default()
}

/// The matching plugin's Prompt-Generator principles (the common catalog plus any
/// language-specific principles), built from its own config.
/// `cfg` is the effective plugin config.
pub fn principles(name: &str, cfg: &toml::Table, input: &PluginInput) -> Vec<Principle> {
    match registry().iter().find(|p| p.name() == name) {
        Some(p) => p.principles(cfg, input),
        None => Vec::new(),
    }
}

/// The matching plugin's level specs — its node-attribute / edge-kind / group
/// dictionaries, built from config with **no analysis**. The `docs` command reads
/// the `files` level to surface a language's own structural metrics (e.g. Rust's
/// `unsafe`, `items`) without walking a source tree.
/// `cfg` is the effective plugin config.
pub fn levels(name: &str, cfg: &toml::Table) -> Vec<code_ranker_plugin_api::level::Level> {
    match registry().iter().find(|p| p.name() == name) {
        Some(p) => p.levels(cfg),
        None => Vec::new(),
    }
}

/// Let the matching plugin refine the language-neutral default metric specs
/// (e.g. add Rust-specific `#[cfg(test)]` nuance to LOC descriptions). The
/// neutral catalog comes from `code-ranker-graph`; the plugin overrides only
/// what differs for its language. `cfg` is the effective plugin config.
pub fn metric_specs(
    name: &str,
    cfg: &toml::Table,
    defaults: BTreeMap<String, AttributeSpec>,
) -> BTreeMap<String, AttributeSpec> {
    match registry().iter().find(|p| p.name() == name) {
        Some(p) => p.metric_specs(cfg, defaults),
        None => defaults,
    }
}

/// Build the effective plugin config for `name`:
///   static plugin base (`plugin.config()`)
///   ⊕ user `[languages.base]`
///   ⊕ user `[languages.<name>]`
///
/// `lang_overrides` is `Config.languages` — the raw per-language tables already
/// carrying any `--config languages.*` edits. The merge uses `deep_merge` from
/// `code_ranker_plugin_api`, matching the rest of the config pipeline.
pub fn effective_plugin_config(
    name: &str,
    lang_overrides: &BTreeMap<String, toml::Table>,
) -> toml::Table {
    let base_cfg = registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.config())
        .unwrap_or_default();

    // A `[plugins.<lang>]` block carries BOTH plugin-config keys (extensions,
    // detect_markers, node_attributes, …) and orchestrator sections
    // (ignore/rules/metrics/levels/report/principles). Only the former belong in the
    // plugin's effective config; the orchestrator sections are read separately via
    // `Config::language_config`, and some (`principles`) even have a conflicting
    // shape here (the plugin's own `[[principles]]` is an array, the project's
    // `[principles.<ID>]` a table), so merging them in would corrupt the plugin config.
    let plugin_keys = |block: &toml::Table| -> toml::Table {
        block
            .iter()
            .filter(|(k, _)| !crate::config::model::LANG_SECTION_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    let mut acc = base_cfg;
    for key in ["base", name] {
        if let Some(block) = lang_overrides.get(key) {
            acc = deep_merge(acc, plugin_keys(block));
        }
    }
    acc
}

/// All plugins whose `detect()` returns `true` under their effective config;
/// sorted alphabetically. Multiple matches are NORMAL (e.g. Rust + Markdown).
///
/// `eff_cfgs` maps each registered plugin name to its pre-built effective config
/// (call `effective_plugin_config` for each registered plugin beforehand).
pub fn detect_all(
    eff_cfgs: &BTreeMap<String, toml::Table>,
    workspace: &Path,
    input: &PluginInput,
) -> Vec<String> {
    let reg = registry();
    let mut found: Vec<String> = reg
        .iter()
        .filter(|p| {
            let cfg = eff_cfgs
                .get(p.name())
                .map(|t| t as &toml::Table)
                .unwrap_or(&EMPTY_TABLE);
            p.detect(cfg, workspace, input)
        })
        .map(|p| p.name().to_string())
        .collect();
    found.sort_unstable();
    found
}

/// An empty TOML table used as the fallback effective config when none is present.
static EMPTY_TABLE: std::sync::LazyLock<toml::Table> = std::sync::LazyLock::new(toml::Table::new);

/// Resolve the active plugins.
///
/// Precedence (low → high; each level fully REPLACES the one below it):
///   1. auto-detect (`detect_all`) — used only when neither config nor console pin the list.
///   2. config `plugins` — replaces auto-detect.
///   3. console `--plugins` (`arg`) — replaces config.
///
/// An empty `detect_all` result (no markers found) → `Err` with a zero-detect message.
pub fn resolve_plugins(
    arg: &[String],
    cfg_plugins: &[String],
    eff_cfgs: &BTreeMap<String, toml::Table>,
    workspace: &Path,
    input: &PluginInput,
    config_file: Option<&str>,
) -> Result<Vec<String>> {
    // Console wins outright.
    if !arg.is_empty() {
        return Ok(arg.to_vec());
    }
    // Config wins over auto-detect.
    if !cfg_plugins.is_empty() {
        return Ok(cfg_plugins.to_vec());
    }
    // Auto-detect: error on empty result.
    let detected = detect_all(eff_cfgs, workspace, input);
    if detected.is_empty() {
        let e = anyhow::anyhow!(
            "could not auto-detect any language in {}: no project markers found \
             (for C/C++ projects, no source files with the expected extensions were found)",
            workspace.display()
        );
        return Err(with_config_hint(e, config_file));
    }
    Ok(detected)
}

/// Startup guard: for the ACTIVE plugins, build a map `extension → [plugin names]`
/// from their effective configs; any extension claimed by more than one plugin is
/// an error (a file would be analysed twice, breaking the one-file-one-language
/// invariant).
pub fn validate_extension_uniqueness(
    active: &[String],
    eff_cfgs: &BTreeMap<String, toml::Table>,
) -> Result<()> {
    let mut ext_owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in active {
        let cfg = eff_cfgs
            .get(name)
            .map(|t| t as &toml::Table)
            .unwrap_or(&EMPTY_TABLE);
        // Read the `extensions` list from the effective config (same key the
        // plugins use in their own TOML).
        let extensions = toml_string_list(cfg, "extensions");
        for ext in extensions {
            ext_owners.entry(ext).or_default().push(name.clone());
        }
    }
    let conflicts: Vec<String> = ext_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(ext, owners)| format!(".{ext} claimed by: {}", owners.join(", ")))
        .collect();
    if !conflicts.is_empty() {
        bail!(
            "extension conflict between active plugins — a file would be analysed by multiple \
             languages (breaking the one-file-one-language invariant):\n  {}\n\
             Fix: adjust `extensions` in `[languages.<lang>]` or restrict `plugins = [\"...\"]`.",
            conflicts.join("\n  ")
        );
    }
    Ok(())
}

/// Augment a failed-detection error with how to pin the languages in config.
fn with_config_hint(e: anyhow::Error, config_file: Option<&str>) -> anyhow::Error {
    let how = match config_file {
        Some(path) => format!(
            "add `plugins = [\"<name>\"]` to {path} \
             (run `code-ranker docs` for a list of built-in plugins)"
        ),
        None => format!(
            "create a `code-ranker.toml` at the project root with:\n\
             \tversion = \"{CONFIG_VERSION}\"\n\
             \tplugins = [\"<name>\"]"
        ),
    };
    anyhow::anyhow!("{e}\n  → or pin the language in config: {how}")
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
