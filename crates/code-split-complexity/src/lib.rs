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

fn spec(group: Option<&str>, label: &str, value_type: ValueType) -> AttributeSpec {
    AttributeSpec {
        value_type,
        label: Some(label.to_string()),
        hint: None,
        group: group.map(str::to_string),
    }
}

fn group(label: &str, hint: &str) -> AttributeGroup {
    AttributeGroup {
        label: Some(label.to_string()),
        hint: Some(hint.to_string()),
    }
}

/// The complexity metric attribute dictionary and its groups. The orchestrator
/// merges these into each level's `node_attributes` / `attribute_groups` (then
/// prunes to keys actually present). Coupling/cycle specs live in
/// `code-split-graph`.
pub fn metric_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    use ValueType::{Float, Int};
    let mut specs = BTreeMap::new();
    let c = Some("complexity");
    let h = Some("halstead");
    let l = Some("loc");
    let mt = Some("maintainability");
    for (k, g, lbl, t) in [
        ("cyclomatic", c, "Cyclomatic", Int),
        ("cognitive", c, "Cognitive", Int),
        ("exits", c, "Exits", Int),
        ("args", c, "Args", Int),
        ("closures", c, "Closures", Int),
        ("mi", mt, "MI", Float),
        ("mi_sei", mt, "MI (SEI)", Float),
        ("sloc", l, "Source", Int),
        ("lloc", l, "Logical", Int),
        ("cloc", l, "Comments", Int),
        ("blank", l, "Blank", Int),
        ("length", h, "Length", Float),
        ("vocabulary", h, "Vocabulary", Float),
        ("volume", h, "Volume", Float),
        ("effort", h, "Effort", Float),
        ("time", h, "Time", Float),
        ("bugs", h, "Bugs", Float),
    ] {
        specs.insert(k.to_string(), spec(g, lbl, t));
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
