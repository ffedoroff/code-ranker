//! The metric catalog, read from `metrics/builtin.toml`: `[categories.*]`,
//! `[ast.*]` (tier-1 measured), `[fields.*]` (derived, each a `cel` formula), and
//! the view sections `[tableview]` / `[cardview]` / `[report.json.aggregate]`.
//! The crate root re-exports the accessors below; the tier-1 input types
//! (`MetricInputs` / `FunctionUnit`) come from `code-ranker-plugin-api`.
//!
//! Wire encoding:
//! - the executable `cel` formula is internal; the emitted [`AttributeSpec`]
//!   carries `formula` (from `formula_human`) and `calc` (from `formula_js`);
//! - `name` / `short` fall back to `label` (a field only spells out what differs);
//! - `\n` in a description (TOML multiline) is encoded as `<br>` on the wire;
//! - the `stats` block is produced by [`crate::stats::compute_stats`] over the
//!   keys whose `[report.json.aggregate]` entry is a plain mean
//!   (`agg('<k>','avg','not_empty')`); the richer aggregate formulas are parsed
//!   and available but not yet wired into the built-in stats.

use crate::attrs::num_attr;
use crate::registry::{Engine, MetricDef};
use code_ranker_plugin_api::metrics::MetricInputs;
use code_ranker_plugin_api::{
    PromptTemplate,
    attrs::ValueType,
    level::{AttributeGroup, AttributeSpec, CycleKindSpec, Direction},
    node::Node,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static BUILTIN_TOML: &str = include_str!("../metrics/builtin.toml");

/// One metric entry in `[ast.*]` (measured) or `[fields.*]` (derived). All spec
/// fields are optional; a pure AST input carries only a `description`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDef {
    #[serde(default = "crate::registry::default_value_type")]
    value_type: String,
    label: Option<String>,
    name: Option<String>,
    short: Option<String>,
    description: Option<String>,
    /// How to fix a breach — the `fix` line in `check` diagnostics.
    remediation: Option<String>,
    /// Executable CEL formula (derived `[fields.*]` only).
    cel: Option<String>,
    /// Pretty display formula (NOT CEL) — emitted as `AttributeSpec.formula`.
    formula_human: Option<String>,
    /// JS the viewer can re-run — emitted as `AttributeSpec.calc`.
    formula_js: Option<String>,
    direction: Option<String>,
    category: Option<String>,
    /// Format large values with K/M suffixes (e.g. `hk`).
    abbreviate: Option<bool>,
    #[serde(default = "crate::registry::default_omit_at")]
    omit_at: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TableView {
    #[serde(default)]
    columns: Vec<String>,
    /// Signed-rank default sort: order = priority, leading `-` = descending.
    #[serde(default)]
    default_sort: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CardView {
    #[serde(default)]
    featured: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MapView {
    /// Attribute keys the SVG map offers as circle-size modes (default `sloc`/`hk`).
    #[serde(default)]
    size: Vec<String>,
    /// Attribute keys the SVG map offers as on/off node filters (default `cycle`).
    #[serde(default)]
    filter: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Report {
    #[serde(default)]
    json: ReportJson,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ReportJson {
    /// `output key → graph-scope CEL formula` (the `stats` block of the report).
    #[serde(default)]
    aggregate: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Builtin {
    #[serde(default)]
    categories: BTreeMap<String, AttributeGroup>,
    #[serde(default)]
    ast: BTreeMap<String, FieldDef>,
    #[serde(default)]
    fields: BTreeMap<String, FieldDef>,
    /// Coupling/cycle specs (`fan_in` / `fan_out` / `hk` / `cycle`): display
    /// specs only — their values are computed post-walk, not by the CEL engine.
    #[serde(default)]
    coupling: BTreeMap<String, FieldDef>,
    /// Cycle-kind diagnostic vocab (`mutual` / `chain`): label + why + fix,
    /// overlaid onto each level's cycle_kinds by the orchestrator.
    #[serde(default)]
    cycles: BTreeMap<String, CycleKindSpec>,
    /// Prompt-Generator framing prose, carried into the snapshot.
    #[serde(default)]
    prompt: PromptTemplate,
    #[serde(default)]
    tableview: TableView,
    #[serde(default)]
    cardview: CardView,
    #[serde(default)]
    mapview: MapView,
    #[serde(default)]
    report: Report,
}

static BUILTIN: LazyLock<Builtin> =
    LazyLock::new(|| toml::from_str(BUILTIN_TOML).expect("metrics/builtin.toml parses"));

/// Compiled derived engine: `[fields.*]` `cel` formulas, with each metric's
/// `omit_at` from its spec, compiled once. Returned together with the defs (for
/// the per-key `omit_at` used when writing values).
static DERIVED: LazyLock<(BTreeMap<String, MetricDef>, Engine)> = LazyLock::new(|| {
    let defs: BTreeMap<String, MetricDef> = BUILTIN
        .fields
        .iter()
        .filter_map(|(k, f)| {
            f.cel
                .as_ref()
                .map(|cel| (k.clone(), derived_def(cel, f.omit_at)))
        })
        .collect();
    let engine = Engine::compile(&defs).expect("metrics/builtin.toml fields compile");
    (defs, engine)
});

fn derived_def(cel: &str, omit_at: f64) -> MetricDef {
    MetricDef {
        formula: cel.to_string(),
        value_type: "float".to_string(),
        omit_at,
        ..MetricDef::default()
    }
}

/// The canonical view orders read from `builtin.toml`. `columns` and `featured`
/// are flat ordered lists (they may include non-field tokens like `kind`,
/// `cycle` and coupling keys); `default_sort` is the signed-rank list.
#[derive(Debug, Clone, Default)]
pub struct Views {
    pub columns: Vec<String>,
    pub default_sort: Vec<String>,
    pub featured: Vec<String>,
    /// Map circle-size modes (attribute keys); built-in default `sloc`/`hk`.
    pub size: Vec<String>,
    /// Map node-filter keys; built-in default `cycle`.
    pub filter: Vec<String>,
}

/// The canonical view orders (table columns + default sort, card featured).
pub fn views() -> Views {
    Views {
        columns: BUILTIN.tableview.columns.clone(),
        default_sort: BUILTIN.tableview.default_sort.clone(),
        featured: BUILTIN.cardview.featured.clone(),
        size: BUILTIN.mapview.size.clone(),
        filter: BUILTIN.mapview.filter.clone(),
    }
}

/// Re-encode a TOML multiline description (`\n` paragraph breaks) as the `<br>`
/// the wire/viewer expects.
fn br(s: &str) -> String {
    s.replace('\n', "<br>")
}

fn value_type(s: &str) -> ValueType {
    match s {
        "int" => ValueType::Int,
        "bool" => ValueType::Bool,
        "str" | "string" => ValueType::Str,
        _ => ValueType::Float,
    }
}

fn direction(s: Option<&str>) -> Direction {
    match s {
        Some("lower_better") => Direction::LowerBetter,
        Some("higher_better") => Direction::HigherBetter,
        _ => Direction::Neutral,
    }
}

/// Build the emitted [`AttributeSpec`] from a metric entry, applying the
/// `name`/`short` ← `label` fallback, the `formula_human`→`formula` /
/// `formula_js`→`calc` mapping, and the `\n`→`<br>` description re-encoding.
fn to_spec(d: &FieldDef) -> AttributeSpec {
    AttributeSpec {
        value_type: value_type(&d.value_type),
        label: d.label.clone(),
        name: d.name.clone().or_else(|| d.label.clone()),
        short: d.short.clone().or_else(|| d.label.clone()),
        description: d.description.as_deref().map(br),
        remediation: d.remediation.as_deref().map(br),
        formula: d.formula_human.clone(),
        calc: d.formula_js.clone(),
        direction: direction(d.direction.as_deref()),
        abbreviate: d.abbreviate,
        group: d.category.clone(),
        thresholds: None,
        omit_at: d.omit_at,
    }
}

/// The metric attribute dictionary + category groups, read from `builtin.toml`.
/// Includes the emitted measured metrics (`[ast.*]` entries that carry a display
/// spec, i.e. have a `label`) and every derived `[fields.*]` metric. Pure AST
/// inputs (no `label`) are excluded — they are formula inputs, not emitted.
pub fn metric_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    let mut specs = BTreeMap::new();
    for (k, d) in &BUILTIN.ast {
        if d.label.is_some() {
            specs.insert(k.clone(), to_spec(d));
        }
    }
    for (k, d) in &BUILTIN.fields {
        specs.insert(k.clone(), to_spec(d));
    }
    (specs, BUILTIN.categories.clone())
}

/// The coupling/cycle attribute dictionary (`fan_in` / `fan_out` /
/// `fan_out_external` / `hk` / `cycle`) + the `coupling` group, read from
/// `builtin.toml` `[coupling.*]`. The VALUES are computed post-walk by
/// `annotate_hk` / `annotate_cycles`; these are the display specs only (incl. the
/// `description` = `why` and `remediation` = `fix` shown by `check`). The
/// orchestrator merges them into each level's `node_attributes` / groups.
pub fn coupling_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    let specs = BUILTIN
        .coupling
        .iter()
        .map(|(k, d)| (k.clone(), to_spec(d)))
        .collect();
    let mut groups = BTreeMap::new();
    if let Some(g) = BUILTIN.categories.get("coupling") {
        groups.insert("coupling".to_string(), g.clone());
    }
    (specs, groups)
}

/// The cycle-kind diagnostic vocabulary (`mutual` / `chain`) from `builtin.toml`
/// `[cycles.*]` — label + `description` (why) + `remediation` (fix). The
/// orchestrator overlays these onto each level's `cycle_kinds`.
pub fn cycle_specs() -> BTreeMap<String, CycleKindSpec> {
    BUILTIN.cycles.clone()
}

/// The Prompt-Generator scaffolding prose from `builtin.toml` `[prompt]` — the
/// language-neutral framing carried in the snapshot so the CLI `prompt` format
/// and the HTML viewer render the same text.
pub fn prompt_template() -> PromptTemplate {
    BUILTIN.prompt.clone()
}

/// The metric keys aggregated into the per-graph `stats` block via the mean
/// (`compute_stats`). Derived from `[report.json.aggregate]`: the keys whose
/// formula is a plain mean of their own metric over `not_empty`
/// (`agg('<k>', 'avg', 'not_empty')`). The richer aggregate formulas (percentiles,
/// `all` population, …) are parsed but not yet wired into the built-in stats.
pub fn stat_keys() -> Vec<String> {
    BUILTIN
        .report
        .json
        .aggregate
        .iter()
        .filter(|(k, formula)| **formula == format!("agg('{k}', 'avg', 'not_empty')"))
        .map(|(k, _)| k.clone())
        .collect()
}

/// All `[report.json.aggregate]` formulas (`output key → graph-scope CEL`). Parsed
/// and available for the future graph-scope aggregate engine; not yet driving the
/// built-in `stats` block (see [`stat_keys`]).
pub fn aggregate_formulas() -> BTreeMap<String, String> {
    BUILTIN.report.json.aggregate.clone()
}

/// Write all built-in metrics for one unit onto `node`: the tier-1 measured
/// values (the LOC block is emitted only when `sloc > 0`) plus the derived
/// metrics computed by the engine from `[fields.*]`. Each value is dropped at its
/// `omit_at`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_compiles() {
        let (specs, groups) = metric_specs();
        assert!(specs.contains_key("volume"), "derived present");
        assert!(specs.contains_key("sloc"), "emitted measured present");
        // Halstead/AST base counts are now emitted (they carry a label), so the
        // derived formulas can render a live derivation line in the viewer.
        assert!(
            specs.contains_key("eta1"),
            "base count emitted (has a display spec)"
        );
        assert!(groups.contains_key("halstead"));
        let (defs, _engine) = &*DERIVED;
        assert!(defs.contains_key("volume") && defs.contains_key("cyclomatic"));
    }

    #[test]
    fn spec_field_mapping_is_wire_compatible() {
        let (specs, _) = metric_specs();
        let vol = &specs["volume"];
        // formula_human → formula, formula_js → calc.
        assert_eq!(vol.formula.as_deref(), Some("length × log₂(vocabulary)"));
        assert_eq!(vol.calc.as_deref(), Some("length * Math.log2(vocabulary)"));
        // name/short fall back to label where the TOML omits them.
        let clo = &specs["closures"];
        assert_eq!(clo.name.as_deref(), Some("Closures"));
        assert_eq!(clo.short.as_deref(), Some("Closures"));
        // multiline description re-encoded with <br>, no raw newlines.
        let cog = &specs["cognitive"];
        let desc = cog.description.as_deref().unwrap();
        assert!(desc.contains("<br>") && !desc.contains('\n'));
    }

    #[test]
    fn stat_keys_are_the_mean_aggregates() {
        let keys = stat_keys();
        // The 17 reproduced means (incl. coupling), not the richer examples.
        assert!(keys.contains(&"cyclomatic".to_string()));
        assert!(keys.contains(&"hk".to_string()));
        assert!(
            !keys
                .iter()
                .any(|k| k.contains("_all_") || k.ends_with("_p99"))
        );
    }

    #[test]
    fn derives_tier2_from_tier1() {
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
        assert_eq!(node.attrs.get("cyclomatic"), Some(&num_attr(3.0)));
        let want = 87.0_f64 * 23.0_f64.log2();
        assert_eq!(node.attrs.get("volume"), Some(&num_attr(want)));
    }
}
