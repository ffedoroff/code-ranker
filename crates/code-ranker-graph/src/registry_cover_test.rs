use super::*;
use code_ranker_plugin_api::attrs::AttrValue;

#[test]
fn registry_error_display() {
    let p = RegistryError::Parse {
        key: "x".into(),
        message: "boom".into(),
    };
    assert!(format!("{p}").contains("x") && format!("{p}").contains("boom"));
    let c = RegistryError::Cycle {
        keys: vec!["a".into(), "b".into()],
    };
    assert!(format!("{c}").contains("a → b"));
}

#[test]
fn reducers_and_percentile_edges() {
    assert_eq!(reduce(&[3.0, 1.0, 2.0], "min"), Some(1.0));
    assert_eq!(reduce(&[3.0, 1.0, 2.0], "max"), Some(3.0));
    assert_eq!(reduce(&[1.0, 2.0], "unknown_reducer"), None);
    assert_eq!(reduce(&[], "avg"), None);
    assert_eq!(percentile(&[], 50.0), None);
}

#[test]
fn top_n_reducer_keeps_largest_then_reduces() {
    let vals = [1.0, 5.0, 3.0, 9.0, 2.0, 8.0]; // top 3 = 9, 8, 5
    assert_eq!(reduce(&vals, "top3_avg"), Some((9.0 + 8.0 + 5.0) / 3.0));
    assert_eq!(reduce(&vals, "top3_max"), Some(9.0));
    assert_eq!(reduce(&vals, "top2_sum"), Some(17.0));
    assert_eq!(reduce(&vals, "top10"), reduce(&vals, "avg")); // N ≥ len, default avg
    assert_eq!(reduce(&vals, "top"), None); // no number → not a top reducer
}

#[test]
fn exec_f64_handles_int_result() {
    // an int-literal formula yields a CEL Int → coerced to f64.
    let mut defs = BTreeMap::new();
    defs.insert(
        "two".to_string(),
        MetricDef {
            formula: "1 + 1".to_string(),
            value_type: "int".to_string(),
            ..Default::default()
        },
    );
    let eng = Engine::compile(&defs).unwrap();
    assert_eq!(
        eng.eval_node(&BTreeMap::new(), &BTreeMap::new()).get("two"),
        Some(&2.0)
    );
}

#[test]
fn to_attribute_spec_maps_types_and_direction() {
    let mk = |vt: &str, dir: Option<&str>| MetricDef {
        formula: "0.0".to_string(),
        value_type: vt.to_string(),
        label: Some("L".into()),
        direction: dir.map(|s| s.to_string()),
        ..Default::default()
    };
    use code_ranker_plugin_api::attrs::ValueType;
    use code_ranker_plugin_api::level::Direction;
    assert_eq!(
        mk("int", None).to_attribute_spec().value_type,
        ValueType::Int
    );
    assert_eq!(
        mk("bool", None).to_attribute_spec().value_type,
        ValueType::Bool
    );
    assert_eq!(
        mk("str", None).to_attribute_spec().value_type,
        ValueType::Str
    );
    assert_eq!(
        mk("float", Some("higher_better"))
            .to_attribute_spec()
            .direction,
        Direction::HigherBetter
    );
    // unknown/absent direction → Neutral
    assert_eq!(
        mk("float", None).to_attribute_spec().direction,
        Direction::Neutral
    );
}

#[test]
fn two_tier_thresholds_map_to_spec() {
    let with = |warning, info| MetricDef {
        formula: "0.0".to_string(),
        warning,
        info,
        ..Default::default()
    };
    // No tiers → no thresholds.
    assert!(with(None, None).to_attribute_spec().thresholds.is_none());
    // One tier mirrors into the other.
    let th = with(Some(1.5), None)
        .to_attribute_spec()
        .thresholds
        .unwrap();
    assert_eq!((th.warning, th.info), (1.5, 1.5));
    // Both tiers preserved.
    let th = with(Some(2.0), Some(1.0))
        .to_attribute_spec()
        .thresholds
        .unwrap();
    assert_eq!((th.warning, th.info), (2.0, 1.0));
}

