//! Registry data types — the leaf definitions the [`super::Engine`] and the
//! evaluation helpers ([`super::eval`]) both build on.
//!
//! Kept in a dependency-free leaf module (it imports only external crates, never
//! `super`) so that `eval` can depend on it and the parent can depend on both
//! without forming a module cycle. Re-exported from `super` so existing call
//! sites and the `use super::*` test modules see these types unchanged.

use code_ranker_plugin_api::{
    attrs::ValueType,
    level::{AttributeSpec, Direction, Thresholds},
};
use serde::Deserialize;
use std::sync::LazyLock;

/// Where a metric is evaluated: per node (default) or once over a collection.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Node,
    Graph,
}

/// One metric definition: a CEL formula plus the spec fields needed to emit it
/// as a first-class, sortable, delta-coloured attribute. Spec fields are
/// optional so a quick user formula needs only `formula`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDef {
    /// CEL expression over other metric keys + the registered math functions.
    pub formula: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default = "default_value_type")]
    pub value_type: String,
    // (the `omit_at` field below also defaults from the registry `[defaults]`.)
    pub label: Option<String>,
    pub name: Option<String>,
    pub short: Option<String>,
    pub description: Option<String>,
    /// How to fix a breach — the `fix` line in `check` diagnostics.
    pub remediation: Option<String>,
    /// Human-readable formula shown in the viewer (display only).
    pub formula_pretty: Option<String>,
    /// JS expression the viewer re-runs with the node's values to show the live
    /// "formula = numbers" line (like a built-in's `formula_js`). When omitted, a
    /// node-scope metric falls back to its CEL `formula` — valid JS for plain
    /// arithmetic / ternaries; if it uses CEL-only host functions (`log2`, `pow`,
    /// …) the viewer simply skips the line. Set `calc` explicitly to control it.
    pub calc: Option<String>,
    /// `lower_better` / `higher_better`.
    pub direction: Option<String>,
    pub group: Option<String>,
    /// No-signal value at which the metric is omitted (the registry `[defaults]`
    /// `omit_at` when unset).
    #[serde(default = "default_omit_at")]
    pub omit_at: f64,
    /// Two-tier severity thresholds (the `warning` / `info` limits the scorecard
    /// and viewer badge against, like a built-in metric). When either is set the
    /// metric carries a [`Thresholds`] in its spec; the other tier falls back to
    /// it. Distinct from the `[rules.thresholds.file]` single-tier `check` gate.
    pub warning: Option<f64>,
    pub info: Option<f64>,
}

/// The registry `[defaults]` block from `metrics/builtin.toml`: the field-omission
/// fallbacks (`value_type` / `omit_at`) a metric entry inherits when it doesn't
/// set the field. The SINGLE source of these values — no literal in Rust. Parsed
/// independently of the full [`crate::builtin`] catalog (it reads only `[defaults]`,
/// so it does not re-enter that catalog's lazy parse) and shared by both the
/// built-in `[ast.*]`/`[fields.*]` entries and a user's `[metrics.<key>]`.
static FIELD_DEFAULTS: LazyLock<FieldDefaults> = LazyLock::new(|| {
    #[derive(Deserialize)]
    struct Wrap {
        defaults: FieldDefaults,
    }
    toml::from_str::<Wrap>(include_str!("../../metrics/builtin.toml"))
        .expect("metrics/builtin.toml [defaults] parses")
        .defaults
});

#[derive(Debug, Clone, Deserialize)]
struct FieldDefaults {
    value_type: String,
    omit_at: f64,
}

/// Default `value_type` for a metric entry that omits it (registry `[defaults]`).
pub(crate) fn default_value_type() -> String {
    FIELD_DEFAULTS.value_type.clone()
}

/// Default `omit_at` for a metric entry that omits it (registry `[defaults]`).
pub(crate) fn default_omit_at() -> f64 {
    FIELD_DEFAULTS.omit_at
}

impl MetricDef {
    /// The two-tier severity thresholds, if the metric declares either tier. A
    /// missing tier mirrors the other, so `warning = 1.0` alone yields
    /// `{ warning: 1.0, info: 1.0 }` (one effective tier).
    fn thresholds(&self) -> Option<Thresholds> {
        match (self.warning, self.info) {
            (None, None) => None,
            (w, i) => Some(Thresholds {
                warning: w.or(i).unwrap_or(0.0),
                info: i.or(w).unwrap_or(0.0),
            }),
        }
    }

    /// The viewer-facing [`AttributeSpec`] for this metric, so a config-defined
    /// metric renders as a named, sortable, delta-coloured column like any
    /// built-in — including the live "formula = numbers" tooltip line, driven by
    /// `calc` (defaulted from the CEL `formula` for node-scope metrics).
    pub fn to_attribute_spec(&self) -> AttributeSpec {
        let value_type = match self.value_type.as_str() {
            "int" => ValueType::Int,
            "bool" => ValueType::Bool,
            "str" | "string" => ValueType::Str,
            _ => ValueType::Float,
        };
        let direction = match self.direction.as_deref() {
            Some("lower_better") => Direction::LowerBetter,
            Some("higher_better") => Direction::HigherBetter,
            _ => Direction::Neutral,
        };
        AttributeSpec {
            value_type,
            label: self.label.clone(),
            name: self.name.clone(),
            short: self.short.clone(),
            description: self.description.clone(),
            remediation: self.remediation.clone(),
            formula: self.formula_pretty.clone(),
            // Node-scope metric: default the live-derivation JS to the CEL formula
            // (valid JS for arithmetic; the viewer no-ops if it can't run it). A
            // graph aggregate isn't shown per node, so it carries no `calc`.
            calc: self
                .calc
                .clone()
                .or_else(|| (self.scope == Scope::Node).then(|| self.formula.clone())),
            direction,
            abbreviate: None,
            group: self.group.clone(),
            thresholds: self.thresholds(),
            omit_at: self.omit_at,
        }
    }
}

/// Errors surfaced when loading/compiling a registry — all caught at load time
/// (not per node), so a bad user formula fails fast with a clear message.
#[derive(Debug)]
pub enum RegistryError {
    /// A `formula` failed to parse as CEL.
    Parse { key: String, message: String },
    /// The metric dependency graph has a cycle (`a` ← `b` ← `a`).
    Cycle { keys: Vec<String> },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Parse { key, message } => {
                write!(f, "metric `{key}`: invalid CEL formula: {message}")
            }
            RegistryError::Cycle { keys } => {
                write!(
                    f,
                    "metric formulas form a dependency cycle: {}",
                    keys.join(" → ")
                )
            }
        }
    }
}

impl std::error::Error for RegistryError {}
