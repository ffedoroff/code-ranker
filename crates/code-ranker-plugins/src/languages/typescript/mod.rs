//! TypeScript language plugin for Code Ranker.
//!
//! Handles `.ts` and `.tsx` files via `tree-sitter-typescript`, reusing the
//! shared ECMAScript walker/resolver in the [`crate::languages::ecmascript`] module. It
//! builds on that shared engine as a peer — never on the JavaScript plugin.

use crate::languages::ecmascript::{
    analyze_ecmascript, ecmascript_function_units, ecmascript_functions_level, ecmascript_level,
    ecmascript_metric_specs, ecmascript_metrics,
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

/// The TypeScript config: the inheritance chain `defaults.toml ⊕
/// ecmascript/config.toml ⊕ typescript/config.toml`. The ECMAScript base supplies
/// the shared engine vocab (`[roles]`/`[halstead]`/`[loc]`, the `arrow`/`generator`
/// node kinds, Halstead spec overrides); `typescript/config.toml` adds only what is
/// TS-specific (extensions, `resolution_order`, `detect_markers`, `doc_lang`).
static CONFIG: LazyLock<toml::Table> = LazyLock::new(|| {
    crate::config::load_chain(&[
        include_str!("../ecmascript/config.toml"),
        include_str!("config.toml"),
    ])
});

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::plugin::PluginRegistration(&TypescriptPlugin)
}

/// The TypeScript language plugin (handles .ts / .tsx / .mts / .cts).
pub struct TypescriptPlugin;

impl LanguagePlugin for TypescriptPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "typescript"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the detect logic stays in Rust). TS detects on
        // `tsconfig.json`.
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

    fn analyze(&self, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        // File-collection extensions and the TS-first import-resolution order are
        // DATA: read from `config.toml`'s `extensions` / `resolution_order`. The
        // grammar selector ([`grammar_for`]) stays in Rust (string → grammar TYPE).
        let exts = crate::config::string_list(&CONFIG, "extensions");
        let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
        let order = crate::config::string_list(&CONFIG, "resolution_order");
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        analyze_ecmascript(
            workspace,
            &exts,
            |ext| grammar_for(ext).map(|(lang, _)| lang),
            &order,
            input.ignore_tests,
            &crate::walk::ignore_from(input),
        )
    }

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
        ecmascript_metrics(graph, grammar_for)
    }

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        ecmascript_function_units(graph, grammar_for)
    }

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        // The common catalog from `defaults.toml`, with `doc_url` resolved to
        // `{doc_base}/typescript/<slug>.md` (TypeScript adds no presets of its own).
        crate::config::resolved_presets(&CONFIG)
    }

    fn report_overrides(&self) -> code_ranker_plugin_api::report::ReportOverride {
        code_ranker_plugin_api::list_override::report_override(&CONFIG)
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

/// Map a TypeScript file extension to its `tree-sitter` grammar and the
/// `else_if_via_else_clause` cognitive flag (the only per-dialect difference in
/// the `else-if` rule). `.tsx` uses the TSX grammar (JSX syntax) with the flag
/// off; `.ts` / `.mts` / `.cts` use plain TypeScript with it on. The binding
/// stays in Rust because it selects a grammar TYPE (only the `extensions` *list*
/// is config); `tsx` is the sole extension literal — it alone picks a different
/// grammar. The shared engine only ever calls this for a collected `extensions`
/// file, so the `_` arm is the TypeScript-proper default.
fn grammar_for(ext: &str) -> Option<(tree_sitter::Language, bool)> {
    match ext {
        "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), false)),
        _ => Some((tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), true)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
