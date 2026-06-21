//! The metric catalog, read from `metrics/builtin.toml`: `[categories.*]`,
//! `[ast.*]` (tier-1 measured), `[fields.*]` (derived, each a `formula_cel`), and
//! the `[report]` view section (+ `[report.stats]` aggregate formulas).
//! The crate root re-exports the accessors below; the tier-1 input types
//! (`MetricInputs` / `FunctionUnit`) come from `code-ranker-plugin-api`.
//!
//! Wire encoding:
//! - the executable `formula_cel` is internal; the emitted [`AttributeSpec`]
//!   carries `formula` (from `formula_pretty`) and `calc` (from `formula_js`);
//! - `name` / `short` fall back to `label` (a field only spells out what differs);
//! - `\n` in a description (TOML multiline) is encoded as `<br>` on the wire;
//! - the `stats` block is produced by [`crate::stats::compute_stats`] over the
//!   keys whose `[report.stats]` entry is a plain mean
//!   (`agg('<k>','avg','not_empty')`); the richer aggregate formulas are parsed
//!   and available but not yet wired into the built-in stats.

use code_ranker_plugin_api::{
    PromptTemplate,
    attrs::ValueType,
    level::{AttributeGroup, AttributeSpec, CycleKindSpec, Direction},
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static BUILTIN_TOML: &str = include_str!("../metrics/builtin.toml");

/// The Prompt-Generator scaffolding prose, authored as Markdown (`## <field>`
/// sections) rather than TOML so it reads naturally and edits like the rest of the
/// corpus. Parsed by [`prompt_template`].
static PROMPT_MD: &str = include_str!("../metrics/prompt.md");

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
    formula_cel: Option<String>,
    /// Pretty display formula (NOT CEL) — emitted as `AttributeSpec.formula`.
    formula_pretty: Option<String>,
    /// JS the viewer can re-run — emitted as `AttributeSpec.calc`.
    formula_js: Option<String>,
    direction: Option<String>,
    category: Option<String>,
    /// Format large values with K/M suffixes (e.g. `hk`).
    abbreviate: Option<bool>,
    #[serde(default = "crate::registry::default_omit_at")]
    omit_at: f64,
}

/// The `[report]` view section of `builtin.toml` — the SAME shape (and key names)
/// the project-side `[report]` override uses, so the vocabulary matches end to
/// end (catalog → `ReportOverride` → `LevelUi` → JSON `ui` → viewer): `columns`,
/// `card`, `size`, `filter` (+ the `default_sort` signed-rank list), and the
/// `[report.stats]` aggregate formulas.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportView {
    /// Node-table column order (may include non-field tokens like `kind`/`cycle`).
    #[serde(default)]
    columns: Vec<String>,
    /// Signed-rank default sort: order = priority, leading `-` = descending.
    #[serde(default)]
    default_sort: Vec<String>,
    /// Card-featured metrics (the big numbers on a node's card).
    #[serde(default)]
    card: Vec<String>,
    /// Attribute keys the SVG map offers as circle-size modes (default `sloc`/`hk`).
    #[serde(default)]
    size: Vec<String>,
    /// Attribute keys the SVG map offers as on/off node filters (default `cycle`).
    #[serde(default)]
    filter: Vec<String>,
    /// `output key → graph-scope CEL formula` for the report's `stats` block.
    #[serde(default)]
    stats: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Builtin {
    #[serde(default)]
    categories: BTreeMap<String, AttributeGroup>,
    #[serde(default)]
    ast: BTreeMap<String, FieldDef>,
    #[serde(default)]
    fields: BTreeMap<String, FieldDef>,
    /// Coupling/cycle specs (`fan_in` / `fan_out` / `fan_out_external` / `cycle`):
    /// display specs only — their values are computed post-walk, not by the CEL
    /// engine. (`hk` folds these into a graph-derived `[fields.hk]` formula.)
    #[serde(default)]
    coupling: BTreeMap<String, FieldDef>,
    /// Cycle-kind diagnostic vocab (`mutual` / `chain`): label + why + fix,
    /// overlaid onto each level's cycle_kinds by the orchestrator.
    #[serde(default)]
    cycles: BTreeMap<String, CycleKindSpec>,
    #[serde(default)]
    report: ReportView,
}

static BUILTIN: LazyLock<Builtin> =
    LazyLock::new(|| toml::from_str(BUILTIN_TOML).expect("metrics/builtin.toml parses"));

/// Computing metric values onto a node — the per-unit derivation engines and the
/// `write_metrics` / `write_derived` entry points, kept in their own file so this
/// module stays the catalog/spec concern. The `mod`/`super` edges are non-flow.
mod write;
pub use write::{write_derived, write_metrics};

