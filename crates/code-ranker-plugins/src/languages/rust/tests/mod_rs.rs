use super::*;
use code_ranker_plugin_api::attrs::AttrValue;
use internal::NodeKind;

fn strip(src: &str) -> String {
    String::from_utf8(strip_cfg_test(src.as_bytes()).0).unwrap()
}

#[test]
fn function_units_extracts_fns_and_methods() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("a.rs");
    std::fs::write(
        &f,
        "fn add(a: i32, b: i32) -> i32 { if a > 0 { return a + b; } b }\n\
             struct S;\n\
             impl S { fn m(&self) -> i32 { 1 } }\n\
             #[cfg(test)]\n\
             mod tests { fn helper() -> i32 { 0 } }\n",
    )
    .unwrap();
    let graph = Graph {
        nodes: vec![Node {
            id: f.to_string_lossy().into_owned(),
            kind: "file".into(),
            name: "a.rs".into(),
            parent: None,
            attrs: Default::default(),
        }],
        edges: vec![],
    };
    // The plugin now returns (node, inputs) pairs (the orchestrator writes the
    // metrics); this test only checks the node structure.
    let cfg = RustPlugin.config();
    let units: Vec<_> = RustPlugin
        .function_units(&cfg, &graph)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(
        units.iter().any(|n| n.name == "add" && n.kind == "fn"),
        "fn add: {:?}",
        units.iter().map(|n| (&n.name, &n.kind)).collect::<Vec<_>>()
    );
    assert!(units.iter().any(|n| n.name == "m" && n.kind == "method"));
    // `#[cfg(test)]` helper is stripped before the per-function walk.
    assert!(
        !units.iter().any(|n| n.name == "helper"),
        "test fn excluded"
    );
    assert!(units.iter().all(|n| n.parent.is_some()));
}

/// Build a `Module` internal node for one file, with structural attrs.
/// `line` distinguishes an inline module (`Some`) from a file-backed one
/// (`None`); `collapse_to_files` lets the file-backed node win.
#[allow(clippy::too_many_arguments)]
fn module_node(
    id: &str,
    path: &str,
    line: Option<u32>,
    visibility: internal::Visibility,
    loc: u32,
    items: u32,
    unsafe_count: u32,
    krate: &str,
) -> internal::Node {
    internal::Node {
        id: id.into(),
        kind: NodeKind::Module,
        name: id.into(),
        path: path.into(),
        parent: None,
        external: None,
        version: None,
        visibility: Some(visibility),
        loc: Some(loc),
        line,
        item_count: Some(items),
        unsafe_count: Some(unsafe_count),
        crate_label: Some(krate.into()),
        facts: Default::default(),
    }
}

#[test]
fn collapse_lets_the_file_backed_module_overwrite_structural_attrs() {
    // Two modules map to one file id (same `path`): an inline module
    // (`line = Some`) is seen first and seeds the file node, then the
    // file-backed module (`line = None`) is the source of truth and must
    // overwrite every structural attr (visibility / loc / items / unsafe /
    // crate). This exercises the Occupied-entry update branch of
    // `collapse_to_files`.
    let mut builder = GraphBuilder::new();
    builder.add_node(module_node(
        "inline",
        "/x/foo.rs",
        Some(5),
        internal::Visibility::Private,
        1,
        1,
        0,
        "wrong-crate",
    ));
    builder.add_node(module_node(
        "file",
        "/x/foo.rs",
        None,
        internal::Visibility::Public,
        42,
        7,
        3,
        "mycrate",
    ));

    let graph = collapse_to_files(builder.build());

    let file = graph
        .nodes
        .iter()
        .find(|n| n.id == "/x/foo.rs")
        .expect("the two modules collapsed into one file node");
    assert_eq!(file.kind, "file");
    assert_eq!(
        file.attrs.get("visibility"),
        Some(&AttrValue::Str("public".into())),
        "file-backed visibility wins"
    );
    assert_eq!(
        file.attrs.get("loc"),
        Some(&AttrValue::Int(42)),
        "file-backed loc wins"
    );
    assert_eq!(
        file.attrs.get("items"),
        Some(&AttrValue::Int(7)),
        "file-backed item count wins"
    );
    assert_eq!(
        file.attrs.get("unsafe"),
        Some(&AttrValue::Int(3)),
        "file-backed unsafe count wins (and is non-zero so it is kept)"
    );
    assert_eq!(
        file.attrs.get("crate"),
        Some(&AttrValue::Str("mycrate".into())),
        "file-backed crate label wins"
    );
}

