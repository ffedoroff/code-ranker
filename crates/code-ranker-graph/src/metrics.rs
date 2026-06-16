//! Language-neutral metric scaffolding. Tier-1 counts ([`MetricInputs`]) are
//! measured by the per-language engines; **every tier-2 metric (its formula AND
//! its spec) is data**, defined in `metrics/builtin.toml` and computed by the
//! CEL [`registry`](crate::registry) engine — no derived-metric name is hardcoded
//! in Rust. [`write_metrics`] writes the tier-1 measured values plus the
//! registry-derived tier-2 values onto a node; [`metric_specs`] / [`stat_keys`]
//! expose the catalog read from the file.
//!
//! The per-language engines (`rust_ts` / `python_ts` / `ecmascript_ts`) live in
//! the language crates; each produces a `MetricInputs` and calls [`write_metrics`].

use crate::attrs::num_attr;
use crate::registry::{Engine, MetricDef, Scope};
use code_ranker_plugin_api::{
    level::{AttributeGroup, AttributeSpec},
    node::Node,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Raw tier-1 counts a per-language engine measures for one unit (a file or, for
/// the `functions` level, a function). Every tier-2 metric is a pure function of
/// these, evaluated by the built-in registry — see `metrics/builtin.toml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricInputs {
    /// Halstead base counts (η₁/η₂/N₁/N₂), as floats (counts are small integers).
    pub eta1: f64,
    pub eta2: f64,
    pub n1: f64,
    pub n2: f64,
    /// Structural counts.
    pub spaces: f64,
    pub branches: f64,
    pub cognitive: f64,
    pub exits: f64,
    pub args: f64,
    pub closures: f64,
    /// LOC breakdown.
    pub sloc: f64,
    pub lloc: f64,
    pub cloc: f64,
    pub blank: f64,
    pub tloc: f64,
    /// Unit span sloc (`end_row − start_row`) — an MI input, not emitted itself.
    pub span_sloc: f64,
}

/// One sub-file unit (a function / method / closure) with its tier-1 counts.
/// Produced by a language engine's `compute_functions` and turned into a node on
/// the optional `functions` level via [`write_metrics`]. `kind` is a free-form,
/// per-language string (`fn` / `method` / `closure` / `lambda` / …).
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionUnit {
    pub kind: String,
    pub name: String,
    /// 1-based inclusive line span.
    pub start_line: u32,
    pub end_line: u32,
    pub inputs: MetricInputs,
}

// ── Built-in metric registry (loaded from data, not hardcoded) ───────────────

static BUILTIN_TOML: &str = include_str!("../metrics/builtin.toml");

#[derive(Debug, Deserialize)]
struct Builtin {
    #[serde(default)]
    stat: Vec<String>,
    #[serde(default)]
    ui: UiOrder,
    #[serde(default)]
    formulas: BTreeMap<String, String>,
    #[serde(default)]
    groups: BTreeMap<String, AttributeGroup>,
    #[serde(default)]
    specs: BTreeMap<String, AttributeSpec>,
}

/// Canonical UI render orders (column / summary / sort / size / card), read from
/// `metrics/builtin.toml`. The orchestrator prunes each to the keys present.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UiOrder {
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub sort: Vec<String>,
    #[serde(default)]
    pub size: Vec<String>,
    #[serde(default)]
    pub card: Vec<String>,
}

static BUILTIN: LazyLock<Builtin> =
    LazyLock::new(|| toml::from_str(BUILTIN_TOML).expect("metrics/builtin.toml parses"));

/// The compiled tier-2 registry: `MetricDef`s built from the file's `[formulas]`
/// (with each metric's `omit_at` taken from its spec), compiled once.
static BUILTIN_ENGINE: LazyLock<(BTreeMap<String, MetricDef>, Engine)> = LazyLock::new(|| {
    let defs: BTreeMap<String, MetricDef> = BUILTIN
        .formulas
        .iter()
        .map(|(k, f)| {
            (
                k.clone(),
                MetricDef {
                    formula: f.clone(),
                    scope: Scope::Node,
                    value_type: "float".to_string(),
                    label: None,
                    name: None,
                    short: None,
                    description: None,
                    formula_pretty: None,
                    direction: None,
                    group: None,
                    omit_at: BUILTIN.specs.get(k).map(|s| s.omit_at).unwrap_or(0.0),
                },
            )
        })
        .collect();
    let engine = Engine::compile(&defs).expect("metrics/builtin.toml formulas compile");
    (defs, engine)
});

/// Write all built-in metrics for one unit onto `node`: the tier-1 measured
/// values (LOC block gated on `sloc > 0`, mirroring historical behaviour) plus
/// the tier-2 metrics computed by the registry from `metrics/builtin.toml`. Each
/// value is dropped at its `omit_at`. No tier-2 metric name appears here — they
/// all flow from the data file.
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
        if i.sloc > 0.0 {
            put("sloc", i.sloc);
            put("lloc", i.lloc);
            put("cloc", i.cloc);
            put("blank", i.blank);
        }
        put("tloc", i.tloc);
    }

    let (defs, engine) = &*BUILTIN_ENGINE;
    for (key, value) in engine.eval_node(&inputs_map(i)) {
        let omit = defs.get(&key).map(|d| d.omit_at).unwrap_or(0.0);
        let a = num_attr(value);
        if a == num_attr(omit) {
            node.attrs.remove(&key);
        } else {
            node.attrs.insert(key, a);
        }
    }
}

/// The tier-1 inputs as a name→value map (the variables tier-2 formulas read).
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

/// The metric attribute dictionary + groups, read from `metrics/builtin.toml`.
/// The orchestrator merges these into each level's `node_attributes` /
/// `attribute_groups` (then prunes to keys present) and overlays thresholds.
pub fn metric_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    (BUILTIN.specs.clone(), BUILTIN.groups.clone())
}

/// The metric keys aggregated into the per-graph `stats` block (from the data
/// file). Coupling stat keys are added by the orchestrator.
pub fn stat_keys() -> Vec<String> {
    BUILTIN.stat.clone()
}

/// The canonical UI render orders, read from `metrics/builtin.toml`.
pub fn ui_order() -> UiOrder {
    BUILTIN.ui.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_loads_and_compiles() {
        // The data file parses and every formula compiles (catches a typo here
        // at test time, not at first use).
        let (defs, _engine) = &*BUILTIN_ENGINE;
        assert!(defs.contains_key("volume"));
        assert!(!BUILTIN.specs.is_empty());
        assert!(!BUILTIN.stat.is_empty());
    }

    #[test]
    fn derives_tier2_from_tier1() {
        // A worked file unit: 87 operand+operator occurrences, vocab 23 → volume.
        let i = MetricInputs {
            eta1: 10.0,
            eta2: 13.0,
            n1: 40.0,
            n2: 47.0,
            spaces: 1.0,
            branches: 2.0,
            span_sloc: 20.0,
            sloc: 18.0,
            cloc: 4.0,
            ..Default::default()
        };
        let mut node = Node {
            id: "x".into(),
            kind: "file".into(),
            name: "x".into(),
            parent: None,
            attrs: Default::default(),
        };
        write_metrics(&mut node, &i);
        // cyclomatic = spaces + branches = 3
        assert_eq!(node.attrs.get("cyclomatic"), Some(&num_attr(3.0)));
        // volume = (n1+n2) * log2(eta1+eta2) = 87 * log2(23)
        let want = 87.0_f64 * 23.0_f64.log2();
        assert_eq!(node.attrs.get("volume"), Some(&num_attr(want)));
    }
}
