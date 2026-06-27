use super::*;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_graph::snapshot::{LanguageSnapshot, Snapshot, SnapshotInit};
use code_ranker_plugin_api::Principle;
use code_ranker_plugin_api::attrs::ValueType;
use code_ranker_plugin_api::level::AttributeSpec;
use std::collections::BTreeMap;

/// The single language a test snapshot carries.
const LANG: &str = "rust";

/// Test shim mirroring the old snapshot-based `resolve_doc`: pulls the principles
/// and `files`-level node-attribute specs out of a test snapshot and feeds the
/// spec-based core. Keeps these tests reading naturally now that production
/// resolves docs from config/plugin specs (no snapshot) via `docs`.
fn resolve_doc(s: &Snapshot, templates: &TemplatesConfig, id: &str) -> Result<String> {
    let lang = &s.languages[LANG];
    resolve_doc_from_specs(
        &lang.principles,
        &lang.graphs["files"].node_attributes,
        templates,
        id,
    )
}

/// A snapshot carrying just the bits `resolve_doc`/`doc_rel_path` read:
/// the principles and the `files` level's node-attribute specs.
fn snap(principles: Vec<Principle>, files_attrs: BTreeMap<String, AttributeSpec>) -> Snapshot {
    let files = LevelGraph {
        node_attributes: files_attrs,
        ..Default::default()
    };
    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), files);
    let mut languages = BTreeMap::new();
    languages.insert(
        LANG.to_string(),
        LanguageSnapshot {
            graphs,
            principles,
            prompt: Default::default(),
        },
    );
    Snapshot::new(SnapshotInit {
        command: "report".into(),
        workspace: ".".into(),
        target: ".".into(),
        plugins: vec![LANG.to_string()],
        config_file: None,
        versions: BTreeMap::new(),
        roots: BTreeMap::new(),
        git: None,
        timings: vec![],
        languages,
    })
}

fn principle(id: &str, doc_url: &str) -> Principle {
    Principle {
        id: id.to_string(),
        label: id.to_string(),
        title: id.to_string(),
        prompt: String::new(),
        doc_url: Some(doc_url.to_string()),
        sort_metric: "hk".to_string(),
        connections: vec![],
    }
}

fn metric_spec() -> AttributeSpec {
    // The doc now resolves from the attribute's key (not a remediation string), so a
    // bare spec under the right key is all these tests need.
    AttributeSpec::new(ValueType::Float, "HK")
}

#[test]
fn resolve_doc_serves_base_fallback() {
    let s = snap(
        vec![principle(
            "SRP",
            "https://x/blob/main/languages/base/SRP.md",
        )],
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
        vec![principle(
            "ADP",
            "https://x/blob/main/languages/rust/ADP.md",
        )],
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
fn resolve_doc_manifest_uses_base_override_when_present() {
    // A `templates.languages.base.<ID>` override substitutes the neutral base that
    // a language manifest assembles over, so the custom base flows into the result.
    let dir = tempfile::tempdir().unwrap();
    let base_path = dir.path().join("base_adp.md");
    let custom_base = corpus_doc("base/ADP.md")
        .unwrap()
        .replace("Acyclic", "ZZ-MARKER-ACYCLIC");
    std::fs::write(&base_path, &custom_base).unwrap();
    let mut templates = TemplatesConfig::default();
    let mut base_overrides = BTreeMap::new();
    base_overrides.insert("ADP".to_string(), base_path.to_string_lossy().into_owned());
    templates
        .languages
        .insert("base".to_string(), base_overrides);

    let s = snap(
        vec![principle(
            "ADP",
            "https://x/blob/main/languages/rust/ADP.md",
        )],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &templates, "ADP").unwrap();
    let manifest = corpus_doc("rust/ADP.md").unwrap();
    let expected = crate::compose::compose(manifest, &custom_base, "Rust").unwrap();
    assert_eq!(doc, expected);
    assert!(
        doc.contains("ZZ-MARKER-ACYCLIC"),
        "custom base flowed in: {doc}"
    );
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
        vec![principle(
            "SRP",
            "https://x/blob/main/languages/rust/SRP.md",
        )],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &templates, "SRP").unwrap();
    assert_eq!(doc, "# my own SRP\n");
}

#[test]
fn resolve_doc_cycle_resolves_to_adp() {
    // `cycle` is ADP's metric lens (not a node attribute), so `--doc cycle` serves
    // the ADP doc — resolved through the ADP principle, same as `--doc ADP`.
    let s = snap(
        vec![principle(
            "ADP",
            "https://x/blob/main/languages/rust/ADP.md",
        )],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "cycle").unwrap();
    let manifest = corpus_doc("rust/ADP.md").unwrap();
    let base = corpus_doc("base/ADP.md").unwrap();
    let expected = crate::compose::compose(manifest, base, "Rust").unwrap();
    assert_eq!(doc, expected, "`--doc cycle` serves the ADP doc");
}

#[test]
fn resolve_doc_finds_metric_doc_by_key() {
    // No matching principle — the doc resolves through the metric key itself: the
    // attribute is present in `node_attributes`, and its base-corpus doc is found by
    // normalized stem (`hk`→`HK`, separators/case ignored). Metric docs live in base/.
    let mut attrs = BTreeMap::new();
    attrs.insert("hk".to_string(), metric_spec());
    let s = snap(vec![], attrs);
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "HK").unwrap();
    assert_eq!(doc, corpus_doc("base/HK.md").unwrap());
}