#[test]
fn strips_cfg_test_module_with_its_attribute() {
    let out = strip(
        "pub fn prod() -> i32 {\n    1\n}\n\n\
             #[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t() { assert_eq!(prod(), 1); }\n}\n",
    );
    assert!(out.contains("pub fn prod"), "production kept: {out}");
    assert!(!out.contains("mod tests"), "test mod removed: {out}");
    assert!(
        !out.contains("#[cfg(test)]"),
        "the cfg attr line removed too: {out}"
    );
    assert!(!out.contains("fn t()"), "test fn removed: {out}");
}

#[test]
fn strips_standalone_test_and_bench_fns() {
    let out = strip("fn prod() {}\n#[test]\nfn it_works() {}\n#[bench]\nfn b(_: &mut ()) {}\n");
    assert!(out.contains("fn prod"));
    assert!(
        !out.contains("it_works") && !out.contains("fn b("),
        "test/bench fns removed: {out}"
    );
}

#[test]
fn keeps_non_test_cfg_and_similarly_named_items() {
    // `cfg(feature = "test")` is a string literal, not a `test` ident; a
    // `mod tests_data` is not gated. Both stay.
    let out = strip("#[cfg(feature = \"test\")]\npub mod gated {}\npub mod tests_data {}\n");
    assert!(out.contains("pub mod gated"), "feature-cfg kept: {out}");
    assert!(
        out.contains("tests_data"),
        "non-gated lookalike kept: {out}"
    );
}

#[test]
fn strips_cfg_all_test_combinations() {
    let out = strip("fn p() {}\n#[cfg(all(test, feature = \"x\"))]\nmod t {}\n");
    assert!(out.contains("fn p"));
    assert!(!out.contains("mod t"), "cfg(all(test,…)) removed: {out}");
}

#[test]
fn unchanged_without_tests_or_on_parse_error() {
    let prod = "pub fn a() {}\n";
    assert_eq!(
        strip_cfg_test(prod.as_bytes()),
        (prod.as_bytes().to_vec(), 0)
    );
    let broken = "@@@ not rust @@@";
    assert_eq!(
        strip_cfg_test(broken.as_bytes()),
        (broken.as_bytes().to_vec(), 0)
    );
}

#[test]
fn tloc_counts_the_whole_removed_test_region() {
    // 4 lines removed: the #[cfg(test)] attr, `mod tests {`, the body line,
    // and the closing `}`.
    let src = "pub fn p() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
    let (_prod, tloc) = strip_cfg_test(src.as_bytes());
    assert_eq!(tloc, 4);
}

fn metric(node: &code_ranker_plugin_api::node::Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => Some(*v as f64),
        Some(code_ranker_plugin_api::attrs::AttrValue::Float(v)) => Some(*v),
        _ => None,
    }
}

/// Strip inline tests from `src`, run the in-tree Rust engine, write the
/// metrics onto a fresh file node, and read one metric — the in-process
/// building block for the metamorphic tests below. Handles `.rs` only.
fn metric_of(_path: &str, src: &str, key: &str) -> Option<f64> {
    let (prod, tloc) = strip_cfg_test(src.as_bytes());
    let mut m = dialect::compute(&prod)?;
    m.tloc = tloc as f64;
    let mut node = code_ranker_plugin_api::node::Node {
        id: "t.rs".into(),
        kind: "file".into(),
        name: "t.rs".into(),
        parent: None,
        attrs: Default::default(),
    };
    code_ranker_graph::write_metrics(&mut node, &m);
    metric(&node, key)
}

// ---- Layer 1: metamorphic FP / FN matrix (see docs/metric-correctness.md) --
//
// Asserts the AST-Accurate principle across `metric × language × lexical
// position × direction`: a control-flow / exit keyword appearing only as a
// look-alike must NOT move the per-function metrics (no false positive); every
// real construct form MUST be counted (no false negative). Pure in-process
// parses — ~0 cost against the 20s budget. (LOC / Halstead are intentionally
// NOT in the keyword-invariance set: a real comment line legitimately changes
// `cloc`, a string legitimately adds Halstead operands — that is not an FP.)

