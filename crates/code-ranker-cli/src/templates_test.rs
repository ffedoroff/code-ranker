use super::*;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::Preset;
use code_ranker_plugin_api::attrs::ValueType;
use code_ranker_plugin_api::level::AttributeSpec;
use std::collections::BTreeMap;

/// A snapshot carrying just the bits `resolve_doc`/`doc_rel_path` read:
/// the principle presets and the `files` level's node-attribute specs.
fn snap(presets: Vec<Preset>, files_attrs: BTreeMap<String, AttributeSpec>) -> Snapshot {
    let files = LevelGraph {
        node_attributes: files_attrs,
        ..Default::default()
    };
    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), files);
    Snapshot::new(
        "report".into(),
        ".".into(),
        ".".into(),
        "rust".into(),
        None,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
        vec![],
        graphs,
        presets,
        Default::default(),
    )
}

fn preset(id: &str, doc_url: &str) -> Preset {
    Preset {
        id: id.to_string(),
        label: id.to_string(),
        title: id.to_string(),
        prompt: String::new(),
        doc_url: Some(doc_url.to_string()),
        sort_metric: "hk".to_string(),
        connections: vec![],
    }
}

fn metric_spec(remediation: &str) -> AttributeSpec {
    let mut spec = AttributeSpec::new(ValueType::Float, "HK");
    spec.remediation = Some(remediation.to_string());
    spec
}

#[test]
fn resolve_doc_serves_base_fallback() {
    let s = snap(
        vec![preset("SRP", "https://x/blob/main/languages/base/SRP.md")],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "SRP").unwrap();
    assert_eq!(doc, corpus_doc("base/SRP.md").unwrap());
}

#[test]
fn resolve_doc_assembles_a_language_manifest() {
    // rust/ADP.md is a manifest (`<!-- doc:base … -->`), so the resolved doc
    // is the composition over base/ADP.md, not the raw manifest text.
    let s = snap(
        vec![preset("ADP", "https://x/blob/main/languages/rust/ADP.md")],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "ADP").unwrap();
    let manifest = corpus_doc("rust/ADP.md").unwrap();
    let base = corpus_doc("base/ADP.md").unwrap();
    let expected = crate::compose::compose(manifest, base, "Rust").unwrap();
    assert_eq!(doc, expected);
    assert!(!doc.contains("<!-- doc:base"), "includes were expanded");
}

#[test]
fn resolve_doc_override_wins_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("custom.md");
    std::fs::write(&path, "# my own SRP\n").unwrap();
    let mut templates = TemplatesConfig::default();
    let mut srp = BTreeMap::new();
    srp.insert("SRP".to_string(), path.to_string_lossy().into_owned());
    templates.languages.insert("rust".to_string(), srp);

    let s = snap(
        vec![preset("SRP", "https://x/blob/main/languages/rust/SRP.md")],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &templates, "SRP").unwrap();
    assert_eq!(doc, "# my own SRP\n");
}

#[test]
fn resolve_doc_finds_metric_via_remediation_url() {
    // No matching preset — the doc resolves through the metric's remediation
    // URL instead (lowercased attribute key).
    let mut attrs = BTreeMap::new();
    attrs.insert(
        "hk".to_string(),
        metric_spec("See https://x/blob/main/languages/base/HK.md for the fix"),
    );
    let s = snap(vec![], attrs);
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "HK").unwrap();
    assert_eq!(doc, corpus_doc("base/HK.md").unwrap());
}

#[test]
fn resolve_doc_unknown_id_errors() {
    let s = snap(
        vec![preset("SRP", "https://x/blob/main/languages/base/SRP.md")],
        BTreeMap::new(),
    );
    let err = resolve_doc(&s, &TemplatesConfig::default(), "ZZZ").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no principle or metric doc"), "{msg}");
    assert!(msg.contains("SRP"), "known principles listed: {msg}");
}

#[test]
fn build_corpus_writes_every_doc_including_assembled() {
    let dir = tempfile::tempdir().unwrap();
    let n = build_corpus(dir.path()).unwrap();
    assert!(n > 0, "wrote at least one doc");

    // Base docs are copied verbatim.
    assert!(dir.path().join("base/HK.md").exists());

    // A rust manifest is published assembled, with its includes expanded.
    let assembled = std::fs::read_to_string(dir.path().join("rust/ADP.md")).unwrap();
    assert!(
        !assembled.contains("<!-- doc:base"),
        "manifest includes expanded on publish"
    );
}

#[test]
fn lang_display_maps_known_folders_and_passes_through() {
    assert_eq!(lang_display("rust"), "Rust");
    assert_eq!(lang_display("cpp"), "C++");
    assert_eq!(lang_display("csharp"), "C#");
    assert_eq!(lang_display("unknown-lang"), "unknown-lang");
}

#[test]
fn corpus_is_embedded_and_keyed_by_rel_path() {
    // The base fallback corpus is always present.
    assert!(corpus_doc("base/HK.md").is_some(), "base/HK.md embedded");
    assert!(corpus_doc("base/SRP.md").is_some(), "base/SRP.md embedded");
    assert!(corpus_doc("nope/X.md").is_none());
}

#[test]
fn url_tail_extracts_corpus_path() {
    assert_eq!(
        url_tail("https://x/blob/main/languages/base/HK.md").as_deref(),
        Some("base/HK.md")
    );
    assert_eq!(
        url_tail("Download from https://x/main/languages/rust/SRP.md now").as_deref(),
        Some("rust/SRP.md"),
        "anchored on /languages/, trailing prose trimmed"
    );
    assert_eq!(url_tail("https://x/elsewhere/HK.md"), None);
}

#[test]
fn bare_relative_path_defaults_to_base_folder() {
    // `split_once('/')` fallback: a path with no slash is treated as base/<id>.
    let (lang, file) = "HK.md".split_once('/').unwrap_or(("base", "HK.md"));
    assert_eq!((lang, file), ("base", "HK.md"));
}
