use super::*;
use code_ranker_plugin_api::attrs::ValueType;
use code_ranker_plugin_api::level::{AttributeSpec, group};

/// A metric spec with the fields the cards read.
fn metric(label: &str, name: &str, desc: &str, category: &str) -> AttributeSpec {
    let mut s = AttributeSpec::new(ValueType::Int, label);
    s.name = Some(name.to_string());
    s.description = Some(desc.to_string());
    s.group = Some(category.to_string());
    s
}

fn specs() -> DocSpecs {
    let mut node_attributes = BTreeMap::new();
    node_attributes.insert(
        "sloc".to_string(),
        metric("Source", "Source lines", "Source lines of code.", "loc"),
    );
    node_attributes.insert(
        "blank".to_string(),
        metric("Blank", "Blank lines", "Empty lines.", "loc"),
    );
    let mut groups = BTreeMap::new();
    groups.insert(
        "loc".to_string(),
        group("Lines of Code", "Lines of code breakdown"),
    );
    DocSpecs {
        // A project-defined principle with no corpus doc and no `doc_url` — so it
        // exercises the synthetic-card fallback (a real id like `SRP` would resolve
        // to the embedded `base/SRP.md`).
        principles: vec![Principle {
            id: "TSR".into(),
            label: "TSR".into(),
            title: "TSR — Test Ratio".into(),
            prompt: "Keep the test ratio healthy.".into(),
            doc_url: None,
            sort_metric: "hk".into(),
            connections: vec![],
        }],
        node_attributes,
        groups,
        templates: TemplatesConfig::default(),
    }
}

#[test]
fn fill_select_injects_live_values_into_the_doc_template() {
    let reason = "ambiguous project in .: markers for multiple plugins found (rust, markdown) — pass --plugin to choose";
    let md = fill_select(&templates::ai_doc_intro().unwrap(), reason);

    assert!(
        md.contains("code-ranker — AI agent skill"),
        "intro head present"
    );
    assert!(
        md.contains("## Commands") && md.contains("**`help`**") && md.contains("**`report"),
        "command list present"
    );
    assert!(md.contains("## Select a language"), "setup section present");
    assert!(
        md.contains(reason),
        "{{reason}} replaced with the diagnostic"
    );
    assert!(
        md.contains(&plugin::names()),
        "{{plugins}} replaced with the registry names"
    );
    assert!(
        md.contains(&format!("version = \"{CONFIG_VERSION}\"")),
        "{{config_version}} replaced with the live CONFIG_VERSION"
    );
    for ph in ["{reason}", "{plugins}", "{config_version}"] {
        assert!(!md.contains(ph), "placeholder {ph} fully substituted");
    }
}

#[test]
fn category_subject_resolves_case_insensitively() {
    let s = specs();
    assert_eq!(category_key(&s, "LOC").as_deref(), Some("loc"));
    assert_eq!(category_key(&s, "nope"), None);
}

#[test]
fn render_category_lists_label_description_and_members() {
    let out = render_category(&specs(), "loc");
    assert!(out.contains("Lines of Code"), "header (human label): {out}");
    assert!(
        out.contains("Lines of code breakdown"),
        "description: {out}"
    );
    // Member metrics, each with name + one-line description.
    assert!(
        out.contains("- sloc: Source lines — Source lines of code."),
        "{out}"
    );
    assert!(out.contains("- blank: Blank lines"), "{out}");
}

#[test]
fn render_metric_renders_the_spec_card() {
    let out = render_metric(&specs(), "sloc");
    assert!(out.contains("# sloc: Source lines"), "title: {out}");
    assert!(
        out.contains("Category: loc — Lines of Code"),
        "category: {out}"
    );
    assert!(out.contains("Source lines of code."), "description: {out}");
}

#[test]
fn render_principle_falls_back_to_a_synthetic_card_without_a_doc() {
    // The custom `TSR` test principle has no `doc_url` and no corpus stem match,
    // so resolution fails and the synthetic card is served.
    let out = render_principle(&specs(), "tsr").unwrap();
    assert!(out.contains("# TSR: TSR — Test Ratio"), "{out}");
    assert!(out.contains("Sort metric: `hk`"), "{out}");
    assert!(out.contains("Keep the test ratio healthy."), "{out}");
}

#[test]
fn catalog_lists_every_subject_class() {
    let out = render_catalog(&specs(), Some("zzz"));
    assert!(
        out.contains("Unknown docs subject `zzz`"),
        "lead note: {out}"
    );
    // Categories and their metrics (two-level): `<key> — <description>` header.
    assert!(
        out.contains("loc — Lines of code breakdown"),
        "category group: {out}"
    );
    assert!(
        out.contains("- sloc: Source lines"),
        "category member: {out}"
    );
    // Principles render as one more group.
    assert!(
        out.contains("principles — SOLID"),
        "principles group: {out}"
    );
    assert!(out.contains("- TSR: Test Ratio"), "principle member: {out}");
    // Closing note points at ai / metrics and the call-anything hint.
    assert!(
        out.contains("Call `docs`") && out.contains("docs ai"),
        "closing note: {out}"
    );
}