/// A Rust function carrying real branching (so all five per-function metrics
/// are non-zero), with an optional doc-comment prefix and an optional
/// statement injected into the body. Used to build FP-matrix variants.
fn rs_src(doc: &str, body_inject: &str) -> String {
    format!(
        "{doc}fn f(a: i32, b: i32) -> i32 {{\n\
             {body_inject}    let g = |x: i32| x + 1;\n\
                 if a > 0 {{ return g(b); }}\n\
                 a + b\n\
             }}\n"
    )
}

// Per-language keyword look-alike guard set — the construct keywords/operators
// a complexity (or `unsafe`) metric can key on. The FP matrix injects these
// *only* as look-alikes and asserts no metric moves. This mirrors the
// "Keyword look-alike guard set" in languages/rust/metrics.md, and
// `rust_trigger_set_documented_in_spec` asserts the spec documents every entry
// — so the two cannot drift. A superset of the analyzer's real triggers is
// fine.
const RUST_TRIGGERS: &[&str] = &[
    "if", "else", "match", "while", "for", "loop", "return", "unsafe", "&&", "||", "?",
];

#[test]
fn rust_complexity_fp_matrix() {
    // Every lexical position that could smuggle a keyword in as text. None may
    // change cyclomatic / cognitive / exits / args / closures vs the base.
    let base = rs_src("", "");
    let kw = RUST_TRIGGERS.join(" ");
    let positions: &[(&str, String)] = &[
        (
            "line comment",
            rs_src("", &format!("    // {kw} && || ?\n")),
        ),
        (
            "block comment",
            rs_src("", &format!("    /* {kw} && || ? */\n")),
        ),
        ("doc comment", rs_src(&format!("/// {kw}\n"), "")),
        (
            "string",
            rs_src("", &format!("    let _s = \"{kw} && || ?\";\n")),
        ),
        (
            "raw string",
            rs_src("", &format!("    let _r = r#\"{kw} && ||\"#;\n")),
        ),
        (
            "identifier",
            rs_src(
                "",
                "    let if_match_return_loop = 0; let _ = if_match_return_loop;\n",
            ),
        ),
        (
            "format string",
            rs_src("", "    let _f = format!(\"if {} while\", a);\n"),
        ),
        (
            "macro body",
            rs_src("", "    let _m = vec![\"if\", \"match\", \"while\"];\n"),
        ),
        (
            "raw identifier",
            rs_src("", "    let r#match = 1; let _ = r#match;\n"),
        ),
    ];
    for key in ["cyclomatic", "cognitive", "exits", "args", "closures"] {
        let want = metric_of("t.rs", &base, key);
        for (pos, src) in positions {
            assert_eq!(
                metric_of("t.rs", src, key),
                want,
                "metric `{key}` moved when a keyword appeared only in: {pos}"
            );
        }
    }
}

#[test]
fn cyclomatic_counts_every_branch_form() {
    // FN guard: every branch form the analyzer recognizes must raise
    // cyclomatic above a branch-free baseline. (Exact per-form increments are
    // the analyzer's rule — layer 4; here we only assert "detected".)
    let baseline =
        metric_of("t.rs", "fn f() -> i32 { 0 }\n", "cyclomatic").expect("baseline cyclomatic");
    let forms: &[(&str, &str)] = &[
        ("if", "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n"),
        (
            "else-if",
            "fn f(a: i32) -> i32 { if a > 0 { 1 } else if a < 0 { 2 } else { 3 } }\n",
        ),
        (
            "match",
            "fn f(a: i32) -> i32 { match a { 0 => 1, _ => 2 } }\n",
        ),
        (
            "while",
            "fn f(mut a: i32) -> i32 { while a > 0 { a -= 1; } a }\n",
        ),
        (
            "for",
            "fn f(a: i32) -> i32 { let mut s = 0; for i in 0..a { s += i; } s }\n",
        ),
        ("loop", "fn f() -> i32 { loop { break; } 0 }\n"),
        (
            "&&",
            "fn f(a: i32, b: i32) -> i32 { let _ = a > 0 && b > 0; 0 }\n",
        ),
        (
            "||",
            "fn f(a: i32, b: i32) -> i32 { let _ = a > 0 || b > 0; 0 }\n",
        ),
        ("?", "fn f() -> Option<i32> { let x = Some(1)?; Some(x) }\n"),
        (
            "if let",
            "fn f() -> i32 { if let Some(x) = Some(1) { x } else { 0 } }\n",
        ),
        (
            "while let",
            "fn f() -> i32 { let mut it = [1].into_iter(); let mut n = 0; while let Some(_) = it.next() { n += 1; } n }\n",
        ),
    ];
    for (name, src) in forms {
        let c = metric_of("t.rs", src, "cyclomatic")
            .unwrap_or_else(|| panic!("cyclomatic missing for `{name}`"));
        assert!(
            c > baseline,
            "branch form `{name}` not counted (cyclomatic {c} <= baseline {baseline})"
        );
    }
    // Magnitude anchor: one extra `if` adds exactly 1.
    let one = metric_of(
        "t.rs",
        "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n",
        "cyclomatic",
    )
    .unwrap();
    let two = metric_of(
        "t.rs",
        "fn f(a: i32) -> i32 { if a > 0 { 1 } else if a < 0 { 2 } else { 3 } }\n",
        "cyclomatic",
    )
    .unwrap();
    assert_eq!(two - one, 1.0, "one extra real `if` must add exactly 1");
}

