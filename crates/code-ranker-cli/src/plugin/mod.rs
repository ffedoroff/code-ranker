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
use code_ranker_graph::write_metrics;
use code_ranker_plugin_api::{
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
    principle::Principle,
};
use std::collections::BTreeMap;
use std::path::Path;

mod resolve;
pub use resolve::{
    detect_all, effective_plugin_config, names_with_aliases, resolve_plugins, to_canonical,
    validate_extension_uniqueness,
};

/// Every self-registered language plugin (see `code_ranker_plugin_api::registry`).
/// The CLI links the `code-ranker-plugins` crate (its `deep_merge` / `list_override`
/// are used elsewhere), so every plugin's `inventory::submit!` is collected here.
pub fn registry() -> Vec<&'static dyn LanguagePlugin> {
    code_ranker_plugin_api::registry()
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
        None => bail!(
            "unknown plugin {name:?}; built-in languages are: {}",
            names_with_aliases()
        ),
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

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
