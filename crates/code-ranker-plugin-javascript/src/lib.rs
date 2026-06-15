//! JavaScript language plugin for Code Ranker.
//!
//! A thin adapter over the shared, grammar-agnostic engine in
//! `code-ranker-ecmascript-core`: it binds the `tree-sitter-javascript` grammar
//! to `.js` / `.jsx` / `.mjs` / `.cjs` and detects projects by `package.json`.
//! It depends on the shared core as a peer — never on the TypeScript plugin (and
//! vice-versa).

use anyhow::Result;
use code_ranker_ecmascript_core::{
    analyze_ecmascript, annotate_ecmascript_metrics, ecmascript_is_test_path, ecmascript_level,
};
use code_ranker_plugin_api::{
    graph::Graph,
    level::Level,
    plugin::{LanguagePlugin, PluginInput, detect_with_marker},
};
use std::path::Path;

/// The JavaScript language plugin (handles .js / .jsx / .mjs / .cjs).
pub struct JavascriptPlugin;

const JS_EXTS: &[&str] = &["js", "jsx", "mjs", "cjs"];

impl LanguagePlugin for JavascriptPlugin {
    fn name(&self) -> &str {
        "javascript"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        detect_with_marker(workspace, "package.json")
    }

    fn levels(&self) -> Vec<Level> {
        vec![ecmascript_level("files")]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        analyze_ecmascript(
            workspace,
            JS_EXTS,
            |ext| match ext {
                "js" | "jsx" | "mjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
                _ => None,
            },
            &["js", "jsx", "mjs", "cjs"],
            input.ignore_tests,
        )
    }

    fn metrics(&self, graph: &mut Graph) -> usize {
        annotate_ecmascript_metrics(graph, |ext| match ext {
            "js" | "jsx" | "mjs" | "cjs" => Some((tree_sitter_javascript::LANGUAGE.into(), false)),
            _ => None,
        })
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        ecmascript_is_test_path(rel_path)
    }
}
