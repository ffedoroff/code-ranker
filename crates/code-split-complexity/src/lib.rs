//! Central, language-agnostic complexity pass. Given a structural graph whose
//! file nodes carry their absolute path as `id`, this reads each file, picks a
//! `rust-code-analysis` parser by extension, and writes the metrics into the
//! node's `attrs` as flat keys. It is the single place that knows
//! rust-code-analysis; plugins emit structure only.
//!
//! The metric attribute dictionary it can produce is exposed via
//! [`metric_specs`] so the orchestrator can declare it in the snapshot.

use code_split_graph::attrs::num_attr;
use code_split_plugin_api::{
    attrs::ValueType,
    graph::Graph,
    level::{AttributeGroup, AttributeSpec},
};
use rust_code_analysis::{
    FuncSpace, JavascriptParser, ParserTrait, PythonParser, RustParser, TsxParser,
    TypescriptParser, metrics,
};
use std::collections::BTreeMap;
use std::path::Path;

/// Annotate every file node (`kind == "file"`) whose `id` is a readable source
/// file of a known extension with complexity metrics. Returns the number of
/// nodes annotated. Nodes whose file cannot be read/parsed are left untouched.
pub fn annotate(graph: &mut Graph) -> usize {
    let mut annotated = 0usize;
    for node in &mut graph.nodes {
        if node.kind != "file" {
            continue;
        }
        let path = Path::new(&node.id);
        let Ok(src) = std::fs::read(path) else {
            continue;
        };
        let Some((space, tloc)) = parse_metrics(path, src) else {
            continue;
        };
        write_metrics(node, &space, tloc);
        annotated += 1;
    }
    annotated
}

/// True if any attribute gates an item to tests: `#[test]`, `#[bench]`, or
/// `#[cfg(test)]` / `#[cfg(all(test, …))]` / `#[cfg(any(test, …))]`. A `test`
/// **identifier** inside `cfg(...)` is what matches — `cfg(feature = "test")`
/// (a string literal) does not.
fn is_test_attr(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("test") || attr.path().is_ident("bench") {
        return true;
    }
    if attr.path().is_ident("cfg")
        && let syn::Meta::List(list) = &attr.meta
    {
        return tokens_have_test_ident(list.tokens.clone());
    }
    false
}

/// Recursively scan a token stream for a bare `test` identifier (descends into
/// `all(...)` / `any(...)` groups).
fn tokens_have_test_ident(ts: proc_macro2::TokenStream) -> bool {
    ts.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == "test",
        proc_macro2::TokenTree::Group(g) => tokens_have_test_ident(g.stream()),
        _ => false,
    })
}

/// Visitor collecting the 1-based, inclusive line ranges of test-only items
/// (`#[cfg(test)]` modules, `#[test]`/`#[cfg(test)]` fns), attribute line
/// included. It recurses into ordinary modules to catch nested test modules but
/// not into a test item it already captured.
#[derive(Default)]
struct TestSpans {
    ranges: Vec<(usize, usize)>,
}

impl TestSpans {
    fn record(&mut self, attrs: &[syn::Attribute], span: proc_macro2::Span) {
        use syn::spanned::Spanned;
        let start = attrs
            .iter()
            .map(|a| a.span().start().line)
            .chain(std::iter::once(span.start().line))
            .min()
            .unwrap_or(0);
        self.ranges.push((start, span.end().line));
    }
}

impl<'ast> syn::visit::Visit<'ast> for TestSpans {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        use syn::spanned::Spanned;
        if m.attrs.iter().any(is_test_attr) {
            self.record(&m.attrs, m.span());
        } else {
            syn::visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        use syn::spanned::Spanned;
        if f.attrs.iter().any(is_test_attr) {
            self.record(&f.attrs, f.span());
        }
    }
}

