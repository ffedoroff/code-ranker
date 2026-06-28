use super::*;

/// `compute_functions` classifies the function-like forms (declaration /
/// method / arrow / generator) and counts branches (covers collect_functions
/// / unit_for / the kind mapping).
#[test]
fn compute_functions_covers_forms() {
    let lang: Language = tree_sitter_javascript::LANGUAGE.into();
    let src = b"function f(x){ if (x) return 1; return 0; }\n\
                const g = (y) => y + 1;\n\
                class C { m(z){ return z; } }\n\
                function* gen(){ yield 1; }\n";
    let units = compute_functions(src, &lang, false);
    assert!(
        units.iter().any(|u| u.name == "f" && u.kind == "function"),
        "function declaration f"
    );
    assert!(
        units.iter().any(|u| u.name == "m" && u.kind == "method"),
        "method m"
    );
    assert!(units.iter().any(|u| u.kind == "arrow"), "arrow fn");
    assert!(units.iter().any(|u| u.kind == "generator"), "generator");
    let f = units.iter().find(|u| u.name == "f").unwrap();
    assert!(f.inputs.branches >= 1.0, "f has an `if` branch");
}

#[test]
fn compute_functions_empty_on_no_functions() {
    let lang: Language = tree_sitter_javascript::LANGUAGE.into();
    assert!(compute_functions(b"const x = 1;\n", &lang, false).is_empty());
}

/// The `ecmascript.toml` `[roles]`/`[halstead]`/`[loc]` sections parse and
/// deserialize into the engine's `RoleCfg` (forces the `ROLE_CFG` LazyLock +
/// `try_into`, which would panic on a bad config) and carry the expected
/// node-kind strings.
#[test]
fn ecmascript_toml_kinds_load() {
    let c = &*ROLE_CFG;
    assert!(
        c.roles
            .space_kinds
            .named
            .contains(&"arrow_function".to_string())
    );
    // `function_declaration` / `method_definition` / … are identity roles
    // (name == named grammar kind), resolved by the engine directly, so
    // `[roles.one]` carries no entries for ECMAScript.
    assert!(c.roles.one.is_empty());
    assert!(c.roles.branch_kinds.anon.contains(&"&&".to_string()));
    assert!(
        c.roles
            .branch_kinds
            .named
            .contains(&"ternary_expression".to_string())
    );
    assert_eq!(c.roles.non_arg_kinds.anon, ["(", ")", ","]);
    assert!(c.halstead.operators.named.contains(&"import".to_string()));
    assert!(c.halstead.operands.anon.contains(&"typeof".to_string()));
    assert_eq!(c.loc.noop_kinds.anon, ["\""]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Metric-correctness tests — ECMAScript (JS / TS / TSX).
//
// Layer-1 metamorphic FP guards plus a spec lock-step check for the shared
// ECMAScript metric engine (`crate::languages::ecmascript::dialect::compute`).
// These previously lived in `code-ranker-complexity`; they moved here when the
// engine moved.
//
// AST-Accurate principle: a control-flow / exit keyword that appears only as a
// look-alike (in a comment, a string, or a template literal) must NOT move the
// per-file `cyclomatic` / `cognitive` metrics.
// ─────────────────────────────────────────────────────────────────────────────

// Per-language keyword look-alike guard set — the construct keywords/operators a
// complexity metric can key on. The FP matrix injects these *only* as
// look-alikes and asserts no metric moves. Mirrors the "Keyword look-alike guard
// set" in languages/typescript/metrics.md, and
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
    let m = crate::languages::ecmascript::dialect::compute(src.as_bytes(), lang, else_if)?;
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
    let path = format!("{root}/plugins/ts/metrics.md");
    let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    for kw in TS_TRIGGERS {
        assert!(
            spec.contains(&format!("`{kw}`")),
            "trigger `{kw}` is not documented in plugins/ts/metrics.md — spec and FP test drifted"
        );
    }
}

#[test]
fn js_generator_function_counts_as_closure() {
    // A generator (`function* …`) is classified as a closure by the analyzer.
    let js: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    let src = "export function* gen() { yield 1; yield 2; }\n";
    let closures = metric_of(src, &js, false, "closures").expect("closures present");
    assert!(
        closures >= 1.0,
        "a generator function must count as a closure, got {closures}"
    );
}

#[test]
fn js_function_and_arrow_classification_branches() {
    // Exercise the func-vs-closure classifier across the forms it special-cases:
    // a named function declaration, a function EXPRESSION assigned to a binding
    // (check_if_func via assign-ancestor), an anonymous function expression as a
    // callback (a closure), an arrow assigned to a variable and one assigned to an
    // object property (check_if_arrow_func via assign-ancestor / property sibling),
    // and an object method shorthand. Running the engine over all of them covers
    // is_func / is_closure / check_if_func / check_if_arrow_func / is_child /
    // has_sibling; the anonymous callback + bare arrows make `closures` non-zero.
    let js: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    let src = "function decl() { return 1; }\n\
const fe = function named() { return 2; };\n\
const arrow = (x) => x + 1;\n\
const obj = { method() { return 3; }, prop: (y) => y * 2 };\n\
export function run(xs) {\n  \
return xs.map(function (x) { return x + 1; }).map((z) => z - 1);\n}\n\
void [decl, fe, arrow, obj];\n";
    let closures = metric_of(src, &js, false, "closures").expect("closures present");
    assert!(
        closures >= 1.0,
        "anonymous callbacks / bare arrows must count as closures, got {closures}"
    );
}
