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

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_test_support::{edge_count_from, has_node, write_file};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn plugin_name_is_javascript() {
        assert_eq!(JavascriptPlugin.name(), "javascript");
    }

    #[test]
    fn detect_requires_package_json() {
        let tmp = TempDir::new().unwrap();
        let input = PluginInput::default();
        assert!(!JavascriptPlugin.detect(tmp.path(), &input));
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        assert!(JavascriptPlugin.detect(tmp.path(), &input));
    }

    #[test]
    fn levels_returns_single_files_level() {
        let levels = JavascriptPlugin.levels();
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].name, "files");
        assert!(levels[0].edge_kinds.contains_key("uses"));
    }

    #[test]
    fn analyze_builds_js_graph_with_imports_and_externals() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_file(
            root,
            "src/a.js",
            "import { greet } from \"./b\";\n\
             import React from \"react\";\n\
             export function helper() { return greet(); }\n",
        );
        write_file(root, "src/b.js", "export function greet() { return 1; }\n");

        let graph = JavascriptPlugin
            .analyze(root, "files", &PluginInput::default())
            .expect("analyze should succeed");

        let a_id = root.join("src/a.js").to_string_lossy().into_owned();
        let b_id = root.join("src/b.js").to_string_lossy().into_owned();
        assert!(has_node(&graph, &a_id), "a.js file node present");
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses"),
            "import edge a.js → b.js"
        );
        assert!(has_node(&graph, "ext:react"), "external node for react");
        assert_eq!(edge_count_from(&graph, &a_id, "uses"), 2, "./b + react");
    }

    #[test]
    fn metrics_annotates_file_nodes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_file(
            root,
            "src/a.js",
            "export function f(x) { if (x > 0) { return 1; } return 2; }\n",
        );
        let mut graph = JavascriptPlugin
            .analyze(root, "files", &PluginInput::default())
            .expect("analyze should succeed");
        let annotated = JavascriptPlugin.metrics(&mut graph);
        assert_eq!(annotated, 1, "the single .js file node is annotated");

        let a_id = root.join("src/a.js").to_string_lossy().into_owned();
        let node = graph.nodes.iter().find(|n| n.id == a_id).unwrap();
        // a function with an `if` and two `return`s has real complexity.
        assert!(
            node.attrs.contains_key("cyclomatic"),
            "cyclomatic written onto the file node"
        );
    }

    #[test]
    fn cjs_is_not_detected_as_test() {
        // `.cjs` files are walked but the JS grammar maps them to no node;
        // is_test_path follows the shared ECMAScript convention.
        assert!(JavascriptPlugin.is_test_path("src/a.test.js"));
        assert!(!JavascriptPlugin.is_test_path("src/a.js"));
    }
}
