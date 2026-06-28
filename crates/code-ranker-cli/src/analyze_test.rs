use super::*;
use std::collections::BTreeMap;
use std::fs;

fn mk_snap() -> Snapshot {
    use code_ranker_graph::snapshot::{LanguageSnapshot, SnapshotInit};
    use code_ranker_plugin_api::PromptTemplate;
    let mut languages = BTreeMap::new();
    languages.insert(
        "rust".to_string(),
        LanguageSnapshot {
            graphs: BTreeMap::new(),
            principles: Vec::new(),
            prompt: PromptTemplate::default(),
        },
    );
    Snapshot::new(SnapshotInit {
        command: "cmd".into(),
        workspace: "ws".into(),
        target: "tgt".into(),
        plugins: vec!["rust".to_string()],
        config_file: None,
        versions: BTreeMap::new(),
        roots: BTreeMap::new(),
        git: None,
        timings: Vec::new(),
        languages,
    })
}

/// A `rust` snapshot whose `files` level holds one mutual cycle.
fn mk_snap_with_cycle() -> Snapshot {
    use code_ranker_graph::level_graph::{CycleGroup, LevelGraph};
    use code_ranker_graph::snapshot::{LanguageSnapshot, SnapshotInit};
    use code_ranker_plugin_api::PromptTemplate;
    use code_ranker_plugin_api::node::Node;

    let node = |id: &str| Node {
        id: id.to_string(),
        kind: "file".into(),
        name: id.to_string(),
        parent: None,
        attrs: Default::default(),
    };
    let files = LevelGraph {
        nodes: vec![node("{target}/a.rs"), node("{target}/b.rs")],
        cycles: vec![CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
        }],
        ..Default::default()
    };
    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), files);
    let mut languages = BTreeMap::new();
    languages.insert(
        "rust".to_string(),
        LanguageSnapshot {
            graphs,
            principles: Vec::new(),
            prompt: PromptTemplate::default(),
        },
    );
    Snapshot::new(SnapshotInit {
        command: "report".into(),
        workspace: "ws".into(),
        target: "tgt".into(),
        plugins: vec!["rust".into()],
        config_file: None,
        versions: BTreeMap::new(),
        roots: BTreeMap::new(),
        git: None,
        timings: Vec::new(),
        languages,
    })
}

/// An `AnalyzeArgs` pointing at `input` with no analysis-only flags set.
fn args_for(input: std::path::PathBuf) -> AnalyzeArgs {
    AnalyzeArgs {
        input,
        plugins: vec![],
        config: vec![],
        ignore_paths: vec![],
        git_branch: None,
        git_commit: None,
        git_dirty_files: None,
        git_origin: None,
    }
}

/// A snapshot input is read and re-gated under the current rules: a `mutual=on`
/// cycle rule turns the embedded mutual cycle into a single `rust` violation.
#[test]
fn analyze_from_snapshot_regates_cycle() {
    let d = tempfile::tempdir().unwrap();
    let jp = d.path().join("s.json");
    fs::write(&jp, serde_json::to_string(&mk_snap_with_cycle()).unwrap()).unwrap();

    let analyzed = analyze_input(&args_for(jp), &["mutual=on".into()], &[]).unwrap();
    assert_eq!(analyzed.snapshot.plugins, vec!["rust"]);
    assert!(
        analyzed.rules_by_lang.contains_key("rust"),
        "per-language rules resolved from config"
    );
    assert_eq!(analyzed.violations.len(), 1, "the mutual cycle is gated");
    let v = &analyzed.violations[0];
    assert_eq!(v.language, "rust");
    assert_eq!(v.graph, "files");
    assert_eq!(v.rule, "cycle.mutual", "the cycle rule fired");
}

/// Analysis-only flags are rejected against a snapshot input — there is no
/// source tree to apply them to.
#[test]
fn analyze_from_snapshot_rejects_analysis_only_flags() {
    let d = tempfile::tempdir().unwrap();
    let jp = d.path().join("s.json");
    fs::write(&jp, serde_json::to_string(&mk_snap()).unwrap()).unwrap();

    let mut with_plugins = args_for(jp.clone());
    with_plugins.plugins = vec!["rust".into()];
    let err = analyze_input(&with_plugins, &[], &[])
        .err()
        .expect("plugins flag should be rejected")
        .to_string();
    assert!(
        err.contains("--plugins does not apply"),
        "plugins rejected: {err}"
    );

    let mut with_ignore = args_for(jp);
    with_ignore.ignore_paths = vec!["x/**".into()];
    let err = analyze_input(&with_ignore, &[], &[])
        .err()
        .expect("ignore flag should be rejected")
        .to_string();
    assert!(
        err.contains("--ignore does not apply"),
        "ignore rejected: {err}"
    );
}

#[test]
fn viewer_embeds_snapshot_inline_and_round_trips() {
    let snap = mk_snap();
    // review: current = snapshot, baseline = null
    let html = code_ranker_viewer::render_html_viewer(None, Some(&snap));
    assert!(
        html.contains(r#"<script type="application/json" id="cs-current">"#),
        "embeds current snapshot inline"
    );
    assert!(
        html.contains(r#"id="cs-baseline">null</script>"#),
        "baseline is null in review mode"
    );
    let back = code_ranker_viewer::extract_embedded_snapshot(&html, "cs-current")
        .expect("cs-current present")
        .unwrap();
    assert_eq!(
        back.plugins,
        vec!["rust"],
        "round-trips through embed/extract"
    );
    assert!(
        code_ranker_viewer::extract_embedded_snapshot(&html, "cs-baseline").is_none(),
        "null baseline extracts to None"
    );
}

#[test]
fn load_snapshot_any_reads_json_and_html() {
    let snap = mk_snap();
    let d = tempfile::tempdir().unwrap();

    let jp = d.path().join("s.json");
    fs::write(&jp, serde_json::to_string(&snap).unwrap()).unwrap();
    assert_eq!(
        load_snapshot_any(&jp).unwrap().plugins,
        vec!["rust"],
        "from .json"
    );

    let hp = d.path().join("r.html");
    fs::write(
        &hp,
        code_ranker_viewer::render_html_viewer(None, Some(&snap)),
    )
    .unwrap();
    assert_eq!(
        load_snapshot_any(&hp).unwrap().plugins,
        vec!["rust"],
        "from embedded .html"
    );
}

#[test]
fn load_snapshot_rejects_schema_version_mismatch() {
    let d = tempfile::tempdir().unwrap();
    let jp = d.path().join("old.json");
    // A snapshot tagged with a different schema version must be rejected
    // with a structured error (not silently mis-parsed).
    let mut v = serde_json::to_value(mk_snap()).unwrap();
    v["schema_version"] = serde_json::Value::String("1".into());
    fs::write(&jp, serde_json::to_string(&v).unwrap()).unwrap();
    let err = format!("{:#}", load_snapshot_any(&jp).unwrap_err());
    assert!(err.contains("schema_version"), "schema error: {err}");
    assert!(err.contains("\"1\""), "names the offending version: {err}");
}