/// Step 1 of the Rust line accounting: remove `#[cfg(test)]` / `#[test]` /
/// `#[bench]` items so the production metrics (`sloc` / `cloc` / `blank` / `hk` /
/// complexity) are then measured on production code only. Returns the production
/// source **and** `tloc` — the number of test lines removed (the whole test
/// region: attribute, body, braces). Parse failures or no test items return the
/// source unchanged with `tloc = 0`.
fn strip_cfg_test(src: &[u8]) -> (Vec<u8>, usize) {
    use syn::visit::Visit;
    let Ok(text) = std::str::from_utf8(src) else {
        return (src.to_vec(), 0);
    };
    let Ok(file) = syn::parse_file(text) else {
        return (src.to_vec(), 0);
    };
    let mut spans = TestSpans::default();
    spans.visit_file(&file);
    if spans.ranges.is_empty() {
        return (src.to_vec(), 0);
    }
    let drop: std::collections::HashSet<usize> =
        spans.ranges.iter().flat_map(|&(s, e)| s..=e).collect();
    let tloc = drop.len();
    let mut out: String = text
        .lines()
        .enumerate()
        .filter(|(i, _)| !drop.contains(&(i + 1)))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    (out.into_bytes(), tloc)
}

/// Pick a parser by file extension and compute the file's production `FuncSpace`
/// plus `tloc` — the number of **test** lines (`#[cfg(test)]` / `#[test]` /
/// `#[bench]`) removed before measuring. Only Rust strips tests, so `tloc` is
/// `0.0` for every other language. (Step 1 strips tests; step 2, in
/// `write_metrics`, counts sloc/cloc/blank on the production remainder.)
fn parse_metrics(path: &Path, src: Vec<u8>) -> Option<(FuncSpace, f64)> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    match ext {
        "rs" => {
            let (prod_src, tloc) = strip_cfg_test(&src);
            let prod = metrics(&RustParser::new(prod_src, path, None), path)?;
            Some((prod, tloc as f64))
        }
        "py" => metrics(&PythonParser::new(src, path, None), path).map(|s| (s, 0.0)),
        "ts" | "mts" | "cts" => {
            metrics(&TypescriptParser::new(src, path, None), path).map(|s| (s, 0.0))
        }
        "tsx" => metrics(&TsxParser::new(src, path, None), path).map(|s| (s, 0.0)),
        "js" | "jsx" | "mjs" | "cjs" => {
            metrics(&JavascriptParser::new(src, path, None), path).map(|s| (s, 0.0))
        }
        _ => None,
    }
}

/// Write the metric attributes for one file node. Each value is omitted when it
/// rounds to zero; the LOC block is gated on `sloc > 0` and the Halstead block
/// on `volume > 0` (matching the historical behavior).
fn write_metrics(node: &mut code_split_plugin_api::node::Node, s: &FuncSpace, tloc: f64) {
    let m = &s.metrics;
    let mut put = |key: &str, v: f64| {
        let a = num_attr(v);
        if matches!(&a, code_split_plugin_api::attrs::AttrValue::Int(0))
            || matches!(&a, code_split_plugin_api::attrs::AttrValue::Float(f) if *f == 0.0)
        {
            node.attrs.remove(key);
        } else {
            node.attrs.insert(key.to_string(), a);
        }
    };

    put("cyclomatic", m.cyclomatic.cyclomatic());
    put("cognitive", m.cognitive.cognitive());
    put("exits", m.nexits.exit());
    let args = if m.nargs.fn_args() > 0.0 {
        m.nargs.fn_args()
    } else {
        m.nargs.closure_args()
    };
    put("args", args);
    put("closures", m.nom.closures());

    put("mi", m.mi.mi_original());
    put("mi_sei", m.mi.mi_sei());

    // `sloc` here means *physical lines of code* — lines with real code, excluding
    // blanks and comment-only lines (see this key's spec). rust-code-analysis names
    // that `ploc()`; its `sloc()` is the total line count (already exposed as `loc`).
    //
    // NOTE: for Rust these four — `sloc` (physical), `lloc` (logical), `cloc`
    // (comments), `blank` — are all measured on the *production* source, i.e.
    // AFTER `strip_cfg_test` removed `#[cfg(test)]` / `#[test]` / `#[bench]`
    // items. So none of them count lines from inline tests; those go to `tloc`.
    let sloc = m.loc.ploc();
    if sloc > 0.0 {
        put("sloc", sloc);
        put("lloc", m.loc.lloc());
        put("cloc", m.loc.cloc());
        put("blank", m.loc.blank());
    }
    // Test source lines (`#[cfg(test)]`/`#[test]`/`#[bench]`), the complement of
    // `sloc`. Zero (non-Rust, or no inline tests) is dropped by `put`.
    put("tloc", tloc);

    let volume = m.halstead.volume();
    if volume > 0.0 {
        put("length", m.halstead.length());
        put(
            "vocabulary",
            m.halstead.u_operators() + m.halstead.u_operands(),
        );
        put("volume", volume);
        put("effort", m.halstead.effort());
        put("time", m.halstead.time());
        put("bugs", m.halstead.bugs());
    }
}

