//! TypeScript language plugin for Code Ranker.
//!
//! Handles `.ts` and `.tsx` files via `tree-sitter-typescript`, reusing the
//! shared ECMAScript walker/resolver from `code-ranker-ecmascript-core`. It
//! depends on that shared core as a peer — never on the JavaScript plugin.

use anyhow::Result;
use code_ranker_ecmascript_core::{
    analyze_ecmascript, annotate_ecmascript_metrics, ecmascript_function_units,
    ecmascript_functions_level, ecmascript_is_test_path, ecmascript_level,
};
use code_ranker_plugin_api::{
    graph::Graph,
    level::Level,
    plugin::{LanguagePlugin, PluginInput, detect_with_marker},
};
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
        vec![ecmascript_level("files"), ecmascript_functions_level()]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
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
            input.ignore_tests,
        )
    }

    fn metrics(&self, graph: &mut Graph) -> usize {
        // `else_if_via_else_clause` is true for TypeScript proper, false for TSX
        // (the only per-dialect difference in the cognitive `else-if` rule).
        annotate_ecmascript_metrics(graph, |ext| match ext {
            "ts" | "mts" | "cts" => {
                Some((tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), true))
            }
            "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), false)),
            _ => None,
        })
    }

    fn function_units(&self, graph: &Graph) -> Vec<code_ranker_plugin_api::node::Node> {
        ecmascript_function_units(graph, |ext| match ext {
            "ts" | "mts" | "cts" => {
                Some((tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), true))
            }
            "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), false)),
            _ => None,
        })
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        ecmascript_is_test_path(rel_path)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_plugin_api::plugin::LanguagePlugin;
    use code_ranker_test_support::{edge_count_from, write_file};
    use std::fs;
    use tempfile::TempDir;

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
    fn levels_returns_files_and_functions() {
        let levels = TypescriptPlugin.levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].name, "files");
        assert!(levels[0].edge_kinds.contains_key("uses"));
        assert_eq!(levels[1].name, "functions");
        assert!(levels[1].node_kinds.contains_key("function"));
    }

    #[test]
    fn function_units_extracts_per_function_nodes() {
        let tmp = TempDir::new().unwrap();
        let ts = tmp.path().join("a.ts");
        fs::write(
            &ts,
            "function add(a: number, b: number): number { if (a) return a + b; return b; }\nclass C { m(x: number) { return x; } }\n",
        )
        .unwrap();
        // a `.tsx` file too, to exercise the TSX grammar arm.
        let tsx = tmp.path().join("w.tsx");
        fs::write(&tsx, "function widget(p: number) { return p; }\n").unwrap();
        let node = |p: &std::path::Path, name: &str| code_ranker_plugin_api::node::Node {
            id: p.to_string_lossy().into_owned(),
            kind: "file".into(),
            name: name.into(),
            parent: None,
            attrs: Default::default(),
        };
        let graph = Graph {
            nodes: vec![node(&ts, "a.ts"), node(&tsx, "w.tsx")],
            edges: vec![],
        };
        let units = TypescriptPlugin.function_units(&graph);
        assert!(
            units
                .iter()
                .any(|n| n.name == "add" && n.kind == "function")
        );
        assert!(units.iter().any(|n| n.name == "m" && n.kind == "method"));
        assert!(units.iter().any(|n| n.name == "widget"), "tsx function");
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

    #[test]
    fn import_path_in_comment_or_string_is_not_an_edge() {
        // Layer-1 metamorphic FP guard (docs/metric-correctness.md): a module path
        // appearing only in a comment, a string, or a template literal must NOT
        // create a dependency edge — imports come from AST nodes, not text.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_file(
            root,
            "src/a.ts",
            "// import { greet } from \"./b\";\n\
             const note = \"import { greet } from './b'\";\n\
             const tpl = `import './b'`;\n\
             void note;\n\
             void tpl;\n\
             export function helper(): number { return 1; }\n",
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
            !graph
                .edges
                .iter()
                .any(|e| e.source == a_id && e.target == b_id),
            "a path in a comment/string/template must not produce an edge"
        );
    }

    /// Build a 2-file TS/TSX project (`a` importing from `./b`) and return the
    /// number of `uses` edges leaving `a`. Centralizes the per-form scaffold.
    fn uses_edges_from_a(a_rel: &str, a_src: &str) -> usize {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_file(root, a_rel, a_src);
        write_file(root, "src/b.ts", "export const g: number = 1;\n");
        let g = TypescriptPlugin
            .analyze(root, "files", &PluginInput::default())
            .expect("analyze should succeed");
        let a_id = root.join(a_rel).to_string_lossy().into_owned();
        edge_count_from(&g, &a_id, "uses")
    }

    #[test]
    fn ts_static_import_forms_produce_edges() {
        // Every static module-specifier form resolves to exactly one dependency
        // edge — they are all `import_statement` nodes carrying a `from "./b"`
        // string, so the type-only / namespace / alias sugar is transparent to
        // the path-based resolver. `require("./b")` is also recognized.
        let forms: &[(&str, &str)] = &[
            (
                "import type",
                "import type { T } from \"./b\";\nexport const x = 1;\n",
            ),
            (
                "namespace",
                "import * as ns from \"./b\";\nvoid ns;\nexport const x = 1;\n",
            ),
            (
                "aliased named",
                "import { g as h } from \"./b\";\nvoid h;\nexport const x = 1;\n",
            ),
            (
                "default",
                "import b from \"./b\";\nvoid b;\nexport const x = 1;\n",
            ),
            ("re-export", "export { g } from \"./b\";\n"),
            (
                "require",
                "const b = require(\"./b\");\nvoid b;\nexport const x = 1;\n",
            ),
        ];
        let mut fails = Vec::new();
        for (label, src) in forms {
            let n = uses_edges_from_a("src/a.ts", src);
            if n != 1 {
                fails.push(format!("{label}: expected 1 edge, got {n}"));
            }
        }
        assert!(
            fails.is_empty(),
            "import forms not resolved:\n{}",
            fails.join("\n")
        );
    }

    #[test]
    fn tsx_file_is_analyzed() {
        // The `.tsx` branch (LANGUAGE_TSX) is parsed like `.ts`: a real import in
        // a .tsx file yields a dependency edge. Guards the tsx arm of `analyze`.
        let n = uses_edges_from_a(
            "src/a.tsx",
            "import { g } from \"./b\";\nvoid g;\nexport const x = 1;\n",
        );
        assert_eq!(n, 1, "a real import in a .tsx file must produce an edge");
    }

    #[test]
    fn dynamic_import_and_import_equals_are_non_goals() {
        // Documented limitations (see principles/typescript/metrics.md): a
        // *dynamic* `import("./b")` and the TS `import b = require("./b")` form
        // are NOT resolved into edges — the specifier sits inside a call the
        // import walker does not descend into. Purely syntactic scope, like
        // un-expanded macros for Rust. Pinned so a future change is deliberate.
        assert_eq!(
            uses_edges_from_a(
                "src/a.ts",
                "export async function f() { return import(\"./b\"); }\n"
            ),
            0,
            "dynamic import() is a documented non-goal"
        );
        assert_eq!(
            uses_edges_from_a(
                "src/a.ts",
                "import b = require(\"./b\");\nvoid b;\nexport const x = 1;\n"
            ),
            0,
            "import = require() is a documented non-goal"
        );
    }

    #[test]
    fn edges_scale_with_real_imports() {
        // Layer-2 generative (docs/metric-correctness.md): `a` really imports `n`
        // files; the edge count from `a` must equal `n`, swept over a grid.
        for n in 0..5 {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            let mut a = String::new();
            for i in 0..n {
                a.push_str(&format!("import {{ x{i} }} from \"./b{i}\";\n"));
                write_file(
                    root,
                    &format!("src/b{i}.ts"),
                    &format!("export const x{i}: number = {i};\n"),
                );
            }
            a.push_str("export const y = 1;\n");
            write_file(root, "src/a.ts", &a);

            let graph = TypescriptPlugin
                .analyze(root, "files", &PluginInput::default())
                .expect("TypescriptPlugin.analyze should succeed");

            let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
            let got = graph
                .edges
                .iter()
                .filter(|e| e.source == a_id && e.kind == "uses")
                .count();
            assert_eq!(
                got, n,
                "expected exactly {n} import edges from a.ts, got {got}"
            );
        }
    }
}