/// The canonical view orders read from `builtin.toml`. `columns` and `featured`
/// are flat ordered lists (they may include non-field tokens like `kind`,
/// `cycle` and coupling keys); `default_sort` is the signed-rank list.
#[derive(Debug, Clone, Default)]
pub struct Views {
    pub columns: Vec<String>,
    pub default_sort: Vec<String>,
    pub card: Vec<String>,
    /// Map circle-size modes (attribute keys); built-in default `sloc`/`hk`.
    pub size: Vec<String>,
    /// Map node-filter keys; built-in default `cycle`.
    pub filter: Vec<String>,
}

/// The canonical view orders (table columns + default sort, card metrics, map
/// size/filter), all from the single `[report]` section of `builtin.toml`.
pub fn views() -> Views {
    Views {
        columns: BUILTIN.report.columns.clone(),
        default_sort: BUILTIN.report.default_sort.clone(),
        card: BUILTIN.report.card.clone(),
        size: BUILTIN.report.size.clone(),
        filter: BUILTIN.report.filter.clone(),
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
/// `name`/`short` ← `label` fallback, the `formula_pretty`→`formula` /
/// `formula_js`→`calc` mapping, and the `\n`→`<br>` description re-encoding.
fn to_spec(d: &FieldDef) -> AttributeSpec {
    AttributeSpec {
        value_type: value_type(&d.value_type),
        label: d.label.clone(),
        name: d.name.clone().or_else(|| d.label.clone()),
        short: d.short.clone().or_else(|| d.label.clone()),
        description: d.description.as_deref().map(br),
        remediation: d.remediation.as_deref().map(br),
        formula: d.formula_pretty.clone(),
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
/// `fan_out_external` / `cycle`) + the `coupling` group, read from
/// `builtin.toml` `[coupling.*]`. The VALUES are computed post-walk by
/// `annotate_coupling` / `annotate_cycles`; these are the display specs only (incl.
/// the `description` = `why` and `remediation` = `fix` shown by `check`). The
/// orchestrator merges them into each level's `node_attributes` / groups. (`hk`'s
/// spec ships with the derived `[fields.*]` via [`metric_specs`].)
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

/// The Prompt-Generator scaffolding prose, parsed from `metrics/prompt.md` — the
/// language-neutral framing carried in the snapshot so the CLI `prompt` format and
/// the HTML viewer render the same text. Each `## <field>` section maps to a
/// [`PromptTemplate`] field; `## task` keeps one entry per non-blank line
/// (verbatim, including the leading `- `), the rest join their body into one line.
pub fn prompt_template() -> PromptTemplate {
    parse_prompt(PROMPT_MD)
}

/// Parse caller-supplied prompt-scaffolding Markdown (same `## <field>` shape as
/// the built-in `metrics/prompt.md`) into a [`PromptTemplate`] — the hook a
/// `[templates] prompt = "…"` config override flows through.
pub fn prompt_template_from(md: &str) -> PromptTemplate {
    parse_prompt(md)
}

/// Parse the `## <field>` sections of `metrics/prompt.md` into a [`PromptTemplate`].
fn parse_prompt(md: &str) -> PromptTemplate {
    let mut t = PromptTemplate::default();
    let mut field = String::new();
    let mut body: Vec<&str> = Vec::new();
    let flush = |field: &str, body: &[&str], t: &mut PromptTemplate| {
        let nonblank = || body.iter().filter(|l| !l.trim().is_empty());
        match field {
            "intro" => t.intro = nonblank().cloned().collect::<Vec<_>>().join(" "),
            "doc_note" => t.doc_note = nonblank().cloned().collect::<Vec<_>>().join(" "),
            "focus" => t.focus = nonblank().cloned().collect::<Vec<_>>().join(" "),
            "cycle_note" => t.cycle_note = nonblank().cloned().collect::<Vec<_>>().join(" "),
            "task" => t.task = nonblank().map(|l| l.trim_end().to_string()).collect(),
            _ => {}
        }
    };
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            flush(&field, &body, &mut t);
            field = h.trim().to_string();
            body.clear();
        } else if !field.is_empty() {
            body.push(line);
        }
    }
    flush(&field, &body, &mut t);
    t
}

/// The metric keys aggregated into the per-graph `stats` block via the mean
/// (`compute_stats`). Derived from `[report.stats]`: the keys whose
/// formula is a plain mean of their own metric over `not_empty`
/// (`agg('<k>', 'avg', 'not_empty')`). The richer aggregate formulas (percentiles,
/// `all` population, …) are parsed but not yet wired into the built-in stats.
pub fn stat_keys() -> Vec<String> {
    BUILTIN
        .report
        .stats
        .iter()
        .filter(|(k, formula)| **formula == format!("agg('{k}', 'avg', 'not_empty')"))
        .map(|(k, _)| k.clone())
        .collect()
}

/// All `[report.stats]` formulas (`output key → graph-scope CEL`). Parsed and
/// available for the future graph-scope aggregate engine; not yet driving the
/// built-in `stats` block (see [`stat_keys`]).
pub fn aggregate_formulas() -> BTreeMap<String, String> {
    BUILTIN.report.stats.clone()
}

#[cfg(test)]
#[path = "builtin_test.rs"]
mod tests;
