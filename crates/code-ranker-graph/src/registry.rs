//! Declarative metric registry — the data-driven home for tier-2+ formulas.
//!
//! A [`MetricDef`] pairs a CEL `formula` with its spec (label / direction / …).
//! The default registry ships built-in derived metrics; a user adds their own by
//! editing config (e.g. `comment_ratio = "sloc > 0.0 ? cloc / sloc * 100.0 :
//! 0.0"`). The [`Engine`] compiles each formula once, topologically orders them
//! by inter-metric dependencies, and evaluates them per node over the node's
//! numeric attributes — the same engine will serve file and function units.
//!
//! Scope: today the engine evaluates **node-scope** metrics and is used to
//! compute *extra* metrics on top of the built-in Rust derivation, so the
//! default pipeline (and its goldens) are untouched until this is wired in.
//! Graph-scope aggregates (`scope = graph`, percentiles, reducers) come later.

use crate::attrs::num_attr;
use cel::{Context, Program, Value};
use code_ranker_plugin_api::{
    attrs::{AttrValue, ValueType},
    level::{AttributeSpec, Direction, Thresholds},
    node::Node,
};
use serde::Deserialize;
use std::collections::BTreeMap;
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
    toml::from_str::<Wrap>(include_str!("../metrics/builtin.toml"))
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

