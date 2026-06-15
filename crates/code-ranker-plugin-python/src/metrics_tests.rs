//! Python metric-correctness tests (ported from `code-ranker-complexity`).
//!
//! These exercise the `python_ts` engine through `write_metrics`, asserting the
//! AST-Accurate principle for Python: control-flow / exit keywords appearing only
//! as look-alikes (comments, strings, docstrings) must NOT move the per-function
//! metrics (no false positive), while real Python constructs MUST be counted
//! (no false negative). See docs/metric-correctness.md.

// Per-language keyword look-alike guard set — the construct keywords a complexity
// metric can key on. The FP matrix injects these *only* as look-alikes and asserts
// no metric moves. Mirrors the "Keyword look-alike guard set" in
// principles/python/metrics.md, and `python_trigger_set_documented_in_spec`
// asserts the spec documents every entry — so the two cannot drift.
const PY_TRIGGERS: &[&str] = &[
    "if", "elif", "else", "while", "for", "and", "or", "return", "try", "except", "with", "assert",
    "raise",
];

/// Read one integer/float metric off a node's attrs.
fn metric(node: &code_ranker_plugin_api::node::Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => Some(*v as f64),
        Some(code_ranker_plugin_api::attrs::AttrValue::Float(v)) => Some(*v),
        _ => None,
    }
}

/// Parse `src` as Python via the `python_ts` engine, write the metrics onto a
/// file node, and read one metric — the in-process building block for the
/// metamorphic tests below.
fn metric_of(src: &str, key: &str) -> Option<f64> {
    let m = crate::python_ts::compute(src.as_bytes())?;
    let mut node = code_ranker_plugin_api::node::Node {
        id: "t.py".into(),
        kind: "file".into(),
        name: "t.py".into(),
        parent: None,
        attrs: Default::default(),
    };
    code_ranker_graph::write_metrics(&mut node, &m);
    metric(&node, key)
}

#[test]
fn python_complexity_fp_matrix() {
    // FP invariance for cyclomatic / cognitive, driven by Python's documented
    // trigger set injected into comment / string / docstring positions. None may
    // change the per-function metrics vs the real base.
    let kw = PY_TRIGGERS.join(" ");
    let base = "def f(x):\n    if x > 0:\n        return 1\n    return 2\n";
    let traps: &[String] = &[
        format!("# {kw}\n{base}"),
        format!("def f(x):\n    s = \"{kw}\"\n    if x > 0:\n        return 1\n    return 2\n"),
        format!("def f(x):\n    \"\"\"{kw}\"\"\"\n    if x > 0:\n        return 1\n    return 2\n"),
    ];
    for key in ["cyclomatic", "cognitive"] {
        let want = metric_of(base, key);
        for trap in traps {
            assert_eq!(
                metric_of(trap, key),
                want,
                "t.py metric `{key}` moved on a keyword look-alike"
            );
        }
    }
}

#[test]
fn python_trigger_set_documented_in_spec() {
    // Lock-step guard: every keyword the FP matrix injects must be documented in
    // the Python metrics spec, so the trigger list and the spec's "Keyword
    // look-alike guard set" cannot drift apart.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let path = format!("{root}/principles/python/metrics.md");
    let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    for kw in PY_TRIGGERS {
        assert!(
            spec.contains(&format!("`{kw}`")),
            "trigger `{kw}` is not documented in principles/python/metrics.md \
             — spec and FP test drifted"
        );
    }
}

#[test]
fn python_branches_raise_cyclomatic() {
    // FN guard: a real `if` branch must raise cyclomatic above a branch-free
    // baseline.
    let baseline =
        metric_of("def f():\n    return 0\n", "cyclomatic").expect("baseline cyclomatic present");
    let branched = metric_of(
        "def f(x):\n    if x > 0:\n        return 1\n    return 2\n",
        "cyclomatic",
    )
    .expect("branched cyclomatic present");
    assert!(
        branched > baseline,
        "a real `if` must raise cyclomatic (branched {branched} <= baseline {baseline})"
    );
}

#[test]
fn python_args_counted() {
    // FN guard: real function parameters must surface the `args` metric.
    let args = metric_of("def f(a, b, c):\n    return a + b + c\n", "args").expect("args present");
    assert!(
        args >= 3.0,
        "three parameters must count as >=3 args, got {args}"
    );
}

#[test]
fn python_loop_else_counts_as_a_branch() {
    // A `for`/`while` may carry an `else:` that runs when the loop completes
    // without `break` — the analyzer counts it as a branch (unlike an `if`'s
    // `else`). So the loop-else form has cyclomatic one above the plain loop.
    let with_else = "def f(xs):\n    for x in xs:\n        return x\n    else:\n        return 0\n";
    let without = "def f(xs):\n    for x in xs:\n        return x\n    return 0\n";
    let c1 = metric_of(with_else, "cyclomatic").expect("cyclomatic present");
    let c0 = metric_of(without, "cyclomatic").expect("cyclomatic present");
    assert!(
        c1 > c0,
        "for…else must add a branch (got {c1} with else vs {c0} without)"
    );
}