#[test]
fn rust_complexity_fn_per_metric() {
    // FN guard for the non-cyclomatic per-function metrics: a real construct
    // must surface the metric.
    let cognitive = metric_of(
        "t.rs",
        "fn f(a: i32, b: i32) -> i32 { if a > 0 { if b > 0 { 1 } else { 2 } } else { 3 } }\n",
        "cognitive",
    )
    .expect("cognitive present");
    assert!(cognitive > 0.0, "nested branches must raise cognitive");

    let exits =
        metric_of("t.rs", "fn f(a: i32) -> i32 { return a; }\n", "exits").expect("exits present");
    assert!(exits >= 1.0, "a real `return` must be counted as an exit");

    let args = metric_of(
        "t.rs",
        "fn f(a: i32, b: i32, c: i32) -> i32 { a + b + c }\n",
        "args",
    )
    .expect("args present");
    assert!(
        args >= 3.0,
        "three parameters must count as >=3 args, got {args}"
    );

    let closures = metric_of(
        "t.rs",
        "fn f() -> i32 { let g = |x: i32| x + 1; g(1) }\n",
        "closures",
    )
    .expect("closures present");
    assert!(closures >= 1.0, "a real closure must be counted");
}

#[test]
fn rust_only_complexity_fp_matrix() {
    // FP invariance for cyclomatic / cognitive, driven by Rust's documented
    // trigger set injected into comment / string positions.
    let check = |path: &str, base: &str, traps: &[String]| {
        for key in ["cyclomatic", "cognitive"] {
            let want = metric_of(path, base, key);
            for trap in traps {
                assert_eq!(
                    metric_of(path, trap, key),
                    want,
                    "{path} metric `{key}` moved on a keyword look-alike"
                );
            }
        }
    };

    let kw = RUST_TRIGGERS.join(" ");
    let base = "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n";
    check(
        "t.rs",
        base,
        &[
            format!("// {kw}\n{base}"),
            format!("fn f(a: i32) -> i32 {{ let _ = \"{kw}\"; if a > 0 {{ 1 }} else {{ 2 }} }}\n"),
        ],
    );
}

#[test]
fn rust_trigger_set_documented_in_spec() {
    // Lock-step guard: every keyword the FP matrix injects must be documented
    // in Rust's metrics spec, so the trigger list and the spec's "Keyword
    // look-alike guard set" cannot drift apart.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let path = format!("{root}/languages/rust/metrics.md");
    let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    for kw in RUST_TRIGGERS {
        assert!(
            spec.contains(&format!("`{kw}`")),
            "trigger `{kw}` is not documented in languages/rust/metrics.md — spec and FP test drifted"
        );
    }
}

// ---- Layer 2: generative tests (see docs/metric-correctness.md) ------------
//
// Generate programs with a KNOWN construct count, then assert the metric
// equals ground truth across a combinatorial grid. Deterministic (no random
// dependency, no flakiness) — proptest-style randomized fuzz is a later
// nightly extension. Still pure in-process parses; the whole grid is ~ms.

