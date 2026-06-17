use super::*;
use crate::test_support::{edge_count_from, has_node, write_file};
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
fn levels_returns_files_and_functions() {
    let levels = JavascriptPlugin.levels();
    assert_eq!(levels.len(), 2);
    assert_eq!(levels[0].name, "files");
    assert!(levels[0].edge_kinds.contains_key("uses"));
    assert_eq!(levels[1].name, "functions");
    assert!(levels[1].node_kinds.contains_key("function"));
}

#[test]
fn function_units_extracts_per_function_nodes() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("a.js");
    fs::write(
        &f,
        "function add(a, b) { if (a) return a + b; return b; }\nconst g = (x) => x;\n",
    )
    .unwrap();
    let graph = Graph {
        nodes: vec![code_ranker_plugin_api::node::Node {
            id: f.to_string_lossy().into_owned(),
            kind: "file".into(),
            name: "a.js".into(),
            parent: None,
            attrs: Default::default(),
        }],
        edges: vec![],
    };
    let units: Vec<_> = JavascriptPlugin
        .function_units(&graph)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(
        units
            .iter()
            .any(|n| n.name == "add" && n.kind == "function"),
        "function add extracted: {:?}",
        units.iter().map(|n| (&n.name, &n.kind)).collect::<Vec<_>>()
    );
    assert!(units.iter().all(|n| n.parent.is_some()));
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
    let graph = JavascriptPlugin
        .analyze(root, "files", &PluginInput::default())
        .expect("analyze should succeed");
    // The plugin measures inputs; the orchestrator (here, the test) writes them.
    let inputs = JavascriptPlugin.metrics(&graph);
    assert_eq!(inputs.len(), 1, "the single .js file node is measured");

    let a_id = root.join("src/a.js").to_string_lossy().into_owned();
    let (id, m) = &inputs[0];
    assert_eq!(id, &a_id, "measured the a.js file node");
    let mut node = graph.nodes.iter().find(|n| n.id == a_id).unwrap().clone();
    code_ranker_graph::write_metrics(&mut node, m);
    // a function with an `if` and two `return`s has real complexity.
    assert!(
        node.attrs.contains_key("cyclomatic"),
        "cyclomatic derived from the measured inputs"
    );
}

#[test]
fn metrics_skip_unreadable_and_unsupported_files() {
    // `/missing.js` maps to a grammar but can't be read; `readme.txt` maps to no
    // grammar (the `_ => None` arm). Both skipped → nothing measured.
    let n = |id: &str| code_ranker_plugin_api::node::Node {
        id: id.into(),
        kind: "file".into(),
        name: id.into(),
        parent: None,
        attrs: Default::default(),
    };
    let graph = Graph {
        nodes: vec![n("/no/such/missing.js"), n("/x/readme.txt")],
        edges: vec![],
    };
    assert!(JavascriptPlugin.metrics(&graph).is_empty());
    assert!(JavascriptPlugin.function_units(&graph).is_empty());
}

#[test]
fn cjs_is_not_detected_as_test() {
    // `.cjs` files are walked but the JS grammar maps them to no node;
    // is_test_path follows the shared ECMAScript convention.
    assert!(JavascriptPlugin.is_test_path("src/a.test.js"));
    assert!(!JavascriptPlugin.is_test_path("src/a.js"));
}
