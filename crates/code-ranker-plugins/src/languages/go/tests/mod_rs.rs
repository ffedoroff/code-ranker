//! Tests for `go/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_go_mod() {
    let d = tempfile::tempdir().unwrap();
    let p = GoPlugin;
    assert!(!p.detect(d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("go.mod"), "module m\n").unwrap();
    assert!(p.detect(d.path(), &PluginInput::default()));
}

#[test]
fn name_and_levels() {
    let p = GoPlugin;
    assert_eq!(p.name(), "go");
    let levels = p.levels();
    assert!(levels.iter().any(|l| l.name == "files"));
    assert!(levels.iter().any(|l| l.name == "functions"));
}

#[test]
fn metrics_and_function_units_over_a_temp_project() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("go.mod"), "module m\n").unwrap();
    std::fs::write(
        d.path().join("a.go"),
        "package m\n\n// A doubles positive inputs.\nfunc A(x int) int {\n\tif x > 0 {\n\t\treturn x * 2\n\t}\n\treturn 0\n}\n",
    )
    .unwrap();

    let p = GoPlugin;
    let g = p
        .analyze(d.path(), "files", &PluginInput::default())
        .unwrap();
    assert!(!p.metrics(&g).is_empty(), "file metrics produced");
    let units = p.function_units(&g);
    assert!(units.iter().any(|(n, _)| n.name == "A"), "function unit A");
    assert!(p.is_test_path("a_test.go"));
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
            n("/no/such/missing.go", code_ranker_plugin_api::node::FILE),
        ],
        edges: vec![],
    };
    assert!(GoPlugin.metrics(&g).is_empty());
    assert!(GoPlugin.function_units(&g).is_empty());
}
