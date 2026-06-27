//! JavaScript language plugin for Code Ranker.
//!
//! A thin adapter over the shared, grammar-agnostic engine in the
//! [`crate::languages::ecmascript`] module: it binds the `tree-sitter-javascript` grammar
//! to `.js` / `.jsx` / `.mjs` / `.cjs` and detects projects by `package.json`.
//! It builds on the shared engine as a peer — never on the TypeScript plugin (and
//! vice-versa).

use crate::languages::ecmascript::{
    analyze_ecmascript, ecmascript_function_units, ecmascript_functions_level, ecmascript_level,
    ecmascript_metric_specs, ecmascript_metrics,
};
use anyhow::Result;
use code_ranker_plugin_api::{
    Principle, detect_with_marker,
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

/// The JavaScript config: the inheritance chain `defaults.toml ⊕
/// ecmascript/config.toml ⊕ javascript/config.toml`. The ECMAScript base supplies
/// the shared engine vocab (`[roles]`/`[halstead]`/`[loc]`, the `arrow`/`generator`
/// node kinds, Halstead spec overrides); `javascript/config.toml` adds only what is
/// JS-specific (extensions, `detect_markers`, `doc_lang`).
static CONFIG: LazyLock<toml::Table> = LazyLock::new(|| {
    crate::config::load_chain(&[
        include_str!("../ecmascript/config.toml"),
        include_str!("config.toml"),
    ])
});

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&JavascriptPlugin)
}

/// The JavaScript language plugin (handles .js / .jsx / .mjs / .cjs).
pub struct JavascriptPlugin;

impl LanguagePlugin for JavascriptPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "javascript"
    }

    fn detect(&self, cfg: &toml::Table, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the walk/detect logic stays in Rust). JS detects on
        // `package.json`.
        crate::config::string_list(cfg, "detect_markers")
            .iter()
            .any(|m| detect_with_marker(workspace, m))
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        vec![
            ecmascript_level("files", cfg),
            ecmascript_functions_level(cfg),
        ]
    }

    fn analyze(&self, cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        // File-collection extensions / import-resolution order are DATA: read
        // from `config.toml`'s `extensions` (a JS-only project collects and
        // resolves against the same list). Every JavaScript extension uses the one
        // `tree-sitter-javascript` grammar, so the grammar selector is a constant
        // (a grammar TYPE can't be config) — the `extensions` list alone gates
        // which files reach it, so no extension is enumerated here.
        let exts = crate::config::string_list(cfg, "extensions");
        let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
        analyze_ecmascript(
            workspace,
            &exts,
            |_ext| Some(tree_sitter_javascript::LANGUAGE.into()),
            &exts,
            input.ignore_tests,
            &crate::walk::ignore_from(input),
        )
    }

    fn metrics(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(String, MetricInputs)> {
        ecmascript_metrics(graph, |_ext| {
            Some((tree_sitter_javascript::LANGUAGE.into(), false))
        })
    }

    fn function_units(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        ecmascript_function_units(graph, |_ext| {
            Some((tree_sitter_javascript::LANGUAGE.into(), false))
        })
    }

    fn principles(&self, cfg: &toml::Table, _input: &PluginInput) -> Vec<Principle> {
        // The common catalog from `defaults.toml`, with `doc_url` resolved to
        // `{doc_base}/typescript/<slug>.md` (JS shares the TS principle corpus).
        crate::config::resolved_principles(cfg)
    }

    fn report_overrides(
        &self,
        cfg: &toml::Table,
    ) -> code_ranker_plugin_api::report::ReportOverride {
        code_ranker_plugin_api::list_override::report_override(cfg)
    }

    fn metric_specs(
        &self,
        cfg: &toml::Table,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        // Shared ECMAScript Halstead operator/operand descriptions (JS and TS use
        // the same `[halstead]` vocab → one home in `ecmascript/config.toml`).
        ecmascript_metric_specs(defaults, cfg)
    }
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
