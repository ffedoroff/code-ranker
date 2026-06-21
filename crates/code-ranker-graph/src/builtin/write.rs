//! Computing built-in metric values onto a node — the per-unit derivation engines
//! and the `write_metrics` / `write_derived` entry points. Split out of the catalog
//! module (`super`) so each file stays a small, single-concern unit. Reads the
//! catalog (`super::BUILTIN` / `super::FieldDef`) via the parent module; the
//! `super::` and `mod` edges are non-flow, so this split adds no coupling.
//!
//! Two engines, by input dependency:
//! - [`DERIVED`] — `[fields.*]` over tier-1 counts only, evaluated per file from the
//!   raw (unrounded) inputs in [`write_metrics`] (the pre-graph tier-2 step);
//! - [`GRAPH_DERIVED`] — `[fields.*]` that read a graph-measured key (e.g. `hk` over
//!   `fan_in`/`fan_out`), evaluated by [`write_derived`] after the coupling pass.

use super::{BUILTIN, FieldDef};
use crate::attrs::num_attr;
use crate::registry::{Engine, MetricDef, references};
use code_ranker_plugin_api::attrs::AttrValue;
use code_ranker_plugin_api::metrics::MetricInputs;
use code_ranker_plugin_api::node::Node;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Graph-measured attribute keys a `[fields.*]` formula may read. A field whose
/// formula references any of these is **graph-derived**: it is evaluated after the
/// coupling/cycle graph pass (by [`write_derived`]), not in the per-file tier-2
/// step ([`write_metrics`]), which runs before the graph exists.
const GRAPH_KEYS: &[&str] = &["fan_in", "fan_out", "fan_out_external", "cycle"];

/// Does this formula read a graph-measured key (→ must run after the graph pass)?
fn is_graph_derived(formula: &str) -> bool {
    GRAPH_KEYS.iter().any(|k| references(formula, k))
}

/// One `[fields.*]` engine over the subset of fields selected by `keep` (by their
/// `formula_cel`), paired with the defs (for the per-key `omit_at` when writing).
fn build_engine(keep: impl Fn(&str) -> bool) -> (BTreeMap<String, MetricDef>, Engine) {
    let defs: BTreeMap<String, MetricDef> = BUILTIN
        .fields
        .iter()
        .filter_map(|(k, f): (&String, &FieldDef)| {
            f.formula_cel
                .as_ref()
                .filter(|cel| keep(cel))
                .map(|cel| (k.clone(), derived_def(cel, f.omit_at)))
        })
        .collect();
    let engine = Engine::compile(&defs).expect("metrics/builtin.toml fields compile");
    (defs, engine)
}

/// Pre-graph derived engine: the `[fields.*]` whose formula reads only tier-1
/// counts, evaluated per file from the raw (unrounded) inputs in [`write_metrics`].
pub(crate) static DERIVED: LazyLock<(BTreeMap<String, MetricDef>, Engine)> =
    LazyLock::new(|| build_engine(|cel| !is_graph_derived(cel)));

/// Post-graph derived engine: the `[fields.*]` whose formula reads a graph-measured
/// key (e.g. `hk` over `fan_in`/`fan_out`), evaluated by [`write_derived`] once the
/// coupling pass has annotated those counts onto the nodes.
static GRAPH_DERIVED: LazyLock<(BTreeMap<String, MetricDef>, Engine)> =
    LazyLock::new(|| build_engine(is_graph_derived));

fn derived_def(cel: &str, omit_at: f64) -> MetricDef {
    MetricDef {
        formula_cel: cel.to_string(),
        value_type: "float".to_string(),
        omit_at,
        ..MetricDef::default()
    }
}

