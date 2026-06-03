//! TypeScript language plugin for Code Split.
//!
//! Handles `.ts` and `.tsx` files via `tree-sitter-typescript`, reusing the
//! shared ECMAScript walker/resolver from `code-split-plugin-javascript`.

use anyhow::Result;
use code_split_plugin_api::{Graph, LanguagePlugin, Level, PluginInput};
use code_split_plugin_javascript::{analyze_ecmascript, detect_with_marker, ecmascript_level};
use std::path::Path;

/// The TypeScript language plugin (handles .ts / .tsx / .mts / .cts).
pub struct TypescriptPlugin;

const TS_EXTS: &[&str] = &["ts", "tsx", "mts", "cts"];

impl LanguagePlugin for TypescriptPlugin {
    fn name(&self) -> &str {
        "typescript"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        detect_with_marker(workspace, "tsconfig.json")
    }

    fn levels(&self) -> Vec<Level> {
        vec![ecmascript_level("files")]
    }

    fn analyze(&self, workspace: &Path, _level: &str, _input: &PluginInput) -> Result<Graph> {
        analyze_ecmascript(
            workspace,
            TS_EXTS,
            |ext| match ext {
                "ts" | "mts" | "cts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
                "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
                _ => None,
            },
            // Resolve imports TS-first, then JS fallbacks.
            &["ts", "tsx", "mts", "cts", "js", "jsx"],
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use code_split_plugin_api::LanguagePlugin;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &std::path::Path, rel: &str, contents: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }

    #[test]
    fn plugin_name_is_typescript() {
        assert_eq!(TypescriptPlugin.name(), "typescript");
    }

    #[test]
    fn detect_requires_tsconfig() {
        let tmp = TempDir::new().unwrap();
        let input = PluginInput::default();
        assert!(!TypescriptPlugin.detect(tmp.path(), &input));
        fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();
        assert!(TypescriptPlugin.detect(tmp.path(), &input));
    }

    #[test]
    fn levels_returns_single_files_level() {
        let levels = TypescriptPlugin.levels();
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].name, "files");
        assert!(levels[0].edge_kinds.contains_key("uses"));
    }

    #[test]
    fn analyze_builds_ts_graph_with_imports_and_externals() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        write_file(
            root,
            "src/a.ts",
            "import { greet } from \"./b\";\n\
             import React from \"react\";\n\
             export function helper() { return greet(); }\n",
        );
        write_file(
            root,
            "src/b.ts",
            "export function greet(): string { return \"hi\"; }\n",
        );

        let input = PluginInput::default();
        let graph = TypescriptPlugin
            .analyze(root, "files", &input)
            .expect("TypescriptPlugin.analyze should succeed");

        let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
        let b_id = root.join("src/b.ts").to_string_lossy().into_owned();

        assert!(
            graph.nodes.iter().any(|n| n.id == a_id && n.kind == "file"),
            "a.ts node present"
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses"),
            "expected import edge a.ts → b.ts"
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| n.id == "ext:react" && n.kind == "external"),
            "external node for react"
        );
    }
}
