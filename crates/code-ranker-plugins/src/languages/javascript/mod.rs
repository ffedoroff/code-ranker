//! JavaScript language plugin for Code Ranker.
//!
//! A thin adapter over the shared, grammar-agnostic engine in the
//! [`crate::languages::ecmascript`] module: it binds the `tree-sitter-javascript` grammar
//! to `.js` / `.jsx` / `.mjs` / `.cjs` and detects projects by `package.json`.
//! It builds on the shared engine as a peer — never on the TypeScript plugin (and
//! vice-versa).

use crate::languages::ecmascript::{
    analyze_ecmascript, ecmascript_function_units, ecmascript_functions_level,
    ecmascript_is_test_path, ecmascript_level, ecmascript_metric_specs, ecmascript_metrics,
};
use anyhow::Result;
use code_ranker_plugin_api::{
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput, Preset, detect_with_marker},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

/// The JavaScript config: `javascript.toml` deep-merged over the shared
/// `defaults.toml`, used to build the preset list (the common catalog +
/// JavaScript's `doc_lang`, which maps to the TypeScript principle corpus).
static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

/// The JavaScript language plugin (handles .js / .jsx / .mjs / .cjs).
pub struct JavascriptPlugin;

impl LanguagePlugin for JavascriptPlugin {
    fn name(&self) -> &str {
        "javascript"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the walk/detect logic stays in Rust). JS detects on
        // `package.json`.
        crate::config::string_list(&CONFIG, "detect_markers")
            .iter()
            .any(|m| detect_with_marker(workspace, m))
    }

    fn levels(&self) -> Vec<Level> {
        vec![
            ecmascript_level("files", &CONFIG),
            ecmascript_functions_level(&CONFIG),
        ]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        // File-collection extensions / import-resolution order are DATA: read
        // from `config.toml`'s `extensions` (a JS-only project collects and
        // resolves against the same list). Every JavaScript extension uses the one
        // `tree-sitter-javascript` grammar, so the grammar selector is a constant
        // (a grammar TYPE can't be config) — the `extensions` list alone gates
        // which files reach it, so no extension is enumerated here.
        let exts = crate::config::string_list(&CONFIG, "extensions");
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

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
        ecmascript_metrics(graph, |_ext| {
            Some((tree_sitter_javascript::LANGUAGE.into(), false))
        })
    }

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        ecmascript_function_units(graph, |_ext| {
            Some((tree_sitter_javascript::LANGUAGE.into(), false))
        })
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        ecmascript_is_test_path(rel_path)
    }

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        // The common catalog from `defaults.toml`, with `doc_url` resolved to
        // `{doc_base}/typescript/<slug>.md` (JS shares the TS principle corpus).
        crate::config::resolved_presets(&CONFIG)
    }

    fn report_overrides(&self) -> code_ranker_plugin_api::report::ReportOverride {
        crate::list_override::report_override(&CONFIG)
    }

    fn metric_specs(
        &self,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        // Shared ECMAScript Halstead operator/operand descriptions (JS and TS use
        // the same `[halstead]` vocab → one home in `ecmascript/config.toml`).
        ecmascript_metric_specs(defaults)
    }
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