/// Write the per-unit built-in metrics onto `node`: the tier-1 measured values
/// (the LOC block is emitted only when `sloc > 0`) plus the **pre-graph** derived
/// metrics ([`DERIVED`]) computed from the raw tier-1 counts. Graph-derived fields
/// (e.g. `hk`) are written later by [`write_derived`], once the coupling pass has
/// run. Each value is dropped at its `omit_at`.
pub fn write_metrics(node: &mut Node, i: &MetricInputs) {
    {
        let mut put = |key: &str, v: f64| {
            let a = num_attr(v);
            if a == num_attr(0.0) {
                node.attrs.remove(key);
            } else {
                node.attrs.insert(key.to_string(), a);
            }
        };
        put("cognitive", i.cognitive);
        put("exits", i.exits);
        put("args", i.args);
        put("closures", i.closures);
        // Halstead/AST base counts — emitted so derived formulas can show their
        // live derivation line in the viewer (each dropped at 0 by `put`).
        put("eta1", i.eta1);
        put("eta2", i.eta2);
        put("n1", i.n1);
        put("n2", i.n2);
        put("spaces", i.spaces);
        put("branches", i.branches);
        put("span_sloc", i.span_sloc);
        if i.sloc > 0.0 {
            put("sloc", i.sloc);
            put("lloc", i.lloc);
            put("cloc", i.cloc);
            put("blank", i.blank);
        }
        put("tloc", i.tloc);
    }

    let (defs, engine) = &*DERIVED;
    // Built-in derived metrics are pure arithmetic over the tier-1 counts — no
    // path/string inputs needed.
    for (key, value) in engine.eval_node(&inputs_map(i), &BTreeMap::new()) {
        let omit = defs.get(&key).map(|d| d.omit_at).unwrap_or(0.0);
        let a = num_attr(value);
        if a == num_attr(omit) {
            node.attrs.remove(&key);
        } else {
            node.attrs.insert(key, a);
        }
    }
}

/// The tier-1 inputs as a name→value map (the variables derived formulas read).
fn inputs_map(i: &MetricInputs) -> BTreeMap<String, f64> {
    BTreeMap::from([
        ("eta1".to_string(), i.eta1),
        ("eta2".to_string(), i.eta2),
        ("n1".to_string(), i.n1),
        ("n2".to_string(), i.n2),
        ("spaces".to_string(), i.spaces),
        ("branches".to_string(), i.branches),
        ("cognitive".to_string(), i.cognitive),
        ("exits".to_string(), i.exits),
        ("args".to_string(), i.args),
        ("closures".to_string(), i.closures),
        ("sloc".to_string(), i.sloc),
        ("lloc".to_string(), i.lloc),
        ("cloc".to_string(), i.cloc),
        ("blank".to_string(), i.blank),
        ("tloc".to_string(), i.tloc),
        ("span_sloc".to_string(), i.span_sloc),
    ])
}

/// Keys a graph-derived formula may read, seeded to `0` so an absent (no-signal,
/// dropped) attribute resolves the same way the complete tier-1 map does in
/// [`write_metrics`] — otherwise a formula referencing e.g. `fan_in` on an
/// uncoupled node would fail to resolve the variable (→ the field is omitted)
/// instead of evaluating it at zero.
const GRAPH_DERIVED_SEED: &[&str] = &[
    "eta1",
    "eta2",
    "n1",
    "n2",
    "spaces",
    "branches",
    "cognitive",
    "exits",
    "args",
    "closures",
    "sloc",
    "lloc",
    "cloc",
    "blank",
    "tloc",
    "span_sloc",
    "fan_in",
    "fan_out",
    "fan_out_external",
];

/// Compute the **graph-derived** built-in metrics ([`GRAPH_DERIVED`], e.g. `hk`)
/// onto `node`, AFTER the coupling pass has written `fan_in`/`fan_out`. Reads the
/// node's own (already-rounded) numeric attributes — exactly the values the former
/// Rust `hk` read via `attr_f64` — so emitted values are unchanged. Pre-graph
/// fields (volume/mi/…) are NOT recomputed here: they are written by
/// [`write_metrics`] from the raw, unrounded tier-1 counts and must not be
/// re-derived from the rounded node attributes. Each value is dropped at its
/// `omit_at`. No-op when no `[fields.*]` is graph-derived.
pub fn write_derived(node: &mut Node) {
    let (defs, engine) = &*GRAPH_DERIVED;
    if defs.is_empty() {
        return;
    }
    let mut attrs: BTreeMap<String, f64> = GRAPH_DERIVED_SEED
        .iter()
        .map(|k| ((*k).to_string(), 0.0))
        .collect();
    for (k, v) in &node.attrs {
        match v {
            AttrValue::Int(i) => {
                attrs.insert(k.clone(), *i as f64);
            }
            AttrValue::Float(f) => {
                attrs.insert(k.clone(), *f);
            }
            AttrValue::Str(_) | AttrValue::Bool(_) => {}
        }
    }
    for (key, value) in engine.eval_node(&attrs, &BTreeMap::new()) {
        let omit = defs.get(&key).map(|d| d.omit_at).unwrap_or(0.0);
        let a = num_attr(value);
        if a == num_attr(omit) {
            node.attrs.remove(&key);
        } else {
            node.attrs.insert(key, a);
        }
    }
}
