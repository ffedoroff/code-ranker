//! Tests for `csharp/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_cs_source_presence() {
    let d = tempfile::tempdir().unwrap();
    let p = CsharpPlugin;
    assert!(!p.detect(d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("A.cs"), "class A {}\n").unwrap();
    assert!(p.detect(d.path(), &PluginInput::default()));
    assert_eq!(p.name(), "csharp");
}

#[test]
fn metrics_and_function_units_over_a_temp_project() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("A.cs"),
        "namespace N { class A { public int Add(int x, int y){ if (x>0){ return x+y; } return y; } } }\n",
    )
    .unwrap();
    let p = CsharpPlugin;
    let g = p
        .analyze(d.path(), "files", &PluginInput::default())
        .unwrap();
    assert!(!p.metrics(&g).is_empty());
    assert!(p.function_units(&g).iter().any(|(n, _)| n.name == "Add"));
    assert!(p.is_test_path("ATests.cs"));
}

#[test]
fn metrics_skip_non_file_and_unreadable_nodes() {
    // An EXTERNAL (non-FILE) node is dropped by the `kind != FILE` guard; a FILE
    // node whose path can't be read falls through `fs::read .. else continue`.
    // Neither yields a measurement.
    let n = |id: &str, kind: &str| Node {
        id: id.into(),
        kind: kind.into(),
        name: id.into(),
        parent: None,
        attrs: Default::default(),
    };
    let g = Graph {
        nodes: vec![
            n("ext:dep", code_ranker_plugin_api::node::EXTERNAL),
            n("/no/such/missing.cs", code_ranker_plugin_api::node::FILE),
        ],
        edges: vec![],
    };
    assert!(CsharpPlugin.metrics(&g).is_empty());
    assert!(CsharpPlugin.function_units(&g).is_empty());
}