#[test]
fn metrics_index_lists_categories_and_members() {
    let out = render_metrics_index(&specs());
    assert!(
        out.contains("loc — Lines of code breakdown"),
        "category: {out}"
    );
    assert!(out.contains("- sloc: Source lines"), "member: {out}");
}

#[test]
fn principles_index_lists_each_principle() {
    let out = render_principles_index(&specs());
    assert!(out.contains("- TSR: Test Ratio"), "principle listed: {out}");
}

#[test]
fn principles_block_reports_when_the_plugin_defines_none() {
    let mut s = specs();
    s.principles.clear();
    let out = render_principles_index(&s);
    assert!(out.contains("(none"), "empty principles note: {out}");
}

#[test]
fn catalog_without_unknown_omits_the_lead_note() {
    // The bare-`docs` path passes `None` — the catalog is the help, so no lead note.
    let out = render_catalog(&specs(), None);
    assert!(
        !out.contains("Unknown docs subject"),
        "no unknown-subject note for the help view: {out}"
    );
    assert!(
        out.contains("code-ranker docs <subject>"),
        "still prints the catalog header: {out}"
    );
}

#[test]
fn categories_block_falls_back_to_the_label_for_a_group_without_a_description() {
    // A metric naming a category that ships no `[categories.<key>]` label/description:
    // the category key is still listed (header falls back to its Titlecase label).
    let mut s = specs();
    s.node_attributes.insert(
        "depth".to_string(),
        metric("Depth", "Nesting depth", "Max nesting.", "complexity"),
    );
    // No `groups["complexity"]` entry → the `None` description branch.
    let out = categories_block(&s);
    // No group entry → `category_label` falls back to the key itself.
    assert!(
        out.contains("complexity — complexity"),
        "category with no description echoes its key as the label: {out}"
    );
    assert!(
        out.contains("- depth: Nesting depth"),
        "member listed: {out}"
    );
}

#[test]
fn categories_block_lists_uncategorized_metrics_with_a_description() {
    let mut s = specs();
    // group = None + a description → surfaces under the (uncategorized) heading.
    let mut cycle = AttributeSpec::new(ValueType::Str, "Cycle");
    cycle.name = Some("Cycle member".to_string());
    cycle.description = Some("Part of a dependency cycle.".to_string());
    s.node_attributes.insert("cycle".to_string(), cycle);
    // group = None + NO description (bare external metadata) → skipped entirely.
    s.node_attributes.insert(
        "crate".to_string(),
        AttributeSpec::new(ValueType::Str, "Crate"),
    );
    let out = categories_block(&s);
    assert!(
        out.contains("(uncategorized)"),
        "uncategorized heading: {out}"
    );
    assert!(
        out.contains("- cycle: Cycle member"),
        "described uncategorized metric listed: {out}"
    );
    assert!(
        !out.contains("- crate:"),
        "doc-less metadata is skipped: {out}"
    );
}

#[test]
fn build_specs_without_config_uses_the_plugin_catalog_and_neutral_input() {
    // No config: exercises `default_plugin_input` and the `None` principle branch —
    // the result is the plugin's own catalog + central metric specs, undecorated.
    let specs = build_specs("rust", None);
    assert!(
        specs.node_attributes.contains_key("sloc"),
        "central LOC metric present"
    );
    assert!(
        specs.principles.iter().any(|p| p.id == "ADP"),
        "rust's principle catalog is present"
    );
}

#[test]
fn build_specs_overlays_project_metrics_and_principles() {
    let mut cfg = config::model::Config::default();
    // A node-scope `[metrics.<key>]` becomes a first-class metric subject.
    let mut def = code_ranker_graph::MetricDef {
        formula_cel: "sloc * 2".to_string(),
        ..Default::default()
    };
    def.scope = code_ranker_graph::Scope::Node;
    def.name = Some("Doubled SLOC".to_string());
    def.description = Some("Twice the source lines.".to_string());
    cfg.metrics.insert("dbl".to_string(), def);
    // A graph-scope metric must NOT leak into the node-attribute dictionary.
    let mut agg = code_ranker_graph::MetricDef {
        formula_cel: "sum(sloc)".to_string(),
        ..Default::default()
    };
    agg.scope = code_ranker_graph::Scope::Graph;
    cfg.metrics.insert("total".to_string(), agg);
    // A `[principles.<ID>]` is appended to the catalog.
    cfg.principles.insert(
        "TSR".to_string(),
        config::model::PrincipleDef {
            sort_metric: "dbl".to_string(),
            title: Some("TSR — Test Ratio".to_string()),
            ..Default::default()
        },
    );

    let specs = build_specs("rust", Some(cfg));
    assert!(
        specs.node_attributes.contains_key("dbl"),
        "node-scope project metric surfaced"
    );
    assert!(
        !specs.node_attributes.contains_key("total"),
        "graph-scope metric stays out of the node dictionary"
    );
    assert!(
        specs.principles.iter().any(|p| p.id == "TSR"),
        "project principle merged into the catalog"
    );
}