/// Apply the node-scope engine to one node: read its numeric attributes, compute
/// every registry metric, and write the results back, omitting each at its
/// `omit_at` exactly like [`crate::write_metrics`]. Built-in attributes already
/// present are used as inputs.
pub fn apply_to_node(node: &mut Node, defs: &BTreeMap<String, MetricDef>, engine: &Engine) {
    let mut attrs: BTreeMap<String, f64> = BTreeMap::new();
    for (k, v) in &node.attrs {
        match v {
            AttrValue::Int(i) => {
                attrs.insert(k.clone(), *i as f64);
            }
            AttrValue::Float(f) => {
                attrs.insert(k.clone(), *f);
            }
            _ => {}
        }
    }
    for (key, value) in engine.eval_node(&attrs) {
        let omit = defs.get(&key).map(|d| d.omit_at).unwrap_or(0.0);
        let a = num_attr(value);
        if a == num_attr(omit) {
            node.attrs.remove(&key);
        } else {
            node.attrs.insert(key, a);
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

/// A compiled, topologically ordered set of metric programs, split by scope:
/// node-scope (per node) and graph-scope (once over the whole node set, via the
/// `agg(key, reducer, population)` reducer function).
pub struct Engine {
    /// `(key, compiled program)` in dependency order (inputs before dependents).
    node_programs: Vec<(String, Program)>,
    graph_programs: Vec<(String, Program)>,
}

impl Engine {
    /// Compile and order both scopes of a registry. Detects formula parse errors
    /// and dependency cycles (within each scope) up front.
    pub fn compile(defs: &BTreeMap<String, MetricDef>) -> Result<Engine, RegistryError> {
        Ok(Engine {
            node_programs: compile_scope(defs, Scope::Node)?,
            graph_programs: compile_scope(defs, Scope::Graph)?,
        })
    }

    /// Does this registry declare any graph-scope (aggregate) metrics?
    pub fn has_graph_metrics(&self) -> bool {
        !self.graph_programs.is_empty()
    }

    /// Evaluate every node-scope metric over `attrs` (a node's numeric values),
    /// returning only the **newly computed** keys. A formula that errors or
    /// yields a non-finite value contributes nothing (the metric is omitted for
    /// that node), mirroring the viewer's `evalCalc` and `omit_at` semantics.
    pub fn eval_node(&self, attrs: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
        // One context per node: the host functions ([`register_stdlib`]) and the
        // node's inputs are set up once, then each computed metric is fed back
        // into the SAME context so later (dependency-ordered) formulas read it —
        // no per-formula context rebuild or function re-registration. The compiled
        // `Program`s are reused as-is (see [`Engine::compile`]).
        let mut ctx = Context::default();
        register_stdlib(&mut ctx);
        for (k, v) in attrs {
            let _ = ctx.add_variable(k.as_str(), *v);
        }
        let mut produced: BTreeMap<String, f64> = BTreeMap::new();
        for (key, program) in &self.node_programs {
            if let Some(v) = exec_f64(program, &ctx) {
                let _ = ctx.add_variable(key.as_str(), v); // feed-forward to dependents
                produced.insert(key.clone(), v);
            }
        }
        produced
    }

    /// Evaluate the graph-scope (aggregate) metrics once over the whole node set,
    /// captured as [`Populations`]. Formulas use the `agg(key, reducer,
    /// population)` reducer function (e.g. `agg('cyclomatic', 'p90',
    /// 'not_empty')`) and may reference earlier graph metrics by name. Empty /
    /// non-finite results are omitted. Returns the produced aggregate values.
    pub fn eval_graph(&self, pops: &Populations) -> BTreeMap<String, f64> {
        // One context for the whole graph pass: host functions + the population
        // reducer (`agg`) are registered once, and each aggregate is fed back so
        // a later aggregate can reference it. Programs are compiled once.
        let mut ctx = Context::default();
        register_stdlib(&mut ctx);
        register_agg(&mut ctx, std::sync::Arc::new(pops.clone()));
        let mut produced: BTreeMap<String, f64> = BTreeMap::new();
        for (key, program) in &self.graph_programs {
            if let Some(v) = exec_f64(program, &ctx) {
                let _ = ctx.add_variable(key.as_str(), v); // feed-forward to dependents
                produced.insert(key.clone(), v);
            }
        }
        produced
    }
}

/// Compile and topologically order the metrics of one scope.
fn compile_scope(
    defs: &BTreeMap<String, MetricDef>,
    scope: Scope,
) -> Result<Vec<(String, Program)>, RegistryError> {
    let scoped: Vec<(&String, &MetricDef)> =
        defs.iter().filter(|(_, d)| d.scope == scope).collect();
    let keys: Vec<String> = scoped.iter().map(|(k, _)| (*k).clone()).collect();
    let order = topo_order(&scoped, &keys)?;
    let mut programs = Vec::with_capacity(order.len());
    for key in order {
        let def = defs.get(&key).expect("key from defs");
        let program = Program::compile(&def.formula).map_err(|e| RegistryError::Parse {
            key: key.clone(),
            message: e.to_string(),
        })?;
        programs.push((key, program));
    }
    Ok(programs)
}

/// The value populations an aggregate reduces over, per metric key. Two flavours
/// per the metric's no-signal value (`omit_at`):
/// - `not_empty` — only nodes whose value carries signal (≠ `omit_at`);
/// - `all` — every applicable (internal) node, missing values counted at the
///   floor (`omit_at`), so e.g. files with zero coupling honestly weigh in.
#[derive(Debug, Clone, Default)]
pub struct Populations {
    not_empty: BTreeMap<String, Vec<f64>>,
    all: BTreeMap<String, Vec<f64>>,
}

impl Populations {
    /// Build both populations from each applicable node's present numeric
    /// attributes (`rows`), the set of metric `keys`, and each key's `omit_at`.
    /// Aggregation is over true computed values: a node missing a key contributes
    /// the floor to `all` (and nothing to `not_empty`).
    pub fn build(
        rows: &[BTreeMap<String, f64>],
        keys: &[String],
        omit_at: &BTreeMap<String, f64>,
    ) -> Populations {
        let mut not_empty = BTreeMap::new();
        let mut all = BTreeMap::new();
        for key in keys {
            let omit = omit_at.get(key).copied().unwrap_or(0.0);
            let present: Vec<f64> = rows.iter().filter_map(|r| r.get(key).copied()).collect();
            let ne: Vec<f64> = present.iter().copied().filter(|v| *v != omit).collect();
            let missing = rows.len().saturating_sub(present.len());
            let mut a = present;
            a.extend(std::iter::repeat_n(omit, missing));
            not_empty.insert(key.clone(), ne);
            all.insert(key.clone(), a);
        }
        Populations { not_empty, all }
    }
}

/// Register `agg(key, reducer, population)` bound to this graph's populations.
/// `reducer` is `sum`/`avg`/`mean`/`min`/`max`/`count`/`median`/`p<q>`;
/// `population` is `all`/`not_empty`. An empty population or unknown reducer
/// yields `NaN` (→ the metric is omitted), never a panic.
fn register_agg(ctx: &mut Context, pops: std::sync::Arc<Populations>) {
    use std::sync::Arc;
    ctx.add_function(
        "agg",
        move |key: Arc<String>, reducer: Arc<String>, population: Arc<String>| -> f64 {
            let table = match population.as_str() {
                "all" => &pops.all,
                _ => &pops.not_empty,
            };
            table
                .get(key.as_str())
                .and_then(|vals| reduce(vals, reducer.as_str()))
                .unwrap_or(f64::NAN)
        },
    );
}

/// Reduce a value population to a scalar. Empty population or unknown reducer →
/// `None` (→ omit). Percentiles use the numpy-default linear interpolation (R-7).
fn reduce(vals: &[f64], reducer: &str) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    match reducer {
        "sum" => Some(vals.iter().sum()),
        "avg" | "mean" => Some(vals.iter().sum::<f64>() / vals.len() as f64),
        "min" => Some(vals.iter().copied().fold(f64::INFINITY, f64::min)),
        "max" => Some(vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "count" => Some(vals.len() as f64),
        "median" => percentile(vals, 50.0),
        // `top<N>` / `top<N>_<reducer>`: keep the N largest values, then apply the
        // base reducer (default `avg`). E.g. `top10_avg`, `top5_sum`, `top10_max`.
        _ if reducer
            .strip_prefix("top")
            .and_then(|rest| rest.split('_').next())
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
        {
            let rest = reducer.strip_prefix("top").unwrap();
            let (num, base) = match rest.split_once('_') {
                Some((n, b)) => (n, b),
                None => (rest, "avg"),
            };
            let n: usize = num.parse().ok()?;
            let mut s = vals.to_vec();
            s.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)); // desc
            s.truncate(n);
            reduce(&s, base)
        }
        r if r.starts_with('p') => r[1..].parse::<f64>().ok().and_then(|q| percentile(vals, q)),
        _ => None,
    }
}

/// The `q`-th percentile (0–100) by linear interpolation between closest ranks —
/// `numpy.percentile`'s default `method="linear"` (Hyndman–Fan type 7).
fn percentile(vals: &[f64], q: f64) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    let mut s = vals.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 1 {
        return Some(s[0]);
    }
    let h = (n as f64 - 1.0) * (q / 100.0);
    let lo = h.floor();
    let lo_i = lo as usize;
    let frac = h - lo;
    let v = if lo_i + 1 < n {
        s[lo_i] + frac * (s[lo_i + 1] - s[lo_i])
    } else {
        s[lo_i]
    };
    Some(v)
}

