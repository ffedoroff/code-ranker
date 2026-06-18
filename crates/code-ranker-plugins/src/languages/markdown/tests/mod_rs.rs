//! Tests for `markdown/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_md_presence_and_has_no_presets() {
    let d = tempfile::tempdir().unwrap();
    let p = MarkdownPlugin;
    assert!(!p.detect(d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("README.md"), "# Hi\n").unwrap();
    assert!(p.detect(d.path(), &PluginInput::default()));
    assert_eq!(p.name(), "markdown");
    assert!(p.presets(&PluginInput::default()).is_empty());
}

#[test]
fn analyze_emits_file_nodes_with_loc() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.md"), "# Title\n\nbody line\n").unwrap();
    let p = MarkdownPlugin;
    let g = p.analyze(d.path(), &PluginInput::default()).unwrap();
    let file = g
        .nodes
        .iter()
        .find(|n| n.name == "a.md")
        .expect("a.md node");
    assert!(matches!(
        file.attrs.get("loc"),
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(n)) if *n >= 3
    ));
}