#[test]
fn calc_defaults_to_formula_for_node_scope() {
    // Node-scope: `calc` (the live derivation line) defaults to the CEL formula.
    let node = MetricDef {
        formula: "tloc / sloc".to_string(),
        ..Default::default()
    };
    assert_eq!(
        node.to_attribute_spec().calc.as_deref(),
        Some("tloc / sloc")
    );
    // Explicit `calc` wins over the formula fallback.
    let explicit = MetricDef {
        formula: "tloc / sloc".to_string(),
        calc: Some("tloc / sloc * 1.0".to_string()),
        ..Default::default()
    };
    assert_eq!(
        explicit.to_attribute_spec().calc.as_deref(),
        Some("tloc / sloc * 1.0")
    );
    // Graph-scope aggregate isn't shown per node → no calc.
    let agg = MetricDef {
        formula: "agg('x', 'avg', 'not_empty')".to_string(),
        scope: Scope::Graph,
        ..Default::default()
    };
    assert!(agg.to_attribute_spec().calc.is_none());
}

#[test]
fn apply_to_node_writes_and_omits() {
    let mut defs = BTreeMap::new();
    defs.insert("ratio".to_string(), {
        let mut d = MetricDef {
            formula: "a * 2.0".to_string(),
            value_type: "float".to_string(),
            ..Default::default()
        };
        d.formula = "a * 2.0".to_string();
        d
    });
    let eng = Engine::compile(&defs).unwrap();
    let mut node = Node {
        id: "n".into(),
        kind: "file".into(),
        name: "n".into(),
        parent: None,
        attrs: Default::default(),
    };
    node.attrs.insert("a".into(), AttrValue::Int(3));
    // pre-seed `ratio` so the omit branch (result == omit_at) can remove it.
    node.attrs.insert("ratio".into(), AttrValue::Int(99));
    apply_to_node(&mut node, &defs, &eng);
    assert_eq!(node.attrs.get("ratio"), Some(&AttrValue::Int(6)));

    // now a formula that yields the omit value removes the attr.
    let mut zdefs = BTreeMap::new();
    zdefs.insert(
        "ratio".to_string(),
        MetricDef {
            formula: "0.0".to_string(),
            value_type: "float".to_string(),
            ..Default::default()
        },
    );
    let zeng = Engine::compile(&zdefs).unwrap();
    apply_to_node(&mut node, &zdefs, &zeng);
    assert!(!node.attrs.contains_key("ratio"));
}

#[test]
fn apply_to_node_exposes_path_to_formula() {
    // A metric can branch on the file's path; bool attrs are ignored as inputs.
    let mut defs = BTreeMap::new();
    defs.insert(
        "gated".to_string(),
        MetricDef {
            formula: r#"path.contains("/generated/") ? 0.0 : 5.0"#.to_string(),
            value_type: "float".to_string(),
            ..Default::default()
        },
    );
    let eng = Engine::compile(&defs).unwrap();
    let mut node = Node {
        id: "n".into(),
        kind: "file".into(),
        name: "n".into(),
        parent: None,
        attrs: Default::default(),
    };
    node.attrs
        .insert("path".into(), AttrValue::Str("src/lib.rs".into()));
    node.attrs.insert("flag".into(), AttrValue::Bool(true)); // ignored input
    apply_to_node(&mut node, &defs, &eng);
    // `num_attr` normalizes a whole float to Int.
    assert_eq!(node.attrs.get("gated"), Some(&AttrValue::Int(5)));

    node.attrs
        .insert("path".into(), AttrValue::Str("src/generated/api.rs".into()));
    apply_to_node(&mut node, &defs, &eng);
    // In `/generated/` the metric is 0 → omitted.
    assert!(!node.attrs.contains_key("gated"));
}
