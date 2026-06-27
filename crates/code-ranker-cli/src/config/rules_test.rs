use super::*;

#[test]
fn rule_doc_resolves_why_fix_from_specs_and_cycle_kinds() {
    use code_ranker_plugin_api::attrs::ValueType;
    let mut na = BTreeMap::new();
    let mut hk = AttributeSpec::new(ValueType::Float, "HK");
    hk.name = Some("Henry–Kafura".into());
    hk.description = Some("why-hk".into());
    hk.remediation = Some("fix-hk".into());
    na.insert("hk".to_string(), hk);
    let mut ck = BTreeMap::new();
    ck.insert(
        "mutual".to_string(),
        CycleKindSpec {
            label: Some("Mutual".into()),
            description: Some("why-cyc".into()),
            remediation: Some("fix-cyc".into()),
        },
    );

    // A threshold id resolves to its metric's node-attribute spec.
    let m = rule_doc("threshold.file.hk", "rust", &na, &ck).expect("metric doc");
    assert_eq!(m.title.as_deref(), Some("Henry–Kafura"));
    assert_eq!(m.why.as_deref(), Some("why-hk"));
    assert_eq!(m.fix.as_deref(), Some("fix-hk"));
    // A cycle id resolves to the cycle-kind spec.
    let c = rule_doc("cycle.mutual", "rust", &na, &ck).expect("cycle doc");
    assert_eq!(c.why.as_deref(), Some("why-cyc"));
    assert_eq!(c.fix.as_deref(), Some("fix-cyc"));
    // An unknown metric has no spec → no doc.
    assert!(rule_doc("threshold.file.bogus", "rust", &na, &ck).is_none());
}

#[test]
fn rule_doc_auto_derives_fix_for_a_metric_without_remediation() {
    use code_ranker_plugin_api::attrs::ValueType;
    let mut na = BTreeMap::new();
    // A built-in metric carries no boilerplate `remediation`; the `fix` line is
    // derived from the key as a pointer to its `docs` page.
    na.insert(
        "sloc".to_string(),
        AttributeSpec::new(ValueType::Int, "Source"),
    );
    let ck = BTreeMap::new();
    let m = rule_doc("threshold.file.sloc", "rust", &na, &ck).expect("metric doc");
    assert_eq!(
        m.fix.as_deref(),
        Some("Run `code-ranker docs rust sloc` and follow its instructions.")
    );
}

#[test]
fn rule_group_resolves_threshold_and_cycle_ids() {
    assert_eq!(rule_group("threshold.file.sloc"), "SIZ");
    assert_eq!(rule_group("threshold.file.cyclomatic"), "CPX");
    assert_eq!(rule_group("threshold.file.hk"), "CPL");
    assert_eq!(rule_group("cycle.mutual"), "CYC");
    assert_eq!(rule_group("threshold.file.bogus"), "?");
}

#[test]
fn apply_cycle_rules_strips_disabled_kind() {
    use crate::config::model::CycleRule;
    let mut cycles = vec![CycleGroup {
        kind: "mutual".into(),
        nodes: vec!["a".into(), "b".into()],
    }];
    let mut nodes: Vec<Node> = vec![];
    // A kind whose budget is disabled is stripped from the groups.
    let rules = CycleRules {
        mutual: CycleRule::Off,
        chain: CycleRule::Max(0),
    };
    apply_cycle_rules(&mut cycles, &mut nodes, &rules);
    assert!(cycles.is_empty(), "disabled kind -> stripped");
}

#[test]
fn apply_cycle_rules_clears_disabled_cycle_attr_on_nodes() {
    use crate::config::model::CycleRule;
    let mut cycles: Vec<CycleGroup> = vec![];
    let node = |id: &str, kind: &str| Node {
        id: id.into(),
        kind: "file".into(),
        name: id.into(),
        parent: None,
        attrs: [("cycle".to_string(), AttrValue::Str(kind.into()))]
            .into_iter()
            .collect(),
    };
    // `mutual` is disabled, `chain` keeps a budget — only the mutual node's
    // `cycle` attribute is cleared.
    let mut nodes = vec![node("a", "mutual"), node("b", "chain")];
    let rules = CycleRules {
        mutual: CycleRule::Off,
        chain: CycleRule::Max(3),
    };
    apply_cycle_rules(&mut cycles, &mut nodes, &rules);
    assert!(
        !nodes[0].attrs.contains_key("cycle"),
        "disabled-kind cycle attr cleared"
    );
    assert!(
        nodes[1].attrs.contains_key("cycle"),
        "an enabled kind's attr is kept"
    );
}

#[test]
fn rule_tuning_emits_cli_and_config_hints() {
    assert!(rule_tuning("cycle.mutual", "rust").contains("plugins.rust.rules.cycles.mutual=off"));
    assert!(
        rule_tuning("threshold.file.hk", "rust")
            .contains("plugins.rust.rules.thresholds.file.hk=N")
    );
    assert_eq!(rule_tuning("bogus.id", "rust"), "");
}