/// Execute a program against a context and coerce a finite numeric result to
/// `f64`. Any error / non-numeric / non-finite result → `None` (→ omit).
fn exec_f64(program: &Program, ctx: &Context) -> Option<f64> {
    match program.execute(ctx) {
        Ok(Value::Float(v)) if v.is_finite() => Some(v),
        Ok(Value::Int(v)) => Some(v as f64),
        Ok(Value::UInt(v)) => Some(v as f64),
        // Non-finite floats and non-numeric results carry no signal.
        _ => None,
    }
}

/// Register the host standard library available to every formula. Today that is
/// the math CEL lacks (each the exact `f64` op the Rust engines use, so a
/// transcribed formula is bit-identical); future non-math helpers belong here
/// too — hence the general name.
fn register_stdlib(ctx: &mut Context) {
    ctx.add_function("log2", |x: f64| x.log2());
    ctx.add_function("ln", |x: f64| x.ln());
    ctx.add_function("log10", |x: f64| x.log10());
    ctx.add_function("pow", |x: f64, y: f64| x.powf(y));
    ctx.add_function("sqrt", |x: f64| x.sqrt());
    ctx.add_function("sin", |x: f64| x.sin());
    ctx.add_function("cos", |x: f64| x.cos());
    ctx.add_function("abs", |x: f64| x.abs());
    ctx.add_function("min2", |x: f64, y: f64| x.min(y));
    ctx.add_function("max2", |x: f64, y: f64| x.max(y));
}