/// One metric row: (key, group, value_type, label, name, short, description,
/// formula, calc, direction). Empty strings become `None`.
type MetricRow = (
    &'static str,
    &'static str,
    ValueType,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

fn group(label: &str, description: &str) -> AttributeGroup {
    AttributeGroup {
        label: Some(label.to_string()),
        description: Some(description.to_string()),
    }
}

/// The complexity metric attribute dictionary and its groups, fully enriched
/// (label/name/short/description/formula/calc/direction) so the UI hardcodes no
/// metric. The orchestrator merges these into each level's `node_attributes` /
/// `attribute_groups` (then prunes to keys actually present) and overlays
/// language thresholds. Coupling/cycle specs live in `code-split-graph`.
pub fn metric_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    use ValueType::{Float, Int};
    let opt = |s: &str| {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    // (key, group, value_type, label, name, short, description, formula, calc, direction)
    let rows: &[MetricRow] = &[
        (
            "cyclomatic",
            "complexity",
            Int,
            "Cyclomatic",
            "Cyclomatic complexity",
            "Cyclomatic",
            "Number of linearly independent paths through the code. Higher values indicate complex branching logic.",
            "branches + 1",
            "",
            "lower_better",
        ),
        (
            "cognitive",
            "complexity",
            Int,
            "Cognitive",
            "Cognitive complexity",
            "Cognitive",
            "Measures how difficult the code is to understand, accounting for nesting depth and non-structural control flow.",
            "",
            "",
            "lower_better",
        ),
        (
            "exits",
            "complexity",
            Int,
            "Exits",
            "Exit points",
            "Exits",
            "Number of exit points (return/throw) in the unit.",
            "",
            "",
            "lower_better",
        ),
        (
            "args",
            "complexity",
            Int,
            "Args",
            "Arguments",
            "Args",
            "Number of function / closure arguments.",
            "",
            "",
            "lower_better",
        ),
        (
            "closures",
            "complexity",
            Int,
            "Closures",
            "Closures",
            "Closures",
            "Number of closures defined in the unit.",
            "",
            "",
            "lower_better",
        ),
        (
            "mi",
            "maintainability",
            Float,
            "MI",
            "Maintainability index",
            "MI",
            "Maintainability Index (0–100, higher is more maintainable). Derived from Halstead volume, cyclomatic complexity, and SLOC.",
            "171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(sloc)",
            "",
            "higher_better",
        ),
        (
            "mi_sei",
            "maintainability",
            Float,
            "MI (SEI)",
            "Maintainability (SEI)",
            "MI SEI",
            "SEI variant of the Maintainability Index — adds a bonus for comment density.",
            "MI + 50·sin(√(2.4 × comment-ratio))",
            "",
            "higher_better",
        ),
        (
            "sloc",
            "loc",
            Int,
            "Source",
            "Source lines (sloc)",
            "SLOC",
            "Source lines of code — lines with at least one non-whitespace, non-comment character. Blank and comment-only lines are not counted. In Rust, lines inside `#[cfg(test)]` / `#[test]` items are excluded too, so this counts production code only (unlike `loc`, the raw file line count).",
            "",
            "",
            "higher_better",
        ),
        (
            "lloc",
            "loc",
            Int,
            "Logical",
            "Logical LOC",
            "Logical",
            "Logical lines — counts statements, not physical lines. In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            "",
            "",
            "higher_better",
        ),
        (
            "cloc",
            "loc",
            Int,
            "Comments",
            "Comment lines",
            "Comments",
            "Comment-only lines (inline comments on code lines are not counted). In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            "",
            "",
            "higher_better",
        ),
        (
            "blank",
            "loc",
            Int,
            "Blank",
            "Blank lines",
            "Blank",
            "Empty or whitespace-only lines. In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            "",
            "",
            "higher_better",
        ),
        (
            "tloc",
            "loc",
            Int,
            "Test",
            "Test lines (tloc)",
            "TLOC",
            "Test lines of code — the lines inside `#[cfg(test)]` / `#[test]` / `#[bench]` items (Rust), removed before the production metrics are measured. The complement of `sloc`: test code never inflates a file's size, HK, or complexity.",
            "",
            "",
            "higher_better",
        ),
        (
            "length",
            "halstead",
            Float,
            "Length",
            "Halstead length",
            "H.len",
            "Program length — total operator + operand occurrences.",
            "N₁ + N₂",
            "",
            "lower_better",
        ),
        (
            "vocabulary",
            "halstead",
            Float,
            "Vocabulary",
            "Halstead vocabulary",
            "H.vocab",
            "Vocabulary — distinct operators + operands.",
            "η₁ + η₂",
            "",
            "lower_better",
        ),
        (
            "volume",
            "halstead",
            Float,
            "Volume",
            "Halstead volume",
            "H.vol",
            "Algorithm size in bits, from distinct operators and operands.",
            "length × log₂(vocabulary)",
            "length * Math.log2(vocabulary)",
            "lower_better",
        ),
        (
            "effort",
            "halstead",
            Float,
            "Effort",
            "Halstead effort",
            "H.effort",
            "Mental effort to implement the algorithm.",
            "volume × difficulty",
            "",
            "lower_better",
        ),
        (
            "time",
            "halstead",
            Float,
            "Time",
            "Halstead time, s",
            "H.time(s)",
            "Estimated implementation time, in seconds.",
            "effort ÷ 18",
            "effort / 18",
            "lower_better",
        ),
        (
            "bugs",
            "halstead",
            Float,
            "Bugs",
            "Halstead bugs",
            "H.bugs",
            "Estimated delivered bugs — a rough predictor of defect density.",
            "effort^⅔ ÷ 3000",
            "effort ** (2/3) / 3000",
            "lower_better",
        ),
    ];
    let mut specs = BTreeMap::new();
    for (k, g, vt, label, name, short, desc, formula, calc, dir) in rows {
        let mut s = AttributeSpec::new(*vt, label);
        s.group = opt(g);
        s.name = opt(name);
        s.short = opt(short);
        s.description = opt(desc);
        s.formula = opt(formula);
        s.calc = opt(calc);
        s.direction = opt(dir);
        specs.insert((*k).to_string(), s);
    }
    let mut groups = BTreeMap::new();
    groups.insert(
        "complexity".to_string(),
        group("Complexity", "Code complexity metrics"),
    );
    groups.insert(
        "halstead".to_string(),
        group("Halstead", "Halstead software metrics"),
    );
    groups.insert(
        "loc".to_string(),
        group("Lines of Code", "Lines of code breakdown"),
    );
    groups.insert(
        "maintainability".to_string(),
        group("Maintainability", "Maintainability index"),
    );
    (specs, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(src: &str) -> String {
        String::from_utf8(strip_cfg_test(src.as_bytes()).0).unwrap()
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
}
