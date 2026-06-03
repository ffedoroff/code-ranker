//! Central, language-agnostic complexity pass. Given a structural graph whose
//! file nodes carry their absolute path as `id`, this reads each file, picks a
//! `rust-code-analysis` parser by extension, and writes the metrics into the
//! node's `attrs` as flat keys. It is the single place that knows
//! rust-code-analysis; plugins emit structure only.
//!
//! The metric attribute dictionary it can produce is exposed via
//! [`metric_specs`] so the orchestrator can declare it in the snapshot.

use code_split_graph::num_attr;
use code_split_plugin_api::{AttributeGroup, AttributeSpec, Graph, ValueType};
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
        let Some(space) = parse_metrics(path, src) else {
            continue;
        };
        write_metrics(node, &space);
        annotated += 1;
    }
    annotated
}

/// Pick a parser by file extension and compute the file's `FuncSpace`.
fn parse_metrics(path: &Path, src: Vec<u8>) -> Option<FuncSpace> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    match ext {
        "rs" => metrics(&RustParser::new(src, path, None), path),
        "py" => metrics(&PythonParser::new(src, path, None), path),
        "ts" | "mts" | "cts" => metrics(&TypescriptParser::new(src, path, None), path),
        "tsx" => metrics(&TsxParser::new(src, path, None), path),
        "js" | "jsx" | "mjs" | "cjs" => metrics(&JavascriptParser::new(src, path, None), path),
        _ => None,
    }
}

/// Write the metric attributes for one file node. Each value is omitted when it
/// rounds to zero; the LOC block is gated on `sloc > 0` and the Halstead block
/// on `volume > 0` (matching the historical behavior).
fn write_metrics(node: &mut code_split_plugin_api::Node, s: &FuncSpace) {
    let m = &s.metrics;
    let mut put = |key: &str, v: f64| {
        let a = num_attr(v);
        if matches!(&a, code_split_plugin_api::AttrValue::Int(0))
            || matches!(&a, code_split_plugin_api::AttrValue::Float(f) if *f == 0.0)
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

    let sloc = m.loc.sloc();
    if sloc > 0.0 {
        put("sloc", sloc);
        put("lloc", m.loc.lloc());
        put("cloc", m.loc.cloc());
        put("blank", m.loc.blank());
    }

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
            "Source lines of code — lines with at least one non-whitespace, non-comment character. Blank and comment-only lines are not counted.",
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
            "Logical lines — counts statements, not physical lines.",
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
            "Comment-only lines (inline comments on code lines are not counted).",
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
            "Empty or whitespace-only lines.",
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
