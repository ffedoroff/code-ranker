//! Tests for `cpp/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_cpp_source_presence() {
    let d = tempfile::tempdir().unwrap();
    let p = CppPlugin;
    assert!(!p.detect(d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("main.cpp"), "int main(){return 0;}\n").unwrap();
    assert!(p.detect(d.path(), &PluginInput::default()));
    assert_eq!(p.name(), "cpp");
}

#[test]
fn metrics_and_function_units_over_a_temp_project() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.cpp"),
        "// doc\nint add(int x, int y) { if (x > 0) { return x + y; } return y; }\n",
    )
    .unwrap();
    let p = CppPlugin;
    let g = p.analyze(d.path(), &PluginInput::default()).unwrap();
    assert!(!p.metrics(&g).is_empty());
    assert!(p.function_units(&g).iter().any(|(n, _)| n.name == "add"));
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
            n("/no/such/missing.cpp", code_ranker_plugin_api::node::FILE),
        ],
        edges: vec![],
    };
    assert!(CppPlugin.metrics(&g).is_empty());
    assert!(CppPlugin.function_units(&g).is_empty());
}
