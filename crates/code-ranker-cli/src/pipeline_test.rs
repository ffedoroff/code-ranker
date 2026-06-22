use super::*;
use std::fs;

#[test]
fn project_presets_override_then_append() {
    use code_ranker_plugin_api::Preset;
    let catalog = vec![Preset {
        id: "CPX".into(),
        label: "CPX".into(),
        title: "Complexity".into(),
        prompt: "old".into(),
        doc_url: None,
        sort_metric: "cognitive".into(),
        connections: vec![],
    }];
    let mut project = BTreeMap::new();
    // Same id → replaces the catalog entry in place.
    project.insert(
        "CPX".to_string(),
        config::model::PresetDef {
            prompt: "new".into(),
            sort_metric: "cyclomatic".into(),
            ..Default::default()
        },
    );
    // New id → appended.
    project.insert(
        "TSR".to_string(),
        config::model::PresetDef {
            sort_metric: "tsr".into(),
            ..Default::default()
        },
    );
    let merged = merge_project_presets(catalog, &project);
    assert_eq!(merged.len(), 2);
    let cpx = merged.iter().find(|p| p.id == "CPX").unwrap();
    assert_eq!(cpx.sort_metric, "cyclomatic", "same id replaced in place");
    assert_eq!(cpx.prompt, "new");
    let tsr = merged.iter().find(|p| p.id == "TSR").unwrap();
    assert_eq!(tsr.sort_metric, "tsr");
    assert_eq!(tsr.title, "TSR", "title defaults to id");
}

#[test]
fn gate_thresholds_uses_gate_as_warning_and_reconciles_info() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("code-ranker.toml");
    fs::write(
        &cfg_path,
        r#"
[metrics.below]
formula_cel = "sloc"
info = 50

[metrics.above]
formula_cel = "sloc"
info = 200

[rules.thresholds.file]
hk = 500000
below = 100
above = 100
"#,
    )
    .unwrap();
    let loaded =
        config::load(dir.path(), &[cfg_path.display().to_string()], &[], &[], &[]).unwrap();
    let th = gate_thresholds(&loaded.config);

    // Built-in metric, no `[metrics]` info → `info` mirrors the gate (one tier).
    assert_eq!(th["hk"].warning, 500_000.0);
    assert_eq!(th["hk"].info, 500_000.0);
    // Custom metric whose declared `info` sits below the gate → `info` kept.
    assert_eq!(th["below"].warning, 100.0);
    assert_eq!(th["below"].info, 50.0);
    // Custom metric whose declared `info` is ≥ the gate → `info` collapses to it.
    assert_eq!(th["above"].warning, 100.0);
    assert_eq!(th["above"].info, 100.0);
}

#[test]
fn assemble_level_synthesizes_default_spec_when_none() {
    // No plugin-provided level spec → `assemble_level` falls back to a bare
    // `files` Level, then layers the central metric/coupling specs over it.
    use code_ranker_plugin_api::graph::Graph;
    let custom = BTreeMap::new();
    let level = assemble_level(
        None,
        Graph::default(),
        vec![],
        BTreeMap::new(),
        BTreeMap::new(),
        &custom,
        "rust",
        &[],
    );
    // An empty graph prunes every metric spec (no node carries it), so the
    // assembled level is well-formed but empty — the point is the default-spec
    // fallback ran without a plugin-provided `Level`.
    assert!(level.nodes.is_empty());
    assert!(level.node_attributes.is_empty(), "pruned to present keys");
}

#[test]
fn assemble_level_keeps_grouping_with_a_function() {
    // A grouping that names a `function` (no `key`) is always usable, so it
    // survives the pruning filter even though no attribute backs it.
    use code_ranker_plugin_api::graph::Graph;
    use code_ranker_plugin_api::level::{Grouping, Level};
    let spec = Level {
        name: "files".into(),
        edge_kinds: BTreeMap::new(),
        node_attributes: BTreeMap::new(),
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds: BTreeMap::new(),
        cycle_kinds: BTreeMap::new(),
        grouping: Some(Grouping {
            key: None,
            function: Some("dir".into()),
        }),
    };
    let custom = BTreeMap::new();
    let level = assemble_level(
        Some(spec),
        Graph::default(),
        vec![],
        BTreeMap::new(),
        BTreeMap::new(),
        &custom,
        "rust",
        &[],
    );
    let g = level.ui.grouping.expect("function grouping retained");
    assert_eq!(g.function.as_deref(), Some("dir"));
}

#[test]
fn detect_plugin_by_single_marker() {
    let cases = vec![
        ("Cargo.toml", "rust"),
        ("pyproject.toml", "python"),
        ("setup.py", "python"),
        ("package.json", "javascript"),
        ("tsconfig.json", "typescript"),
    ];
    for (marker, expected) in cases {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(marker), "").unwrap();
        assert_eq!(
            plugin::detect(d.path(), &PluginInput::default()).unwrap(),
            expected,
            "marker {marker}"
        );
    }
}

#[test]
fn detect_plugin_errors_on_ambiguous_or_empty() {
    let amb = tempfile::tempdir().unwrap();
    fs::write(amb.path().join("Cargo.toml"), "").unwrap();
    fs::write(amb.path().join("package.json"), "").unwrap();
    let err = format!(
        "{:#}",
        plugin::detect(amb.path(), &PluginInput::default()).unwrap_err()
    );
    assert!(err.contains("multiple"), "ambiguous error: {err}");

    let empty = tempfile::tempdir().unwrap();
    let err = format!(
        "{:#}",
        plugin::detect(empty.path(), &PluginInput::default()).unwrap_err()
    );
    assert!(err.contains("no project marker"), "empty error: {err}");
}
