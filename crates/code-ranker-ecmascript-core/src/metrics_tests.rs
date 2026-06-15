//! ECMAScript (JS / TS / TSX) metric-correctness tests.
//!
//! Layer-1 metamorphic FP guards plus a spec lock-step check for the shared
//! ECMAScript metric engine (`crate::ecmascript_ts::compute`). These previously
//! lived in `code-ranker-complexity`; they moved here when the engine moved.
//!
//! AST-Accurate principle: a control-flow / exit keyword that appears only as a
//! look-alike (in a comment, a string, or a template literal) must NOT move the
//! per-file `cyclomatic` / `cognitive` metrics.

// Per-language keyword look-alike guard set — the construct keywords/operators a
// complexity metric can key on. The FP matrix injects these *only* as
// look-alikes and asserts no metric moves. Mirrors the "Keyword look-alike guard
// set" in principles/typescript/metrics.md, and
// `typescript_trigger_set_documented_in_spec` asserts the spec documents every
// entry — so the two cannot drift. A superset of the analyzer's real triggers is
// fine.
const TS_TRIGGERS: &[&str] = &[
    "if", "else", "while", "for", "do", "switch", "case", "catch", "return", "throw", "&&", "||",
    "??", "?",
];

fn metric(node: &code_ranker_plugin_api::node::Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => Some(*v as f64),
        Some(code_ranker_plugin_api::attrs::AttrValue::Float(v)) => Some(*v),
        _ => None,
    }
}

/// Parse `src` through the ECMAScript engine with the given grammar and read one
/// metric — the in-process building block for the metamorphic tests below.
fn metric_of(src: &str, lang: &tree_sitter::Language, else_if: bool, key: &str) -> Option<f64> {
    let m = crate::ecmascript_ts::compute(src.as_bytes(), lang, else_if)?;
    let mut node = code_ranker_plugin_api::node::Node {
        id: "t".into(),
        kind: "file".into(),
        name: "t".into(),
        parent: None,
        attrs: Default::default(),
    };
    code_ranker_graph::write_metrics(&mut node, &m);
    metric(&node, key)
}

#[test]
fn ts_complexity_fp_matrix() {
    // FP invariance for cyclomatic / cognitive: the documented TS trigger set
    // injected into comment / string / template positions must not move either
    // metric. TypeScript grammar; `else if` is collapsed via the else clause.
    let ts: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let kw = TS_TRIGGERS.join(" ");
    let base = "export function f(x: number): number { if (x > 0) { return 1; } return 2; }\n";
    let traps = [
        format!("// {kw}\n{base}"),
        format!(
            "export function f(x: number): number {{ const s: string = \"{kw}\"; void s; if (x > 0) {{ return 1; }} return 2; }}\n"
        ),
        format!(
            "export function f(x: number): number {{ const s = `{kw}`; void s; if (x > 0) {{ return 1; }} return 2; }}\n"
        ),
    ];

    for key in ["cyclomatic", "cognitive"] {
        let want = metric_of(base, &ts, true, key);
        for trap in &traps {
            assert_eq!(
                metric_of(trap, &ts, true, key),
                want,
                "ts metric `{key}` moved on a keyword look-alike"
            );
        }
    }
}

#[test]
fn js_complexity_fp_matrix() {
    // Same FP invariance for JavaScript. JavaScript grammar; `else if` is not
    // collapsed via the else clause.
    let js: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    let kw = TS_TRIGGERS.join(" ");
    let base = "export function f(x) { if (x > 0) { return 1; } return 2; }\n";
    let traps = [
        format!("// {kw}\n{base}"),
        format!(
            "export function f(x) {{ const s = \"{kw}\"; void s; if (x > 0) {{ return 1; }} return 2; }}\n"
        ),
        format!(
            "export function f(x) {{ const s = `{kw}`; void s; if (x > 0) {{ return 1; }} return 2; }}\n"
        ),
    ];

    for key in ["cyclomatic", "cognitive"] {
        let want = metric_of(base, &js, false, key);
        for trap in &traps {
            assert_eq!(
                metric_of(trap, &js, false, key),
                want,
                "js metric `{key}` moved on a keyword look-alike"
            );
        }
    }
}

#[test]
fn tsx_file_metrics_computed() {
    // The TSX grammar arm must actually compute metrics: a function with one
    // real branch yields cyclomatic above the branch-free baseline.
    let tsx: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    let baseline = metric_of(
        "export function f(x: number): number { return x; }\n",
        &tsx,
        false,
        "cyclomatic",
    )
    .expect("tsx cyclomatic present for branch-free function");

    let branched = metric_of(
        "export function f(x: number): number { if (x > 0) { return 1; } return 2; }\n",
        &tsx,
        false,
        "cyclomatic",
    )
    .expect("tsx cyclomatic present for branched function");

    assert!(
        branched > baseline,
        "a real branch must raise tsx cyclomatic above the branch-free baseline ({branched} <= {baseline})"
    );
}

#[test]
fn typescript_trigger_set_documented_in_spec() {
    // Lock-step guard: every keyword the FP matrix injects must be documented in
    // the TypeScript metrics spec, so the trigger list and the spec's "Keyword
    // look-alike guard set" cannot drift apart.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let path = format!("{root}/principles/typescript/metrics.md");
    let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    for kw in TS_TRIGGERS {
        assert!(
            spec.contains(&format!("`{kw}`")),
            "trigger `{kw}` is not documented in principles/typescript/metrics.md — spec and FP test drifted"
        );
    }
}
