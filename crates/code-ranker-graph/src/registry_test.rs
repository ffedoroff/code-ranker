use super::*;

fn def(formula: &str) -> MetricDef {
    MetricDef {
        formula: formula.to_string(),
        value_type: "float".to_string(),
        ..Default::default()
    }
}

#[test]
fn computes_a_simple_ratio() {
    let mut defs = BTreeMap::new();
    defs.insert(
        "comment_ratio".to_string(),
        def("sloc > 0.0 ? cloc / sloc * 100.0 : 0.0"),
    );
    let eng = Engine::compile(&defs).unwrap();
    let attrs = BTreeMap::from([("sloc".to_string(), 40.0), ("cloc".to_string(), 10.0)]);
    let out = eng.eval_node(&attrs, &BTreeMap::new());
    assert_eq!(out.get("comment_ratio"), Some(&25.0));
}

#[test]
fn math_functions_match_rust_f64() {
    let mut defs = BTreeMap::new();
    // volume = length * log2(vocabulary) — same op as the built-in engine.
    defs.insert(
        "volume".to_string(),
        def("vocabulary > 0.0 ? length * log2(vocabulary) : 0.0"),
    );
    let eng = Engine::compile(&defs).unwrap();
    let attrs = BTreeMap::from([
        ("length".to_string(), 87.0),
        ("vocabulary".to_string(), 23.0),
    ]);
    let out = eng.eval_node(&attrs, &BTreeMap::new());
    let expect = 87.0_f64 * 23.0_f64.log2();
    assert_eq!(out.get("volume"), Some(&expect));
}

#[test]
fn dependent_metrics_evaluate_in_order() {
    let mut defs = BTreeMap::new();
    defs.insert("length".to_string(), def("n1 + n2"));
    defs.insert("double_len".to_string(), def("length * 2.0"));
    let eng = Engine::compile(&defs).unwrap();
    let attrs = BTreeMap::from([("n1".to_string(), 3.0), ("n2".to_string(), 4.0)]);
    let out = eng.eval_node(&attrs, &BTreeMap::new());
    assert_eq!(out.get("length"), Some(&7.0));
    assert_eq!(out.get("double_len"), Some(&14.0));
}

#[test]
fn detects_dependency_cycle() {
    let mut defs = BTreeMap::new();
    defs.insert("a".to_string(), def("b + 1.0"));
    defs.insert("b".to_string(), def("a + 1.0"));
    assert!(matches!(
        Engine::compile(&defs),
        Err(RegistryError::Cycle { .. })
    ));
}

#[test]
fn invalid_formula_is_a_load_error() {
    let mut defs = BTreeMap::new();
    defs.insert("bad".to_string(), def("1 +"));
    assert!(matches!(
        Engine::compile(&defs),
        Err(RegistryError::Parse { .. })
    ));
}

fn graph_def(formula: &str) -> MetricDef {
    let mut d = def(formula);
    d.scope = Scope::Graph;
    d
}

fn rows(key: &str, vals: &[f64]) -> Vec<BTreeMap<String, f64>> {
    vals.iter()
        .map(|v| BTreeMap::from([(key.to_string(), *v)]))
        .collect()
}

#[test]
fn percentile_matches_numpy_r7() {
    // numpy.percentile([10,20,30,100], 50) == 25.0 (linear interpolation).
    assert_eq!(percentile(&[10.0, 20.0, 30.0, 100.0], 50.0), Some(25.0));
    // p0 = min, p100 = max.
    assert_eq!(percentile(&[5.0, 1.0, 9.0], 0.0), Some(1.0));
    assert_eq!(percentile(&[5.0, 1.0, 9.0], 100.0), Some(9.0));
    // single element → itself for any q.
    assert_eq!(percentile(&[7.0], 90.0), Some(7.0));
}

#[test]
fn graph_aggregate_over_population() {
    let mut defs = BTreeMap::new();
    defs.insert(
        "cyc_p90".to_string(),
        graph_def("agg('cyclomatic', 'p90', 'not_empty')"),
    );
    defs.insert(
        "cyc_mean".to_string(),
        graph_def("agg('cyclomatic', 'avg', 'not_empty')"),
    );
    let eng = Engine::compile(&defs).unwrap();
    assert!(eng.has_graph_metrics());
    let r = rows("cyclomatic", &[2.0, 4.0, 6.0, 8.0, 10.0]);
    let pops = Populations::build(&r, &["cyclomatic".to_string()], &BTreeMap::new());
    let out = eng.eval_graph(&pops);
    assert_eq!(out.get("cyc_mean"), Some(&6.0));
    // p90 of [2,4,6,8,10] (R-7): h=(5-1)*0.9=3.6 → 8 + 0.6*(10-8) = 9.2
    assert_eq!(out.get("cyc_p90"), Some(&9.2));
}

#[test]
fn all_population_counts_missing_at_floor() {
    // 2 nodes have hk, 3 don't → `all` includes 3 zeros; `not_empty` only the 2.
    let mut r = rows("hk", &[100.0, 300.0]);
    r.push(BTreeMap::new());
    r.push(BTreeMap::new());
    r.push(BTreeMap::new());
    let pops = Populations::build(&r, &["hk".to_string()], &BTreeMap::new());
    let mut defs = BTreeMap::new();
    defs.insert(
        "hk_med_all".to_string(),
        graph_def("agg('hk','median','all')"),
    );
    defs.insert(
        "hk_med_ne".to_string(),
        graph_def("agg('hk','median','not_empty')"),
    );
    let out = Engine::compile(&defs).unwrap().eval_graph(&pops);
    // all = [0,0,0,100,300] → median 0; not_empty = [100,300] → median 200
    assert_eq!(out.get("hk_med_all"), Some(&0.0));
    assert_eq!(out.get("hk_med_ne"), Some(&200.0));
}

#[test]
fn graph_metrics_compose() {
    // a ratio of two aggregates (graph metric referencing another graph metric).
    let mut defs = BTreeMap::new();
    defs.insert("total".to_string(), graph_def("agg('x','sum','not_empty')"));
    defs.insert("n".to_string(), graph_def("agg('x','count','not_empty')"));
    defs.insert("ratio".to_string(), graph_def("n > 0.0 ? total / n : 0.0"));
    let eng = Engine::compile(&defs).unwrap();
    let pops = Populations::build(
        &rows("x", &[2.0, 4.0, 6.0]),
        &["x".to_string()],
        &BTreeMap::new(),
    );
    let out = eng.eval_graph(&pops);
    assert_eq!(out.get("total"), Some(&12.0));
    assert_eq!(out.get("ratio"), Some(&4.0));
}

#[test]
fn error_or_nonfinite_is_omitted() {
    let mut defs = BTreeMap::new();
    // references a missing variable → execution error → omitted, no panic.
    defs.insert("x".to_string(), def("missing_var + 1.0"));
    let eng = Engine::compile(&defs).unwrap();
    let out = eng.eval_node(&BTreeMap::new(), &BTreeMap::new());
    assert!(!out.contains_key("x"));
}