/// A Rust function with `noise` keyword-laden look-alike lines (a comment plus
/// a string binding, neither a real construct) followed by `branches` real,
/// independent `if` statements (each adds exactly 1 to cyclomatic).
fn gen_rs(branches: usize, noise: usize) -> String {
    let mut body = String::new();
    for i in 0..noise {
        body.push_str(&format!(
            "    // if match while for loop return && || ? noise {i}\n"
        ));
        body.push_str(&format!(
            "    let _n{i} = \"if match while return && ||\";\n"
        ));
    }
    for i in 0..branches {
        body.push_str(&format!("    if x > {i} {{ let _ = {i}; }}\n"));
    }
    format!("fn f(x: i32) -> i32 {{\n{body}    0\n}}\n")
}

#[test]
fn generative_cyclomatic_counts_branches_not_noise() {
    // Ground truth by construction: cyclomatic = baseline + (real `if` count),
    // independent of how many keyword look-alike lines surround it. Sweeps an
    // 8×8 grid of (branches, noise) — 64 generated programs.
    for noise in 0..8 {
        let base = metric_of("t.rs", &gen_rs(0, noise), "cyclomatic").expect("cyclomatic present");
        for branches in 0..8 {
            let cyc = metric_of("t.rs", &gen_rs(branches, noise), "cyclomatic")
                .expect("cyclomatic present");
            assert_eq!(
                cyc,
                base + branches as f64,
                "cyclomatic must add exactly 1 per real `if` and 0 per noise line \
                     (branches={branches}, noise={noise})"
            );
        }
    }
}

#[test]
fn generative_complexity_invariant_to_noise() {
    // A fixed real structure (2 args, a closure, a branch, a `return`) with a
    // growing pile of keyword look-alikes around it. Every per-function metric
    // must stay exactly at its noise-free value — no false positive at any
    // noise level.
    let mk = |noise: usize| -> String {
        let mut body = String::new();
        for i in 0..noise {
            body.push_str(&format!("    // if match return unsafe && || {i}\n"));
            body.push_str(&format!("    let _n{i} = \"if match return && ||\";\n"));
        }
        format!(
            "fn f(a: i32, b: i32) -> i32 {{\n\
                 {body}    let g = |x: i32| x + 1;\n\
                     if a > 0 {{ return g(b); }}\n\
                     a + b\n\
                 }}\n"
        )
    };
    for key in ["cyclomatic", "cognitive", "exits", "args", "closures"] {
        let want = metric_of("t.rs", &mk(0), key);
        for noise in 1..10 {
            assert_eq!(
                metric_of("t.rs", &mk(noise), key),
                want,
                "metric `{key}` moved at noise={noise} — keyword look-alikes leaked in"
            );
        }
    }
}

#[test]
fn per_function_metrics_aggregate_over_child_functions() {
    // Regression for the whole "root-vs-sum" class: `write_metrics` once read
    // the ROOT space value for `cyclomatic` / `cognitive` / `exits` / `args` /
    // `closures`, which for a file is the vacuous root count (0, or 1 for
    // cyclomatic) — every file looked identical. The real signal lives in the
    // child function spaces, so each must be the SUM over them.
    //
    // `a` takes 2 args, nests two `if`s, and `return`s; `b` defines a 1-arg
    // closure. So the file must surface: cyclomatic (summed branches), a
    // non-zero cognitive (nesting), exits (the `return`), args (2 fn + 1
    // closure = 3), and closures (1).
    let src = "fn a(x: i32, y: i32) -> i32 { if x > 0 { if x > 1 { return x; } y } else { 3 } }\n\
                   fn b() -> i32 { let f = |z: i32| z + 1; f(2) }\n";
    // Each is summed over the child functions — well above the vacuous root
    // value, proving aggregation rather than a root-only read.
    let cyc = metric_of("t.rs", src, "cyclomatic").expect("cyclomatic present");
    assert!(cyc > 1.0, "cyclomatic should be summed, got {cyc}");
    let cog = metric_of("t.rs", src, "cognitive").expect("cognitive present");
    assert!(cog > 0.0, "cognitive should be summed, got {cog}");
    let exits = metric_of("t.rs", src, "exits").expect("exits present");
    assert!(exits >= 1.0, "exits should count the `return`, got {exits}");
    let args = metric_of("t.rs", src, "args").expect("args present");
    assert!(
        args >= 3.0,
        "args should sum fn (2) + closure (1), got {args}"
    );
    let closures = metric_of("t.rs", src, "closures").expect("closures present");
    assert!(
        closures >= 1.0,
        "closures should count the closure, got {closures}"
    );
}

