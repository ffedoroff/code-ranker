//! Tests for `markdown/mod.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn detects_by_md_presence_and_has_no_principles() {
    let d = tempfile::tempdir().unwrap();
    let p = MdPlugin;
    let cfg = p.config();
    assert!(!p.detect(&cfg, d.path(), &PluginInput::default()));
    std::fs::write(d.path().join("README.md"), "# Hi\n").unwrap();
    assert!(p.detect(&cfg, d.path(), &PluginInput::default()));
    assert_eq!(p.name(), "md");
    assert!(p.principles(&cfg, &PluginInput::default()).is_empty());
}

#[test]
fn analyze_emits_file_nodes_with_loc() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.md"), "# Title\n\nbody line\n").unwrap();
    let p = MdPlugin;
    let cfg = p.config();
    let g = p.analyze(&cfg, d.path(), &PluginInput::default()).unwrap();
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
