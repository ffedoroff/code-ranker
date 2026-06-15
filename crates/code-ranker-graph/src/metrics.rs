//! Language-neutral complexity-metric scaffolding: the computed-value carrier
//! [`FileMetrics`], the writer [`write_metrics`] (omit-at + LOC/Halstead gating),
//! the no-signal values [`metric_omit_at`], and the metric attribute catalog
//! [`metric_specs`]. The sibling [`coupling_specs`](crate::coupling_specs) lives
//! alongside it; both are merged into the snapshot's node-attribute dictionary.
//!
//! The per-language **engines** that produce a `FileMetrics` live in the language
//! crates — `rust_ts` in `code-ranker-plugin-rust`, `python_ts` in
//! `code-ranker-plugin-python`, `ecmascript_ts` in `code-ranker-ecmascript-core`.
//! Each plugin computes a `FileMetrics` with its own engine and calls
//! [`write_metrics`] here. This module names no language and pulls in no grammar.

use crate::attrs::num_attr;
use code_ranker_plugin_api::{
    attrs::ValueType,
    level::{AttributeGroup, AttributeSpec, Direction, SpecRow, attr_dict, group},
    node::Node,
};
use std::collections::BTreeMap;

/// The per-file complexity metric values a language engine computes, in the
/// canonical key set this crate writes. Engines fill the fields they support and
/// leave the rest at `0.0` (e.g. `tloc` is non-zero only for Rust, where inline
/// `#[cfg(test)]` items are stripped). [`write_metrics`] turns this into node
/// attributes, applying each key's `omit_at` and the LOC / Halstead gating.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileMetrics {
    pub cyclomatic: f64,
    pub cognitive: f64,
    pub exits: f64,
    pub args: f64,
    pub closures: f64,
    pub mi: f64,
    pub mi_sei: f64,
    pub sloc: f64,
    pub lloc: f64,
    pub cloc: f64,
    pub blank: f64,
    /// Test lines (Rust only: lines removed with `#[cfg(test)]`/`#[test]`/`#[bench]`).
    pub tloc: f64,
    pub length: f64,
    pub vocabulary: f64,
    pub volume: f64,
    pub effort: f64,
    pub time: f64,
    pub bugs: f64,
}

/// Write the metric attributes for one file node from a computed [`FileMetrics`].
/// Each value is dropped at its `omit_at` ([`metric_omit_at`]); the LOC block is
/// gated on `sloc > 0` and the Halstead block on `volume > 0`. The same omit
/// values are published on the specs ([`metric_specs`]), so emission and the
/// declared spec never drift. Called by each language plugin after its engine
/// produces the values.
pub fn write_metrics(node: &mut Node, m: &FileMetrics) {
    let mut put = |key: &str, v: f64| {
        let a = num_attr(v);
        if a == num_attr(metric_omit_at(key)) {
            node.attrs.remove(key);
        } else {
            node.attrs.insert(key.to_string(), a);
        }
    };
    put("cyclomatic", m.cyclomatic);
    put("cognitive", m.cognitive);
    put("exits", m.exits);
    put("args", m.args);
    put("closures", m.closures);
    put("mi", m.mi);
    put("mi_sei", m.mi_sei);
    if m.sloc > 0.0 {
        put("sloc", m.sloc);
        put("lloc", m.lloc);
        put("cloc", m.cloc);
        put("blank", m.blank);
    }
    put("tloc", m.tloc);
    if m.volume > 0.0 {
        put("length", m.length);
        put("vocabulary", m.vocabulary);
        put("volume", m.volume);
        put("effort", m.effort);
        put("time", m.time);
        put("bugs", m.bugs);
    }
}

/// The value at which a per-file metric carries no signal and is **omitted** from
/// output (see [`code_ranker_plugin_api::level::AttributeSpec::omit_at`]). `0` for
/// almost everything; `1` for `cyclomatic` — the analyzer gives the file unit a
/// McCabe base path of `1`, so a function-less file reports a vacuous `1` that
/// carries no signal and must be dropped. The per-language writers gate emission
/// on this value and [`metric_specs`] publishes the same value on each spec, so
/// the two never drift.
fn metric_omit_at(key: &str) -> f64 {
    match key {
        "cyclomatic" => 1.0,
        _ => 0.0,
    }
}