// ---- Layer 3: asserted anchors (see docs/metric-correctness.md) -----------
//
// Layers 1 & 2 prove RELATIVE behaviour (noise-invariance, +1 per construct)
// but never pin an ABSOLUTE value, so a uniform offset/scale bug (every count
// shifted by +1, or doubled) would pass green. These anchors pin exact values
// hand-derived from languages/rust/metrics.md, catching that scale class.

#[test]
fn complexity_absolute_anchors_hand_derived() {
    // Integer counting metrics, pinned to EXACT file-level values, hand-derived
    // from the spec's rules (metrics.md §cyclomatic / §exits,args,closures).
    //
    // These pin the analyzer-of-record's whole-file values (what we emit):
    //   • `cyclomatic` = the file unit's base path (1) + Σ over functions of
    //     (1 + branch points). Per-function McCabe (`V(G)=E−N+2P` = Σ over
    //     functions) is the theory; the analyzer adds the file unit on top and
    //     we emit it verbatim (it is also the value `mi` is computed from).
    //     `classify` = file 1 + fn 4 (base1+if+else-if+||) = 5.
    //   • `exits` = Σ over functions of (a value-returning `-> T` exit +
    //     explicit return/?). "Exit points" has no canonical theory, so the
    //     analyzer's rule is the source of truth (metrics.md §exits). The
    //     `-> i32` snippets below read 2 (the explicit return + the `-> T` exit).
    //   • `args` / `closures` / `cognitive` have no file-unit offset.
    // All pinned so any drift from the analyzer's output is caught.
    let classify = "fn classify(n: i32) -> &'static str {\n\
            \x20   if n < 0 { \"neg\" } else if n == 0 || n == 1 { \"small\" } else { \"big\" }\n\
            }\n";
    let two_closures = "fn f() { let g = |x: i32| x + 1; let h = |y: i32| y; let _ = (g, h); }\n";
    // (label, path, src, key, exact_expected)
    let cases: &[(&str, &str, &str, &str, f64)] = &[
        // file unit 1 + fn(base1 + if + else-if + ||) = 1 + 4 = 5.
        ("classify", "t.rs", classify, "cyclomatic", 5.0),
        // file unit 1 + fn(base1 + 1 if) = 1 + 2 = 3 (else is free).
        (
            "single if",
            "t.rs",
            "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n",
            "cyclomatic",
            3.0,
        ),
        // 1 explicit return + 1 value-returning exit (`-> i32`) → 2.
        (
            "one return",
            "t.rs",
            "fn f() -> i32 { return 1; }\n",
            "exits",
            2.0,
        ),
        // 1 `?` + 1 value-returning exit (`-> Option`) → 2.
        (
            "one try op",
            "t.rs",
            "fn f() -> Option<i32> { let x = Some(1)?; Some(x) }\n",
            "exits",
            2.0,
        ),
        (
            "three params",
            "t.rs",
            "fn f(a: i32, b: i32, c: i32) -> i32 { a + b + c }\n",
            "args",
            3.0,
        ),
        ("two closures", "t.rs", two_closures, "closures", 2.0),
        ("two closure args", "t.rs", two_closures, "args", 2.0),
    ];
    let mut fails = Vec::new();
    for (label, path, src, key, want) in cases {
        match metric_of(path, src, key) {
            Some(got) if got == *want => {}
            other => fails.push(format!("{label}: {key} want {want}, got {other:?}")),
        }
    }
    assert!(
        fails.is_empty(),
        "failing integer anchors:\n{}",
        fails.join("\n")
    );
}

#[test]
fn complexity_frozen_scale_anchors() {
    // Algorithm-specific metrics (cognitive nesting weights, Halstead
    // dictionaries, MI) cannot be hand-derived reliably, so they are FROZEN
    // anchors: values produced by `rust-code-analysis` for one fixed snippet,
    // verified once. Their job is to catch a uniform offset/scale regression
    // (a library bump that doubles `volume`, an MI formula edit) — not to
    // claim an independent ground truth. They change only when the underlying
    // algorithm changes, and that change should be deliberate.
    let classify = "fn classify(n: i32) -> &'static str {\n\
            \x20   if n < 0 { \"neg\" } else if n == 0 || n == 1 { \"small\" } else { \"big\" }\n\
            }\n";
    // (key, expected, abs_tolerance)
    let cases: &[(&str, f64, f64)] = &[
        ("cognitive", 4.0, 0.0),   // exact integer
        ("vocabulary", 18.0, 0.0), // η₁ + η₂, exact integer
        ("length", 28.0, 0.0),     // N₁ + N₂, exact integer
        ("volume", 116.757, 0.01), // length × log₂(vocabulary)
        ("effort", 875.684, 0.01), // difficulty × volume
        ("mi", 127.299, 0.01),     // maintainability index
        ("mi_sei", 108.463, 0.01), // SEI variant
    ];
    let mut fails = Vec::new();
    for (key, want, tol) in cases {
        match metric_of("t.rs", classify, key) {
            Some(got) if (got - *want).abs() <= *tol => {}
            other => fails.push(format!("{key}: want {want} (±{tol}), got {other:?}")),
        }
    }
    assert!(
        fails.is_empty(),
        "failing scale anchors:\n{}",
        fails.join("\n")
    );
}

