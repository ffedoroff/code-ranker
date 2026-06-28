use super::*;
use crate::attrs::num_attr;
use code_ranker_plugin_api::attrs::AttrValue;
use code_ranker_plugin_api::metrics::MetricInputs;
use code_ranker_plugin_api::node::Node;

#[test]
fn prompt_template_parses_from_markdown() {
    let t = prompt_template();
    assert_eq!(
        t.intro,
        "I want to apply this to some modules in my system."
    );
    assert!(
        t.doc_note.contains("`code-ranker docs {lang} {id}`"),
        "doc_note points at the offline per-language docs command: {:?}",
        t.doc_note
    );
    // `## task` keeps one entry per bullet, verbatim (the leading `- ` stays).
    assert_eq!(t.task.len(), 4, "four task bullets: {:?}", t.task);
    assert!(t.task.iter().all(|l| l.starts_with("- ")), "{:?}", t.task);
    assert!(
        t.task[3].contains("{id}"),
        "task keeps the {{id}} placeholder"
    );
    assert!(t.focus.starts_with("**Focus") && t.focus.ends_with("**"));
    assert!(t.cycle_note.starts_with("This is **one** dependency cycle"));
}

/// `prompt_template_from` is the hook a `[templates] prompt = "…"` override flows
/// through: it parses caller-supplied scaffolding instead of the built-in default.
#[test]
fn prompt_template_from_parses_caller_supplied_markdown() {
    let md =
        "## intro\nCustom intro line.\n\n## task\n- first\n- second\n\n## focus\nStay sharp.\n";
    let t = prompt_template_from(md);
    assert_eq!(t.intro, "Custom intro line.");
    assert_eq!(t.task, vec!["- first", "- second"]);
    assert_eq!(t.focus, "Stay sharp.");
    // Unspecified sections stay at their default (empty).
    assert!(t.cycle_note.is_empty());
}

/// `aggregate_formulas` exposes every `[report.stats]` formula; `stat_keys` is the
/// subset whose formula is a plain mean — so the keys must be a subset of the map.
#[test]
fn aggregate_formulas_superset_of_stat_keys() {
    let formulas = aggregate_formulas();
    assert!(!formulas.is_empty(), "built-in report.stats is non-empty");
    for k in stat_keys() {
        assert!(
            formulas.contains_key(&k),
            "stat key {k:?} has a backing formula"
        );
    }
}

#[test]
fn parses_and_compiles() {
    let (specs, groups) = metric_specs();
    assert!(specs.contains_key("volume"), "derived present");
    assert!(specs.contains_key("sloc"), "emitted measured present");
    // Halstead/AST base counts are now emitted (they carry a label), so the
    // derived formulas can render a live derivation line in the viewer.
    assert!(
        specs.contains_key("eta1"),
        "base count emitted (has a display spec)"
    );
    assert!(groups.contains_key("halstead"));
    let (defs, _engine) = &*super::write::DERIVED;
    assert!(defs.contains_key("volume") && defs.contains_key("cyclomatic"));
}

#[test]
fn spec_field_mapping_is_wire_compatible() {
    let (specs, _) = metric_specs();
    let vol = &specs["volume"];
    // formula_pretty → formula, formula_js → calc.
    assert_eq!(vol.formula.as_deref(), Some("length × log₂(vocabulary)"));
    assert_eq!(vol.calc.as_deref(), Some("length * Math.log2(vocabulary)"));
    // `short` falls back to `label` where the TOML omits it (closures sets no
    // `short`); `name` is the spec's own value.
    let clo = &specs["closures"];
    assert_eq!(clo.name.as_deref(), Some("Closures defined"));
    assert_eq!(clo.short.as_deref(), Some("Closures"));
    // multiline description re-encoded with <br>, no raw newlines.
    let cog = &specs["cognitive"];
    let desc = cog.description.as_deref().unwrap();
    assert!(desc.contains("<br>") && !desc.contains('\n'));
}

#[test]
fn stat_keys_are_the_mean_aggregates() {
    let keys = stat_keys();
    // The 17 reproduced means (incl. coupling), not the richer examples.
    assert!(keys.contains(&"cyclomatic".to_string()));
    assert!(keys.contains(&"hk".to_string()));
    assert!(
        !keys
            .iter()
            .any(|k| k.contains("_all_") || k.ends_with("_p99"))
    );
}

#[test]
fn derives_tier2_from_tier1() {
    let i = MetricInputs {
        eta1: 10.0,
        eta2: 13.0,
        n1: 40.0,
        n2: 47.0,
        spaces: 1.0,
        branches: 2.0,
        span_sloc: 20.0,
        sloc: 18.0,
        cloc: 4.0,
        ..Default::default()
    };
    let mut node = Node {
        id: "x".into(),
        kind: "file".into(),
        name: "x".into(),
        parent: None,
        attrs: Default::default(),
    };
    write_metrics(&mut node, &i);
    assert_eq!(node.attrs.get("cyclomatic"), Some(&num_attr(3.0)));
    let want = 87.0_f64 * 23.0_f64.log2();
    assert_eq!(node.attrs.get("volume"), Some(&num_attr(want)));
    // `hk` is graph-derived: write_metrics runs before the coupling pass, so it is
    // not emitted here even though `sloc` is present.
    assert_eq!(node.attrs.get("hk"), None, "hk is not a pre-graph field");
}

#[test]
fn graph_derived_hk_from_coupling_counts() {
    let mut node = Node {
        id: "x".into(),
        kind: "file".into(),
        name: "x".into(),
        parent: None,
        attrs: Default::default(),
    };
    node.attrs.insert("sloc".into(), AttrValue::Int(10));
    node.attrs.insert("fan_in".into(), AttrValue::Int(2));
    node.attrs.insert("fan_out".into(), AttrValue::Int(3));
    write_derived(&mut node);
    // hk = sloc * (fan_in * fan_out)^2 = 10 * (2*3)^2 = 360
    assert_eq!(node.attrs.get("hk"), Some(&num_attr(360.0)));

    // No coupling: fan_in/fan_out absent seed to 0 → hk = 0 → omitted.
    let mut bare = Node {
        id: "y".into(),
        kind: "file".into(),
        name: "y".into(),
        parent: None,
        attrs: Default::default(),
    };
    bare.attrs.insert("sloc".into(), AttrValue::Int(10));
    write_derived(&mut bare);
    assert_eq!(bare.attrs.get("hk"), None);
}