#[test]
fn normalize_id_collapses_separators_and_case() {
    assert_eq!(normalize_id("Fan-in"), "fanin");
    assert_eq!(normalize_id("fan_in"), "fanin");
    assert_eq!(normalize_id("FAN in"), "fanin");
    assert_eq!(normalize_id("HK"), "hk");
}

#[test]
fn metric_doc_stem_maps_key_to_corpus_stem() {
    // `_`/`-`/case all ignored, so a metric key finds its corpus doc.
    assert_eq!(metric_doc_stem("hk"), Some("HK"));
    assert_eq!(metric_doc_stem("fan_in"), Some("Fan-in"));
    assert_eq!(metric_doc_stem("fan_out"), Some("Fan-out"));
    // A metric with no prose doc resolves to nothing.
    assert_eq!(metric_doc_stem("sloc"), None);
}

#[test]
fn doc_rel_path_serves_lang_override_for_a_metric_doc() {
    // A metric doc (`hk` → `HK`) is routed to the `<lang>/` corpus when the
    // plugin's principle docs route there (`override_lang` → "rust") AND that
    // language actually ships `rust/HK.md` — the metric-override branch added in
    // 566fb23 (templates.rs line 63). Without a rust-routing principle the same
    // metric falls back to `base/HK.md` (see the previous test).
    let mut attrs = BTreeMap::new();
    attrs.insert("hk".to_string(), metric_spec());
    let s = snap(
        vec![principle(
            "ADP",
            "https://x/blob/main/languages/rust/ADP.md",
        )],
        attrs,
    );
    let lang = &s.languages[LANG];
    let na = &lang.graphs["files"].node_attributes;
    assert_eq!(
        doc_rel_path(&lang.principles, na, "HK"),
        Some("rust/HK.md".to_string())
    );
}