#[test]
fn declaration_only_file_emits_no_complexity() {
    // No functions → only the file unit space → cyclomatic is a vacuous 1 and
    // cognitive is 0. Both must be dropped (not shown as a meaningless "1"),
    // matching how `put` already drops cognitive's 0. Mirrors real files like
    // a clap CLI model or a type-definitions module.
    let src = "pub struct Cli { pub verbose: bool }\n\
                   pub enum Mode { A, B }\n";
    assert_eq!(
        metric_of("t.rs", src, "cyclomatic"),
        None,
        "a declaration-only file must not emit a vacuous cyclomatic"
    );
    assert_eq!(
        metric_of("t.rs", src, "cognitive"),
        None,
        "a declaration-only file must not emit cognitive"
    );
}

#[test]
fn metric_specs_override_adds_rust_cfg_test_note() {
    // The neutral default descriptions carry no language nuance; the Rust
    // plugin re-adds the `#[cfg(test)]` LOC-exclusion note for sloc/lloc/
    // cloc/blank — so it appears only in Rust snapshots, never in py/js/ts.
    let defaults = code_ranker_graph::metric_specs().0;
    // sanity: the shared default is language-neutral
    assert!(
        !defaults["blank"]
            .description
            .as_deref()
            .unwrap_or("")
            .contains("#[cfg(test)]"),
        "the shared default must stay language-neutral"
    );

    let cfg = RustPlugin.config();
    let refined = RustPlugin.metric_specs(&cfg, defaults);
    for key in ["sloc", "lloc", "cloc", "blank"] {
        let desc = refined[key].description.as_deref().unwrap_or("");
        assert!(
            desc.contains("#[cfg(test)]"),
            "Rust `{key}` description should note the cfg(test) exclusion"
        );
    }
}

#[test]
fn metrics_and_function_units_skip_unreadable_files() {
    // A file node whose path does not exist is silently skipped by both passes
    // (the `fs::read(..) else continue` arms) — no panic, no output.
    let graph = code_ranker_plugin_api::graph::Graph {
        nodes: vec![code_ranker_plugin_api::node::Node {
            id: "/no/such/dir/missing.rs".into(),
            kind: "file".into(),
            name: "missing.rs".into(),
            parent: None,
            attrs: Default::default(),
        }],
        edges: vec![],
    };
    let cfg = RustPlugin.config();
    assert!(RustPlugin.metrics(&cfg, &graph).is_empty());
    assert!(RustPlugin.function_units(&cfg, &graph).is_empty());
}

#[test]
fn strip_cfg_test_passes_through_non_utf8() {
    // Non-UTF-8 input can't be parsed; `strip_cfg_test` returns it unchanged with
    // `tloc = 0` (the `from_utf8 .. else` guard) rather than panicking.
    let (out, tloc) = strip_cfg_test(&[0xff, 0xfe, 0x00]);
    assert_eq!(out, vec![0xff, 0xfe, 0x00]);
    assert_eq!(tloc, 0);
}

#[test]
fn offline_metadata_error_explains_warm_cache() {
    let err = cargo_metadata::Error::CargoMetadata {
        stderr: "no such manifest".into(),
    };
    let e = analyze::offline_metadata_error(std::path::Path::new("/proj/Cargo.toml"), err);
    let msg = format!("{e}");
    assert!(msg.contains("/proj/Cargo.toml"), "names the manifest");
    assert!(
        msg.contains("offline tool"),
        "explains the offline constraint"
    );
    assert!(
        msg.contains("no such manifest"),
        "includes the underlying cargo error"
    );
}
