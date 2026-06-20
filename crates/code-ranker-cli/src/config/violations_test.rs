use super::*;
use crate::config::model::CycleRule;
use code_ranker_graph::level_graph::CycleGroup;

/// The effective strict cycle rules (from the built-in `defaults.toml`) — the
/// trivial `RulesConfig::default()` is now an empty serde filler, so tests that
/// exercise the *shipped* default behaviour build it from `Config::default()`.
fn strict_rules() -> RulesConfig {
    crate::config::model::Config::default().rules
}

fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
    let mut n = Node {
        id: id.into(),
        kind: "file".into(),
        name: id.into(),
        parent: None,
        attrs: Default::default(),
    };
    for (k, v) in attrs {
        n.attrs.insert((*k).into(), v.clone());
    }
    n
}

fn level_with(nodes: Vec<Node>, cycles: Vec<CycleGroup>) -> BTreeMap<String, LevelGraph> {
    let level = LevelGraph {
        nodes,
        cycles,
        ..Default::default()
    };
    BTreeMap::from([("files".to_string(), level)])
}

#[test]
fn check_reports_enabled_cycle_group() {
    let graphs = level_with(
        vec![],
        vec![CycleGroup {
            kind: "chain".into(),
            nodes: vec!["a".into(), "b".into(), "c".into()],
        }],
    );
    let vs = check_violations(&graphs, &strict_rules());
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "cycle.chain");
    assert_eq!(vs[0].group, "CYC");
}

#[test]
fn cycle_budget_allows_up_to_n() {
    let cycles: Vec<CycleGroup> = (0..3)
        .map(|i| CycleGroup {
            kind: "chain".into(),
            nodes: vec![format!("a{i}"), format!("b{i}"), format!("c{i}")],
        })
        .collect();
    let graphs = level_with(vec![], cycles);
    let mut rules = RulesConfig::default();
    rules.cycles.chain = CycleRule::Max(3);
    assert!(check_violations(&graphs, &rules).is_empty());
    rules.cycles.chain = CycleRule::Max(2);
    assert_eq!(check_violations(&graphs, &rules).len(), 3);
}

#[test]
fn cycle_violation_points_at_breaking_edge_line() {
    use code_ranker_plugin_api::edge::Edge;
    let edge = |s: &str, t: &str, line: u32| Edge {
        source: s.into(),
        target: t.into(),
        kind: "uses".into(),
        line: Some(line),
        attrs: Default::default(),
    };
    let level = LevelGraph {
        nodes: vec![
            file_node("{target}/a.rs", &[]),
            file_node("{target}/b.rs", &[]),
        ],
        edges: vec![
            edge("{target}/a.rs", "{target}/b.rs", 12),
            edge("{target}/b.rs", "{target}/a.rs", 7),
        ],
        cycles: vec![CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
        }],
        ..Default::default()
    };
    let graphs = BTreeMap::from([("files".to_string(), level)]);
    let vs = check_violations(&graphs, &strict_rules());
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "cycle.mutual");
    // First edge in the level's order whose endpoints are both in the cycle
    // is a.rs -> b.rs at line 12.
    assert_eq!(vs[0].location, "{target}/a.rs");
    assert_eq!(vs[0].line, Some(12));
}

#[test]
fn check_reports_node_threshold_breach() {
    let graphs = level_with(
        vec![
            file_node("hot.rs", &[("cognitive", AttrValue::Int(50))]),
            file_node("cold.rs", &[("cognitive", AttrValue::Int(5))]),
        ],
        vec![],
    );
    let mut rules = RulesConfig::default();
    rules.thresholds.file.set("cognitive".into(), 25.0);
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "threshold.file.cognitive");
    assert_eq!(vs[0].group, "CPX");
    assert!(vs[0].location.contains("hot.rs"));
}

#[test]
fn sloc_threshold_reads_sloc_attr() {
    // `sloc` is a first-class threshold metric (the headline of the
    // open-vocabulary change), distinct from the raw `loc` line count.
    let graphs = level_with(
        vec![file_node("big.rs", &[("sloc", AttrValue::Int(1200))])],
        vec![],
    );
    let mut rules = RulesConfig::default();
    rules.thresholds.file.set("sloc".into(), 800.0);
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "threshold.file.sloc");
    assert_eq!(vs[0].group, "SIZ");
}

#[test]
fn custom_metric_threshold_breaches() {
    // A project `[metrics.tsr]` is checked exactly like a built-in: the key
    // doubles as the node-attribute key, and the breach reads the bare key as
    // its label (no registry entry to borrow a nicer one from).
    let graphs = level_with(
        vec![
            file_node("hot.rs", &[("tsr", AttrValue::Float(2.5))]),
            file_node("cold.rs", &[("tsr", AttrValue::Float(0.3))]),
        ],
        vec![],
    );
    let mut rules = RulesConfig::default();
    rules.thresholds.file.set("tsr".into(), 1.0);
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "threshold.file.tsr");
    assert!(vs[0].location.contains("hot.rs"));
    assert!(vs[0].message.contains("tsr"));
}

#[test]
fn loc_threshold_reads_loc_attr() {
    let graphs = level_with(
        vec![file_node("big.rs", &[("loc", AttrValue::Int(900))])],
        vec![],
    );
    let mut rules = RulesConfig::default();
    rules.thresholds.file.set("loc".into(), 500.0);
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "threshold.file.loc");
    assert_eq!(vs[0].group, "SIZ");
}

fn check_def(when: &str, message: &str, group: Option<&str>) -> code_ranker_graph::CheckDef {
    code_ranker_graph::CheckDef {
        when: when.into(),
        message: message.into(),
        group: group.map(str::to_string),
        why: None,
        fix: None,
        title: None,
    }
}

#[test]
fn custom_check_fires_with_path_predicate_and_carries_group() {
    // A DE1101-style check: inline tests (tloc>0) in a production file, but
    // sibling *_tests.rs files are exempt via the path predicate.
    let prod = {
        let mut n = file_node("{target}/src/handler.rs", &[("tloc", AttrValue::Int(40))]);
        n.attrs
            .insert("path".into(), AttrValue::Str("src/handler.rs".into()));
        n
    };
    let test_file = {
        let mut n = file_node(
            "{target}/src/handler_tests.rs",
            &[("tloc", AttrValue::Int(40))],
        );
        n.attrs
            .insert("path".into(), AttrValue::Str("src/handler_tests.rs".into()));
        n
    };
    let graphs = level_with(vec![prod, test_file], vec![]);
    let mut rules = RulesConfig::default();
    rules.checks.insert(
        "de1101".into(),
        check_def(
            r#"tloc > 0 && !name.endsWith("_tests.rs")"#,
            "{path}: {tloc} inline test lines",
            Some("TST"),
        ),
    );
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1, "only the production file fires");
    assert_eq!(vs[0].rule, "check.de1101");
    assert_eq!(vs[0].group, "TST");
    assert_eq!(vs[0].message, "src/handler.rs: 40 inline test lines");
    assert!(vs[0].location.contains("handler.rs"));
}

#[test]
fn bad_custom_check_predicate_becomes_a_loud_violation() {
    let graphs = level_with(
        vec![file_node("a.rs", &[("tloc", AttrValue::Int(1))])],
        vec![],
    );
    let mut rules = RulesConfig::default();
    rules
        .checks
        .insert("broken".into(), check_def("tloc >", "m", None));
    let vs = check_violations(&graphs, &rules);
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].rule, "check.broken");
    assert!(vs[0].message.contains("invalid `when` predicate"));
}
