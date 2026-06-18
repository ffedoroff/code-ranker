//! Level descriptors + the **semantics dictionaries** that let the core handle
//! unknown kinds/keys without hardcoding their names, and let the UI render any
//! language/metric set purely from data: edge kinds ([`EdgeKindSpec`]),
//! node/edge attributes ([`AttributeSpec`], grouped via [`AttributeGroup`]),
//! node kinds ([`NodeKindSpec`]) and cycle kinds ([`CycleKindSpec`]).
//!
//! The dictionaries are **maps** keyed by the kind/attribute/group name; the
//! spec value holds only the remaining metadata.

use crate::attrs::ValueType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Semantics of one edge kind. Keyed by the edge `kind` in
/// [`Level::edge_kinds`]. `flow` is the single source of truth for "is this
/// information flow": counted in coupling/cycles AND drawn when `true`;
/// structural (e.g. `contains`) and excluded/hidden when `false`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeKindSpec {
    pub flow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Long human description (used as a UI tooltip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A named group of attributes (UI section). Keyed by group name in
/// [`Level::attribute_groups`]; attributes reference it via
/// [`AttributeSpec::group`]. Metadata only — storage stays flat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Two-tier per-metric thresholds (at/under `info` is fine; above `warning` is
/// likely a problem). Carried on an [`AttributeSpec`]; produced by a plugin
/// (language-calibrated), absent when a metric has no calibration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Thresholds {
    pub info: f64,
    pub warning: f64,
}

/// Whether a metric's delta is "good" when it moves up or down — drives the
/// green/red colouring in the viewer. `Neutral` (the default) means the metric
/// has no agreed-good direction (raw sizes, structural counts) and is left
/// uncoloured. `Neutral` is skipped on the wire, so a neutral metric serializes
/// exactly as the old `Option<String>` form did: the `direction` field absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Neutral,
    LowerBetter,
    HigherBetter,
}

impl Direction {
    /// `true` for the default (no opinion) — used by serde to omit the field.
    pub fn is_neutral(&self) -> bool {
        matches!(self, Direction::Neutral)
    }
}

/// Describes one attribute key (on a node or an edge). Everything the UI needs
/// to label, explain, format, compute and threshold the metric — so the viewer
/// hardcodes no metric by name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSpec {
    pub value_type: ValueType,
    /// Concise display label (table grouping, popup rows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Full name used as a tooltip title (falls back to `label`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Short label for narrow table headers (falls back to `label`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    /// Long human description (tooltip body); the `why` line in `check` diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How to fix a breach of this metric — the `fix` line in `check` diagnostics.
    /// Data, not code: lives in the metric catalog / language config, not the CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    /// Human-readable formula, e.g. `"sloc × (fan_in × fan_out)²"` (display only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// Evaluable JS expression over sibling attribute names + `Math`, e.g.
    /// `"sloc * (fan_in * fan_out) ** 2"`. Lets the UI show the live derivation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calc: Option<String>,
    /// Whether higher or lower is "better" — drives delta colouring. `Neutral`
    /// (the default) is omitted from the wire.
    #[serde(default, skip_serializing_if = "Direction::is_neutral")]
    pub direction: Direction,
    /// Format large values with K/M suffixes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviate: Option<bool>,
    /// Optional group this attribute belongs to, by [`AttributeGroup`] key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Optional two-tier thresholds (language-calibrated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thresholds: Option<Thresholds>,
    /// Value at which this metric carries no signal and is **omitted** from the
    /// node (absent in the JSON, blank in the viewer). `0` for almost everything;
    /// `1` for `cyclomatic` (McCabe's floor — a function-less file would otherwise
    /// report a vacuous `1`). Published so the frontend knows what an absent cell
    /// means (and may treat it as this value when sorting). Default `0` is omitted
    /// from the wire.
    #[serde(default, skip_serializing_if = "f64_is_zero")]
    pub omit_at: f64,
}

/// `skip_serializing_if` helper: the default `omit_at` (`0`) stays off the wire.
fn f64_is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl AttributeSpec {
    /// A minimal spec with just a type + label (the common structural case).
    pub fn new(value_type: ValueType, label: &str) -> Self {
        Self {
            value_type,
            label: Some(label.to_string()),
            name: None,
            short: None,
            description: None,
            remediation: None,
            formula: None,
            calc: None,
            direction: Direction::Neutral,
            abbreviate: None,
            group: None,
            thresholds: None,
            omit_at: 0.0,
        }
    }
}