/// The complexity metric attribute dictionary and its groups, fully enriched
/// (label/name/short/description/formula/calc/direction) so the UI hardcodes no
/// metric. The orchestrator merges these into each level's `node_attributes` /
/// `attribute_groups` (then prunes to keys actually present) and overlays
/// language thresholds. Coupling/cycle specs live in `code-ranker-graph`.
pub fn metric_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    use Direction::{HigherBetter, LowerBetter};
    use ValueType::Float;
    let mut specs = attr_dict(vec![
        (
            "cyclomatic",
            SpecRow {
                group: "complexity",
                label: "Cyclomatic",
                name: "Cyclomatic complexity",
                short: "Cyclomatic",
                description: "Number of independent paths through the code — roughly the minimum number of test cases needed to cover every branch.<br>A function starts at 1 and gains +1 per decision point: each `if` / `else if`, every `match` / `switch` arm, every loop, and each `&&` / `||` in a condition.<br>Summed across every function in the file, so it grows with both size and branching — the file's total branching burden.<br>Counts paths only, ignoring how deeply they nest. For a readability-weighted view see `cognitive`.",
                formula: "Σ (branches + 1) over functions",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "cognitive",
            SpecRow {
                group: "complexity",
                label: "Cognitive",
                name: "Cognitive complexity",
                short: "Cognitive",
                description: "How hard the code is for a human to follow — not just how many paths it has.<br>Like `cyclomatic` it adds +1 for each break in linear flow (`if`, `else`, `match`, loops, `catch`, chained `&&` / `||`), but it also adds an extra +1 for every level of nesting: an `if` inside a loop inside an `if` costs far more than three flat `if`s.<br>That nesting penalty is the point — deeply indented logic is what actually strains a reader, so a high `cognitive` next to a modest `cyclomatic` flags tangled, hard-to-read code.<br>Summed across every function in the file.",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "exits",
            SpecRow {
                group: "complexity",
                label: "Exits",
                name: "Exit points",
                short: "Exits",
                description: "Number of exit points (return/throw) in the unit.",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "args",
            SpecRow {
                group: "complexity",
                label: "Args",
                name: "Arguments",
                short: "Args",
                description: "Number of function / closure arguments.",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "closures",
            SpecRow {
                group: "complexity",
                label: "Closures",
                name: "Closures",
                short: "Closures",
                description: "Number of closures defined in the unit.",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "mi",
            SpecRow {
                group: "maintainability",
                value_type: Float,
                label: "MI",
                name: "Maintainability index",
                short: "MI",
                description: "Maintainability Index (0–100, higher is more maintainable). Derived from Halstead volume, cyclomatic complexity, and SLOC.",
                formula: "171 − 5.2·ln(volume) − 0.23·cyclomatic − 16.2·ln(sloc)",
                direction: HigherBetter,
                ..Default::default()
            },
        ),
        (
            "mi_sei",
            SpecRow {
                group: "maintainability",
                value_type: Float,
                label: "MI (SEI)",
                name: "Maintainability (SEI)",
                short: "MI SEI",
                description: "SEI variant of the Maintainability Index — adds a bonus for comment density.",
                formula: "MI + 50·sin(√(2.4 × comment-ratio))",
                direction: HigherBetter,
                ..Default::default()
            },
        ),
        (
            "sloc",
            SpecRow {
                group: "loc",
                label: "Source",
                name: "Source lines",
                short: "SLOC",
                description: "Source lines of code — lines with at least one non-whitespace, non-comment character. Blank and comment-only lines are not counted (unlike `loc`, the raw file line count).",
                ..Default::default()
            },
        ),
        (
            "lloc",
            SpecRow {
                group: "loc",
                label: "Logical",
                name: "Logical lines",
                short: "Logical",
                description: "Logical lines — counts statements, not physical lines.",
                ..Default::default()
            },
        ),
        (
            "cloc",
            SpecRow {
                group: "loc",
                label: "Comments",
                name: "Comment lines",
                short: "Comments",
                description: "Comment-only lines (inline comments on code lines are not counted).",
                ..Default::default()
            },
        ),
        (
            "blank",
            SpecRow {
                group: "loc",
                label: "Blank",
                name: "Blank lines",
                short: "Blank",
                description: "Empty or whitespace-only lines.",
                ..Default::default()
            },
        ),
        (
            "tloc",
            SpecRow {
                group: "loc",
                label: "Test",
                name: "Test lines",
                short: "TLOC",
                description: "Test lines of code — the lines inside `#[cfg(test)]` / `#[test]` / `#[bench]` items (Rust), removed before the production metrics are measured. The complement of `sloc`: test code never inflates a file's size, HK, or complexity.",
                ..Default::default()
            },
        ),
        (
            "length",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Length",
                name: "Halstead length",
                short: "H.len",
                description: "Program length — total operator + operand occurrences.",
                formula: "N₁ + N₂",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "vocabulary",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Vocabulary",
                name: "Halstead vocabulary",
                short: "H.vocab",
                description: "Vocabulary — distinct operators + operands.",
                formula: "η₁ + η₂",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "volume",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Volume",
                name: "Halstead volume",
                short: "H.vol",
                description: "Algorithm size in bits, from distinct operators and operands.",
                formula: "length × log₂(vocabulary)",
                calc: "length * Math.log2(vocabulary)",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "effort",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Effort",
                name: "Halstead effort",
                short: "H.effort",
                description: "Mental effort to implement the algorithm.",
                formula: "volume × difficulty",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "time",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Time",
                name: "Halstead time, s",
                short: "H.time(s)",
                description: "Estimated implementation time, in seconds.",
                formula: "effort ÷ 18",
                calc: "effort / 18",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
        (
            "bugs",
            SpecRow {
                group: "halstead",
                value_type: Float,
                label: "Bugs",
                name: "Halstead bugs",
                short: "H.bugs",
                description: "Estimated delivered bugs — a rough predictor of defect density.",
                formula: "effort^⅔ ÷ 3000",
                calc: "effort ** (2/3) / 3000",
                direction: LowerBetter,
                ..Default::default()
            },
        ),
    ]);
    // Publish each metric's no-signal value on its spec, from the same
    // `metric_omit_at` the writers gate on — so the emitted JSON and the declared
    // spec agree.
    for (key, spec) in specs.iter_mut() {
        spec.omit_at = metric_omit_at(key);
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