#[test]
fn resolve_doc_unknown_id_errors() {
    let s = snap(
        vec![principle(
            "SRP",
            "https://x/blob/main/languages/base/SRP.md",
        )],
        BTreeMap::new(),
    );
    let err = resolve_doc(&s, &TemplatesConfig::default(), "ZZZ").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no principle or metric doc"), "{msg}");
    assert!(msg.contains("SRP"), "known principles listed: {msg}");
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

#[test]
fn resolve_doc_ai_index_expands_tldr_marker() {
    // The AI overview resolves by filename fallback, and its
    // `<!-- doc:tldr-index -->` marker expands to the per-doc catalog.
    let s = snap(
        vec![principle(
            "ADP",
            "https://x/blob/main/languages/rust/ADP.md",
        )],
        BTreeMap::new(),
    );
    let doc = resolve_doc(&s, &TemplatesConfig::default(), "AI").unwrap();
    assert!(
        doc.contains("code-ranker — AI agent skill"),
        "overview head kept"
    );
    assert!(
        !doc.contains("doc:tldr-index"),
        "marker expanded, not left literal"
    );
    assert!(
        doc.contains("### ADP — Acyclic Dependencies Principle"),
        "catalog lists ADP"
    );
    assert!(
        doc.contains("Full doc: `code-ranker docs ADP`"),
        "each entry points at its --doc id"
    );
    assert!(doc.contains("**TL;DR**"), "entries carry their TL;DR");
    assert!(
        !doc.contains("### code-ranker — AI agent skill"),
        "AI.md excludes itself from its own index"
    );
}

#[test]
fn ai_doc_matches_resolve_doc_and_needs_no_snapshot() {
    // `ai_doc()` backs the project-free `ai` subcommand: it must produce exactly
    // what `docs AI` does, but without a snapshot or plugin.
    let doc = ai_doc().unwrap();
    let via_resolve = resolve_doc(
        &snap(vec![], BTreeMap::new()),
        &TemplatesConfig::default(),
        "AI",
    )
    .unwrap();
    assert_eq!(doc, via_resolve, "ai_doc == docs AI output");
    assert!(
        doc.contains("code-ranker — AI agent skill"),
        "overview head"
    );
    assert!(!doc.contains("doc:tldr-index"), "catalog marker expanded");
    assert!(
        !doc.contains("ai:select"),
        "select-section markers stripped"
    );
    assert!(
        doc.contains("## Commands") && doc.contains("**`help`**"),
        "the playbook lists the main commands incl. help"
    );
    assert!(
        doc.contains("## Principles & metrics") && doc.contains("### ADP"),
        "the resolved-mode doc carries the full catalog"
    );
    // The Select-a-language section is stripped in the resolved doc — plugin setup is
    // only shown by the `ai` command's unresolved branch (see `ai::fill_select`). Its
    // placeholders must never leak into a served doc.
    assert!(
        !doc.contains("## Select a language"),
        "resolved playbook never mentions how to set the plugin"
    );
    for ph in ["{reason}", "{plugins}", "{config_version}"] {
        assert!(
            !doc.contains(ph),
            "no template placeholder {ph} in a served doc"
        );
    }
}

#[test]
fn ai_doc_intro_keeps_description_and_commands_but_not_the_playbook() {
    let intro = ai_doc_intro().unwrap();
    assert!(
        intro.contains("code-ranker — AI agent skill"),
        "intro keeps the title + product description"
    );
    assert!(
        intro.contains("## Commands")
            && intro.contains("**`check")
            && intro.contains("**`report")
            && intro.contains("**`docs")
            && intro.contains("**`help`**"),
        "intro lists the main commands: {intro}"
    );
    // Carries the Select-a-language template (placeholders still raw — `ai` fills them).
    assert!(
        intro.contains("## Select a language") && intro.contains("{plugins}"),
        "intro includes the plugin-setup template from the doc: {intro}"
    );
    assert!(
        !intro.contains("ai:select"),
        "bracketing markers not included"
    );
    // Stops before the analysis playbook + catalog (those wait for a plugin).
    assert!(
        !intro.contains("## The two that matter most")
            && !intro.contains("## Principles & metrics"),
        "intro stops before the analysis playbook: {intro}"
    );
}

#[test]
fn resolve_doc_resolves_base_doc_by_filename_stem() {
    // Docs that are neither a principle nor a node attribute resolve by their base
    // filename stem: hyphenated metric files (key is `fan_in`, file is `Fan-in`)
    // and the `metrics` reference.
    let s = snap(vec![], BTreeMap::new());
    assert_eq!(
        resolve_doc(&s, &TemplatesConfig::default(), "Fan-in").unwrap(),
        corpus_doc("base/Fan-in.md").unwrap()
    );
    assert_eq!(
        resolve_doc(&s, &TemplatesConfig::default(), "metrics").unwrap(),
        corpus_doc("base/metrics.md").unwrap()
    );
}

#[test]
fn doc_summary_prefers_tldr_then_first_paragraph() {
    let with_tldr = "# T\n\n**TL;DR**: line one\nline two\n\n## Next\nbody";
    assert_eq!(
        doc_summary(with_tldr).as_deref(),
        Some("**TL;DR**: line one line two")
    );
    let no_tldr = "# T\n\nFirst prose paragraph.\nstill it.\n\n## Next";
    assert_eq!(
        doc_summary(no_tldr).as_deref(),
        Some("First prose paragraph. still it.")
    );
}

#[test]
fn catalog_entry_includes_summary_when_present_and_omits_when_absent() {
    let with = catalog_entry("Henry–Kafura", "HK", Some("A coupling metric."));
    assert!(
        with.starts_with("### Henry–Kafura"),
        "heading first: {with}"
    );
    assert!(
        with.contains("Full doc: `code-ranker docs HK`"),
        "carries the --doc pointer: {with}"
    );
    assert!(
        with.ends_with("A coupling metric."),
        "summary appended: {with}"
    );

    // No summary → heading + pointer only, no trailing paragraph (the `None` arm).
    let without = catalog_entry("Edge Case", "EC", None);
    assert_eq!(without, "### Edge Case\n\nFull doc: `code-ranker docs EC`");
}

#[test]
fn with_trailing_newline_appends_only_when_missing() {
    assert_eq!(with_trailing_newline("x".to_string()), "x\n");
    assert_eq!(
        with_trailing_newline("x\n".to_string()),
        "x\n",
        "already terminated → unchanged, no double newline"
    );
    assert_eq!(with_trailing_newline(String::new()), "\n");
}
