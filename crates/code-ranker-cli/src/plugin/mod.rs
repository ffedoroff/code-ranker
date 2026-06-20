//! Thin CLI-side accessors over the plugin registry. The CLI NEVER names a
//! concrete language: plugins self-register via `inventory::submit!` in the
//! `code-ranker-plugins` crate and are collected by `code_ranker_plugin_api::registry`.
//! Everything here works only through the `LanguagePlugin` trait and the plugin's
//! `name()`. Adding a language is a self-contained module in the plugins crate.

use anyhow::{Result, bail};
use code_ranker_graph::write_metrics;
use code_ranker_plugin_api::{
    graph::Graph,
    level::{AttributeSpec, Level, Thresholds},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
    preset::Preset,
};
use std::collections::BTreeMap;
use std::path::Path;

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
pub fn analyze(name: &str, workspace: &Path, input: &PluginInput) -> Result<(Graph, Vec<Level>)> {
    let reg = registry();
    match reg.iter().find(|p| p.name() == name) {
        Some(p) => {
            let graph = p.analyze(workspace, input)?;
            Ok((graph, p.levels()))
        }
        None => bail!("unknown plugin {name:?}; built-in plugins are: {}", names()),
    }
}

/// Have the matching plugin **measure** its per-language complexity inputs, then
/// write every metric (tier-1 + the tier-2 registry derivations) onto the graph's
/// file nodes here, in the orchestrator. Returns the number of nodes annotated.
/// Measuring is a per-language concern owned by the plugin (no central
/// by-extension dispatcher); enrichment (`write_metrics`, which needs the metric
/// catalog) is central — so a plugin never depends on `code-ranker-graph`.
pub fn annotate_metrics(name: &str, graph: &mut Graph) -> usize {
    let reg = registry();
    let Some(p) = reg.iter().find(|p| p.name() == name) else {
        return 0;
    };
    let by_id: BTreeMap<String, MetricInputs> = p.metrics(graph).into_iter().collect();
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
pub fn function_units(name: &str, graph: &Graph) -> Vec<Node> {
    let reg = registry();
    let Some(p) = reg.iter().find(|p| p.name() == name) else {
        return Vec::new();
    };
    p.function_units(graph)
        .into_iter()
        .map(|(mut node, inputs)| {
            write_metrics(&mut node, &inputs);
            node
        })
        .collect()
}

/// Tool/toolchain versions the matching plugin wants recorded in the snapshot.
pub fn versions(name: &str, workspace: &Path, input: &PluginInput) -> Vec<(String, String)> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.versions(workspace, input))
        .unwrap_or_default()
}

/// Named external-path roots the matching plugin contributes for shortening node
/// ids (e.g. Rust's `cargo` / `registry` / `rustup` / `rust-src`). Language
/// knowledge lives in the plugin; the orchestrator only adds the generic
/// `target` root on top.
pub fn roots(name: &str, workspace: &Path) -> Vec<(String, String)> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.roots(workspace))
        .unwrap_or_default()
}

/// Language-calibrated per-metric thresholds from the matching plugin.
pub fn thresholds(name: &str) -> BTreeMap<String, Thresholds> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.thresholds())
        .unwrap_or_default()
}

/// The matching plugin's report-list overrides (table `columns` / card / JSON
/// `stats`), applied by the orchestrator over the global catalog lists.
pub fn report_overrides(name: &str) -> code_ranker_plugin_api::report::ReportOverride {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.report_overrides())
        .unwrap_or_default()
}

/// The matching plugin's Prompt-Generator presets (the common catalog plus any
/// language-specific presets), built from its own config.
pub fn presets(name: &str, input: &PluginInput) -> Vec<Preset> {
    match registry().iter().find(|p| p.name() == name) {
        Some(p) => p.presets(input),
        None => Vec::new(),
    }
}

/// Let the matching plugin refine the language-neutral default metric specs
/// (e.g. add Rust-specific `#[cfg(test)]` nuance to LOC descriptions). The
/// neutral catalog comes from `code-ranker-graph`; the plugin overrides only
/// what differs for its language.
pub fn metric_specs(
    name: &str,
    defaults: BTreeMap<String, AttributeSpec>,
) -> BTreeMap<String, AttributeSpec> {
    match registry().iter().find(|p| p.name() == name) {
        Some(p) => p.metric_specs(defaults),
        None => defaults,
    }
}

/// Auto-detect the plugin from workspace markers. Errors if none or more than
/// one matches.
pub fn detect(workspace: &Path, input: &PluginInput) -> Result<String> {
    let reg = registry();
    let found: Vec<&str> = reg
        .iter()
        .filter(|p| p.detect(workspace, input))
        .map(|p| p.name())
        .collect();
    match found.as_slice() {
        [one] => Ok((*one).to_string()),
        [] => bail!(
            "could not auto-detect a plugin in {}: no project marker found — pass --plugin {}",
            workspace.display(),
            names()
        ),
        _ => bail!(
            "ambiguous project in {}: markers for multiple plugins found ({}) — pass --plugin to choose",
            workspace.display(),
            found.join(", ")
        ),
    }
}

/// Resolve the plugin name: explicit `--plugin` > config `plugin` > auto-detect.
/// A value of `auto` (or absence) triggers project-marker detection. Lives here,
/// with the registry and [`detect`], so plugin selection is one concern.
pub fn resolve_plugin(arg: Option<&str>, cfg: Option<&str>, workspace: &Path) -> Result<String> {
    if let Some(p) = arg
        && p != "auto"
    {
        return Ok(p.to_string());
    }
    if let Some(p) = cfg
        && p != "auto"
    {
        return Ok(p.to_string());
    }
    detect(workspace, &PluginInput::default())
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
