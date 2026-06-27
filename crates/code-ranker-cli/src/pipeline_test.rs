use super::*;
use std::fs;

#[test]
fn project_principles_override_then_append() {
    use code_ranker_plugin_api::Principle;
    let catalog = vec![Principle {
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
        config::model::PrincipleDef {
            prompt: "new".into(),
            sort_metric: "cyclomatic".into(),
            ..Default::default()
        },
    );
    // New id → appended.
    project.insert(
        "TSR".to_string(),
        config::model::PrincipleDef {
            sort_metric: "tsr".into(),
            ..Default::default()
        },
    );
    let merged = config::merge_project_principles(catalog, &project);
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
        format!(
            "version = \"{}\"\n{}",
            code_ranker_graph::version::CONFIG_VERSION,
            r#"[plugins.base.metrics.below]
formula_cel = "sloc"
info = 50

[plugins.base.metrics.above]
formula_cel = "sloc"
info = 200

[plugins.base.rules.thresholds.file]
hk = 500000
below = 100
above = 100
"#
        ),
    )
    .unwrap();
    let loaded =
        config::load(dir.path(), &[cfg_path.display().to_string()], &[], &[], &[]).unwrap();
    let th = gate_thresholds(&loaded.config.language_config("base").unwrap());

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
    let cfg = toml::Table::new();
    let level = assemble_level(
        None,
        Graph::default(),
        vec![],
        BTreeMap::new(),
        BTreeMap::new(),
        &custom,
        "rust",
        &cfg,
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
    let cfg = toml::Table::new();
    let level = assemble_level(
        Some(spec),
        Graph::default(),
        vec![],
        BTreeMap::new(),
        BTreeMap::new(),
        &custom,
        "rust",
        &cfg,
        &[],
    );
    let g = level.ui.grouping.expect("function grouping retained");
    assert_eq!(g.function.as_deref(), Some("dir"));
}

/// Single-marker detection cases: each marker file causes exactly the expected
/// plugin to be detected via `detect_all`.
#[test]
fn detect_plugin_by_single_marker() {
    use code_ranker_plugin_api::plugin::PluginInput;
    let cases = vec![
        ("Cargo.toml", "rust"),
        ("pyproject.toml", "python"),
        ("setup.py", "python"),
        ("package.json", "javascript"),
        ("tsconfig.json", "typescript"),
    ];
    let empty_overrides = BTreeMap::new();
    for (marker, expected) in cases {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(marker), "").unwrap();
        let eff_cfgs: BTreeMap<String, toml::Table> = plugin::registry()
            .iter()
            .map(|p| {
                (
                    p.name().to_string(),
                    plugin::effective_plugin_config(p.name(), &empty_overrides),
                )
            })
            .collect();
        let detected = plugin::detect_all(&eff_cfgs, d.path(), &PluginInput::default());
        assert!(
            detected.contains(&expected.to_string()),
            "marker {marker} should detect {expected}, got: {detected:?}"
        );
    }
}

/// `detect_all` returns an empty list on an empty directory, and multiple
/// results when both Cargo.toml and package.json are present — no error.
#[test]
fn detect_all_multi_and_empty() {
    use code_ranker_plugin_api::plugin::PluginInput;
    let empty_overrides = BTreeMap::new();

    // Empty directory: no detections (previously an error, now just empty).
    let empty = tempfile::tempdir().unwrap();
    let eff_cfgs: BTreeMap<String, toml::Table> = plugin::registry()
        .iter()
        .map(|p| {
            (
                p.name().to_string(),
                plugin::effective_plugin_config(p.name(), &empty_overrides),
            )
        })
        .collect();
    let detected = plugin::detect_all(&eff_cfgs, empty.path(), &PluginInput::default());
    assert!(
        detected.is_empty(),
        "empty directory → no detections (no error): {detected:?}"
    );

    // Two markers → two detections (not an error).
    let amb = tempfile::tempdir().unwrap();
    fs::write(amb.path().join("Cargo.toml"), "").unwrap();
    fs::write(amb.path().join("package.json"), "").unwrap();
    let detected = plugin::detect_all(&eff_cfgs, amb.path(), &PluginInput::default());
    assert!(
        detected.len() >= 2,
        "two markers → two (or more) detections: {detected:?}"
    );
    assert!(
        detected.contains(&"rust".to_string()),
        "rust detected: {detected:?}"
    );
    assert!(
        detected.contains(&"javascript".to_string()),
        "javascript detected: {detected:?}"
    );
    // Sorted.
    let mut sorted = detected.clone();
    sorted.sort_unstable();
    assert_eq!(detected, sorted, "detect_all output is sorted");
}
