//! Tests for `python/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn metrics_and_function_units_skip_unreadable_files() {
    // A file node whose path does not exist is silently skipped by both passes
    // (the `fs::read(..) else continue` arms) — no panic, no output.
    let graph = code_ranker_plugin_api::graph::Graph {
        nodes: vec![code_ranker_plugin_api::node::Node {
            id: "/no/such/dir/missing.py".into(),
            kind: "file".into(),
            name: "missing.py".into(),
            parent: None,
            attrs: Default::default(),
        }],
        edges: vec![],
    };
    assert!(PythonPlugin.metrics(&graph).is_empty());
    assert!(PythonPlugin.function_units(&graph).is_empty());
}