/// One row of a declarative attribute table: a flat, named-field description of
/// an [`AttributeSpec`]. Empty `&str` fields become `None`; `direction` defaults
/// to [`Direction::Neutral`]. Build a whole dictionary with [`attr_dict`].
///
/// This replaces both the per-field `spec.x = Some(...)` boilerplate and the
/// positional metric-row tuples that the metric/coupling crates used to carry
/// separately, so every centrally-computed spec is declared the same way.
#[derive(Clone)]
pub struct SpecRow {
    pub group: &'static str,
    pub value_type: ValueType,
    pub label: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    pub description: &'static str,
    pub remediation: &'static str,
    pub formula: &'static str,
    pub calc: &'static str,
    pub direction: Direction,
    pub abbreviate: bool,
    /// See [`AttributeSpec::omit_at`]. Defaults to `0`.
    pub omit_at: f64,
}

impl Default for SpecRow {
    fn default() -> Self {
        SpecRow {
            group: "",
            value_type: ValueType::Int,
            label: "",
            name: "",
            short: "",
            description: "",
            remediation: "",
            formula: "",
            calc: "",
            direction: Direction::Neutral,
            abbreviate: false,
            omit_at: 0.0,
        }
    }
}

impl SpecRow {
    fn into_spec(self) -> AttributeSpec {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        AttributeSpec {
            value_type: self.value_type,
            label: opt(self.label),
            name: opt(self.name),
            short: opt(self.short),
            description: opt(self.description),
            remediation: opt(self.remediation),
            formula: opt(self.formula),
            calc: opt(self.calc),
            direction: self.direction,
            abbreviate: self.abbreviate.then_some(true),
            group: opt(self.group),
            thresholds: None,
            omit_at: self.omit_at,
        }
    }
}

/// Assemble a `key → AttributeSpec` dictionary from a declarative table.
pub fn attr_dict(rows: Vec<(&'static str, SpecRow)>) -> BTreeMap<String, AttributeSpec> {
    rows.into_iter()
        .map(|(k, r)| (k.to_string(), r.into_spec()))
        .collect()
}

/// Build an [`AttributeGroup`] from a label + description.
pub fn group(label: &str, description: &str) -> AttributeGroup {
    AttributeGroup {
        label: Some(label.to_string()),
        description: Some(description.to_string()),
    }
}

/// Visual + label semantics of one node kind (`"file"` / `"external"` / …).
/// Keyed by kind in [`Level::node_kinds`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeKindSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plural: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    /// `true` marks a third-party node (a library); the UI derives "external
    /// edge" from the endpoint kind, not from any edge flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
}

/// Label + description of one cycle kind (`"mutual"` / `"chain"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleKindSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How to fix this cycle kind — the `fix` line in `check` diagnostics. Data,
    /// not code: lives in the shared `defaults.toml` `[cycle_kinds]`, not the CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// How the viewer should cluster nodes in the diagram. Exactly one of `key`
/// (group by the value of a node attribute, e.g. `crate`) or `function` (a named
/// grouper the viewer implements, e.g. `dir` — derive the folder from the path).
/// Absent → the viewer falls back to its default `dir` grouper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grouping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// An analysis level the plugin can produce, with the semantics needed to score
/// and draw it. The orchestrator merges in centrally-computed attribute specs
/// and the computed `ui` block before writing the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    pub edge_kinds: BTreeMap<String, EdgeKindSpec>,
    pub node_attributes: BTreeMap<String, AttributeSpec>,
    pub edge_attributes: BTreeMap<String, AttributeSpec>,
    pub attribute_groups: BTreeMap<String, AttributeGroup>,
    /// Node-kind vocabulary (label/colour/external). Plugins seed it from
    /// [`crate::default_node_kinds`] and may customize.
    #[serde(default)]
    pub node_kinds: BTreeMap<String, NodeKindSpec>,
    /// Cycle-kind vocabulary. Plugins seed it from [`crate::default_cycle_kinds`].
    #[serde(default)]
    pub cycle_kinds: BTreeMap<String, CycleKindSpec>,
    /// How the viewer should cluster nodes (defaults to `dir` when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<Grouping>,
}
