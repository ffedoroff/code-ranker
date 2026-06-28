//! Tests for `c/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_c_source_presence() {
    let d = tempfile::tempdir().unwrap();
    let p = CPlugin;
    let cfg = p.config();
    assert!(!p.detect(&cfg, d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("main.c"), "int main(){return 0;}\n").unwrap();
    assert!(p.detect(&cfg, d.path(), &PluginInput::default()));
}

#[test]
fn metrics_and_function_units_over_a_temp_project() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.c"),
        "// doc\nint add(int x, int y) { if (x > 0) { return x + y; } return y; }\n",
    )
    .unwrap();
    let p = CPlugin;
    let cfg = p.config();
    let g = p.analyze(&cfg, d.path(), &PluginInput::default()).unwrap();
    assert!(!p.metrics(&cfg, &g).is_empty(), "file metrics produced");
    assert!(
        p.function_units(&cfg, &g)
            .iter()
            .any(|(n, _)| n.name == "add")
    );
    assert_eq!(p.name(), "c");
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
            n("/no/such/missing.c", code_ranker_plugin_api::node::FILE),
        ],
        edges: vec![],
    };
    let cfg = CPlugin.config();
    assert!(CPlugin.metrics(&cfg, &g).is_empty());
    assert!(CPlugin.function_units(&cfg, &g).is_empty());
}