/// Whole-word membership: does `formula` reference identifier `key`? Mirrors the
/// viewer's `\bkey\b` scan — snake_case keys are safe (`mi` won't hit `mi_sei`).
fn references(formula: &str, key: &str) -> bool {
    let bytes = formula.as_bytes();
    let kb = key.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while let Some(pos) = formula[i..].find(key) {
        let start = i + pos;
        let end = start + kb.len();
        let before_ok = start == 0 || !is_word(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        i = start + 1;
    }
    false
}

/// Kahn topological sort over the metric dependency graph (edge `dep → key` when
/// `key`'s formula references `dep`). Returns evaluation order or a cycle error.
fn topo_order(
    defs: &[(&String, &MetricDef)],
    keys: &[String],
) -> Result<Vec<String>, RegistryError> {
    let keyset: std::collections::BTreeSet<&str> = keys.iter().map(|s| s.as_str()).collect();
    // deps[key] = set of other registry keys it references.
    let mut deps: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    let mut indeg: BTreeMap<String, usize> = BTreeMap::new();
    for k in keys {
        deps.entry(k.clone()).or_default();
        indeg.entry(k.clone()).or_insert(0);
    }
    for (key, def) in defs {
        for cand in &keyset {
            if *cand != key.as_str()
                && references(&def.formula, cand)
                && deps.get_mut(*key).unwrap().insert((*cand).to_string())
            {
                *indeg.get_mut(*key).unwrap() += 1;
            }
        }
    }
    // Kahn, draining zero-indegree keys (BTree iteration = stable order).
    let mut order = Vec::with_capacity(keys.len());
    loop {
        let ready: Vec<String> = indeg
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        if ready.is_empty() {
            break;
        }
        for k in ready {
            indeg.remove(&k);
            for (other, od) in deps.iter() {
                if od.contains(&k)
                    && let Some(d) = indeg.get_mut(other)
                {
                    *d -= 1;
                }
            }
            order.push(k);
        }
    }
    if order.len() != keys.len() {
        let mut remaining: Vec<String> = indeg.keys().cloned().collect();
        remaining.sort();
        return Err(RegistryError::Cycle { keys: remaining });
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(formula: &str) -> MetricDef {
        MetricDef {
            formula: formula.to_string(),
            value_type: "float".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn computes_a_simple_ratio() {
        let mut defs = BTreeMap::new();
        defs.insert(
            "comment_ratio".to_string(),
            def("sloc > 0.0 ? cloc / sloc * 100.0 : 0.0"),
        );
        let eng = Engine::compile(&defs).unwrap();
        let attrs = BTreeMap::from([("sloc".to_string(), 40.0), ("cloc".to_string(), 10.0)]);
        let out = eng.eval_node(&attrs);
        assert_eq!(out.get("comment_ratio"), Some(&25.0));
    }

    #[test]
    fn math_functions_match_rust_f64() {
        let mut defs = BTreeMap::new();
        // volume = length * log2(vocabulary) — same op as the built-in engine.
        defs.insert(
            "volume".to_string(),
            def("vocabulary > 0.0 ? length * log2(vocabulary) : 0.0"),
        );
        let eng = Engine::compile(&defs).unwrap();
        let attrs = BTreeMap::from([
            ("length".to_string(), 87.0),
            ("vocabulary".to_string(), 23.0),
        ]);
        let out = eng.eval_node(&attrs);
        let expect = 87.0_f64 * 23.0_f64.log2();
        assert_eq!(out.get("volume"), Some(&expect));
    }

    #[test]
    fn dependent_metrics_evaluate_in_order() {
        let mut defs = BTreeMap::new();
        defs.insert("length".to_string(), def("n1 + n2"));
        defs.insert("double_len".to_string(), def("length * 2.0"));
        let eng = Engine::compile(&defs).unwrap();
        let attrs = BTreeMap::from([("n1".to_string(), 3.0), ("n2".to_string(), 4.0)]);
        let out = eng.eval_node(&attrs);
        assert_eq!(out.get("length"), Some(&7.0));
        assert_eq!(out.get("double_len"), Some(&14.0));
    }

    #[test]
    fn detects_dependency_cycle() {
        let mut defs = BTreeMap::new();
        defs.insert("a".to_string(), def("b + 1.0"));
        defs.insert("b".to_string(), def("a + 1.0"));
        assert!(matches!(
            Engine::compile(&defs),
            Err(RegistryError::Cycle { .. })
        ));
    }

    #[test]
    fn invalid_formula_is_a_load_error() {
        let mut defs = BTreeMap::new();
        defs.insert("bad".to_string(), def("1 +"));
        assert!(matches!(
            Engine::compile(&defs),
            Err(RegistryError::Parse { .. })
        ));
    }

    fn graph_def(formula: &str) -> MetricDef {
        let mut d = def(formula);
        d.scope = Scope::Graph;
        d
    }

    fn rows(key: &str, vals: &[f64]) -> Vec<BTreeMap<String, f64>> {
        vals.iter()
            .map(|v| BTreeMap::from([(key.to_string(), *v)]))
            .collect()
    }

    #[test]
    fn percentile_matches_numpy_r7() {
        // numpy.percentile([10,20,30,100], 50) == 25.0 (linear interpolation).
        assert_eq!(percentile(&[10.0, 20.0, 30.0, 100.0], 50.0), Some(25.0));
        // p0 = min, p100 = max.
        assert_eq!(percentile(&[5.0, 1.0, 9.0], 0.0), Some(1.0));
        assert_eq!(percentile(&[5.0, 1.0, 9.0], 100.0), Some(9.0));
        // single element → itself for any q.
        assert_eq!(percentile(&[7.0], 90.0), Some(7.0));
    }

    #[test]
    fn graph_aggregate_over_population() {
        let mut defs = BTreeMap::new();
        defs.insert(
            "cyc_p90".to_string(),
            graph_def("agg('cyclomatic', 'p90', 'not_empty')"),
        );
        defs.insert(
            "cyc_mean".to_string(),
            graph_def("agg('cyclomatic', 'avg', 'not_empty')"),
        );
        let eng = Engine::compile(&defs).unwrap();
        assert!(eng.has_graph_metrics());
        let r = rows("cyclomatic", &[2.0, 4.0, 6.0, 8.0, 10.0]);
        let pops = Populations::build(&r, &["cyclomatic".to_string()], &BTreeMap::new());
        let out = eng.eval_graph(&pops);
        assert_eq!(out.get("cyc_mean"), Some(&6.0));
        // p90 of [2,4,6,8,10] (R-7): h=(5-1)*0.9=3.6 → 8 + 0.6*(10-8) = 9.2
        assert_eq!(out.get("cyc_p90"), Some(&9.2));
    }

    #[test]
    fn all_population_counts_missing_at_floor() {
        // 2 nodes have hk, 3 don't → `all` includes 3 zeros; `not_empty` only the 2.
        let mut r = rows("hk", &[100.0, 300.0]);
        r.push(BTreeMap::new());
        r.push(BTreeMap::new());
        r.push(BTreeMap::new());
        let pops = Populations::build(&r, &["hk".to_string()], &BTreeMap::new());
        let mut defs = BTreeMap::new();
        defs.insert(
            "hk_med_all".to_string(),
            graph_def("agg('hk','median','all')"),
        );
        defs.insert(
            "hk_med_ne".to_string(),
            graph_def("agg('hk','median','not_empty')"),
        );
        let out = Engine::compile(&defs).unwrap().eval_graph(&pops);
        // all = [0,0,0,100,300] → median 0; not_empty = [100,300] → median 200
        assert_eq!(out.get("hk_med_all"), Some(&0.0));
        assert_eq!(out.get("hk_med_ne"), Some(&200.0));
    }

    #[test]
    fn graph_metrics_compose() {
        // a ratio of two aggregates (graph metric referencing another graph metric).
        let mut defs = BTreeMap::new();
        defs.insert("total".to_string(), graph_def("agg('x','sum','not_empty')"));
        defs.insert("n".to_string(), graph_def("agg('x','count','not_empty')"));
        defs.insert("ratio".to_string(), graph_def("n > 0.0 ? total / n : 0.0"));
        let eng = Engine::compile(&defs).unwrap();
        let pops = Populations::build(
            &rows("x", &[2.0, 4.0, 6.0]),
            &["x".to_string()],
            &BTreeMap::new(),
        );
        let out = eng.eval_graph(&pops);
        assert_eq!(out.get("total"), Some(&12.0));
        assert_eq!(out.get("ratio"), Some(&4.0));
    }

    #[test]
    fn error_or_nonfinite_is_omitted() {
        let mut defs = BTreeMap::new();
        // references a missing variable → execution error → omitted, no panic.
        defs.insert("x".to_string(), def("missing_var + 1.0"));
        let eng = Engine::compile(&defs).unwrap();
        let out = eng.eval_node(&BTreeMap::new());
        assert!(!out.contains_key("x"));
    }
}

#[cfg(test)]
mod cover_tests {
    use super::*;
    use code_ranker_plugin_api::attrs::AttrValue;

    #[test]
    fn registry_error_display() {
        let p = RegistryError::Parse {
            key: "x".into(),
            message: "boom".into(),
        };
        assert!(format!("{p}").contains("x") && format!("{p}").contains("boom"));
        let c = RegistryError::Cycle {
            keys: vec!["a".into(), "b".into()],
        };
        assert!(format!("{c}").contains("a → b"));
    }

    #[test]
    fn reducers_and_percentile_edges() {
        assert_eq!(reduce(&[3.0, 1.0, 2.0], "min"), Some(1.0));
        assert_eq!(reduce(&[3.0, 1.0, 2.0], "max"), Some(3.0));
        assert_eq!(reduce(&[1.0, 2.0], "unknown_reducer"), None);
        assert_eq!(reduce(&[], "avg"), None);
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn top_n_reducer_keeps_largest_then_reduces() {
        let vals = [1.0, 5.0, 3.0, 9.0, 2.0, 8.0]; // top 3 = 9, 8, 5
        assert_eq!(reduce(&vals, "top3_avg"), Some((9.0 + 8.0 + 5.0) / 3.0));
        assert_eq!(reduce(&vals, "top3_max"), Some(9.0));
        assert_eq!(reduce(&vals, "top2_sum"), Some(17.0));
        assert_eq!(reduce(&vals, "top10"), reduce(&vals, "avg")); // N ≥ len, default avg
        assert_eq!(reduce(&vals, "top"), None); // no number → not a top reducer
    }

    #[test]
    fn exec_f64_handles_int_result() {
        // an int-literal formula yields a CEL Int → coerced to f64.
        let mut defs = BTreeMap::new();
        defs.insert(
            "two".to_string(),
            MetricDef {
                formula: "1 + 1".to_string(),
                value_type: "int".to_string(),
                ..Default::default()
            },
        );
        let eng = Engine::compile(&defs).unwrap();
        assert_eq!(eng.eval_node(&BTreeMap::new()).get("two"), Some(&2.0));
    }

    #[test]
    fn to_attribute_spec_maps_types_and_direction() {
        let mk = |vt: &str, dir: Option<&str>| MetricDef {
            formula: "0.0".to_string(),
            value_type: vt.to_string(),
            label: Some("L".into()),
            direction: dir.map(|s| s.to_string()),
            ..Default::default()
        };
        use code_ranker_plugin_api::attrs::ValueType;
        use code_ranker_plugin_api::level::Direction;
        assert_eq!(
            mk("int", None).to_attribute_spec().value_type,
            ValueType::Int
        );
        assert_eq!(
            mk("bool", None).to_attribute_spec().value_type,
            ValueType::Bool
        );
        assert_eq!(
            mk("str", None).to_attribute_spec().value_type,
            ValueType::Str
        );
        assert_eq!(
            mk("float", Some("higher_better"))
                .to_attribute_spec()
                .direction,
            Direction::HigherBetter
        );
        // unknown/absent direction → Neutral
        assert_eq!(
            mk("float", None).to_attribute_spec().direction,
            Direction::Neutral
        );
    }

    #[test]
    fn two_tier_thresholds_map_to_spec() {
        let with = |warning, info| MetricDef {
            formula: "0.0".to_string(),
            warning,
            info,
            ..Default::default()
        };
        // No tiers → no thresholds.
        assert!(with(None, None).to_attribute_spec().thresholds.is_none());
        // One tier mirrors into the other.
        let th = with(Some(1.5), None)
            .to_attribute_spec()
            .thresholds
            .unwrap();
        assert_eq!((th.warning, th.info), (1.5, 1.5));
        // Both tiers preserved.
        let th = with(Some(2.0), Some(1.0))
            .to_attribute_spec()
            .thresholds
            .unwrap();
        assert_eq!((th.warning, th.info), (2.0, 1.0));
    }

    #[test]
    fn calc_defaults_to_formula_for_node_scope() {
        // Node-scope: `calc` (the live derivation line) defaults to the CEL formula.
        let node = MetricDef {
            formula: "tloc / sloc".to_string(),
            ..Default::default()
        };
        assert_eq!(
            node.to_attribute_spec().calc.as_deref(),
            Some("tloc / sloc")
        );
        // Explicit `calc` wins over the formula fallback.
        let explicit = MetricDef {
            formula: "tloc / sloc".to_string(),
            calc: Some("tloc / sloc * 1.0".to_string()),
            ..Default::default()
        };
        assert_eq!(
            explicit.to_attribute_spec().calc.as_deref(),
            Some("tloc / sloc * 1.0")
        );
        // Graph-scope aggregate isn't shown per node → no calc.
        let agg = MetricDef {
            formula: "agg('x', 'avg', 'not_empty')".to_string(),
            scope: Scope::Graph,
            ..Default::default()
        };
        assert!(agg.to_attribute_spec().calc.is_none());
    }

    #[test]
    fn apply_to_node_writes_and_omits() {
        let mut defs = BTreeMap::new();
        defs.insert("ratio".to_string(), {
            let mut d = MetricDef {
                formula: "a * 2.0".to_string(),
                value_type: "float".to_string(),
                ..Default::default()
            };
            d.formula = "a * 2.0".to_string();
            d
        });
        let eng = Engine::compile(&defs).unwrap();
        let mut node = Node {
            id: "n".into(),
            kind: "file".into(),
            name: "n".into(),
            parent: None,
            attrs: Default::default(),
        };
        node.attrs.insert("a".into(), AttrValue::Int(3));
        // pre-seed `ratio` so the omit branch (result == omit_at) can remove it.
        node.attrs.insert("ratio".into(), AttrValue::Int(99));
        apply_to_node(&mut node, &defs, &eng);
        assert_eq!(node.attrs.get("ratio"), Some(&AttrValue::Int(6)));

        // now a formula that yields the omit value removes the attr.
        let mut zdefs = BTreeMap::new();
        zdefs.insert(
            "ratio".to_string(),
            MetricDef {
                formula: "0.0".to_string(),
                value_type: "float".to_string(),
                ..Default::default()
            },
        );
        let zeng = Engine::compile(&zdefs).unwrap();
        apply_to_node(&mut node, &zdefs, &zeng);
        assert!(!node.attrs.contains_key("ratio"));
    }
}
