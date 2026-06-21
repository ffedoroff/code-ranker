//! Declarative metric registry — the data-driven home for tier-2+ formulas.
//!
//! A [`MetricDef`] pairs a CEL `formula_cel` with its spec (label / direction / …).
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
use cel::{Context, Program};
use code_ranker_plugin_api::{attrs::AttrValue, node::Node};
use std::collections::BTreeMap;

/// Registry data types (extracted to a dependency-free leaf so `eval` can build
/// on them without a module cycle). Re-exported so existing call sites and the
/// `use super::*` test modules compile unchanged.
mod model;
pub use model::{MetricDef, RegistryError, Scope};
// `builtin.rs` reaches these serde-default fns via `crate::registry::…`.
pub(crate) use model::{default_omit_at, default_value_type};

/// Pure numeric / formula evaluation helpers + [`Populations`] (extracted to keep
/// this file's aggregate complexity in check). Re-exported so existing call sites
/// and the `use super::*` test modules compile unchanged.
mod eval;
pub use eval::Populations;
use eval::{exec_f64, register_agg, topo_order};
pub(crate) use eval::{references, register_math};
// Used only by the `use super::*` test submodules below.
#[cfg(test)]
use eval::{percentile, reduce};

/// Apply the node-scope engine to one node: read its numeric attributes, compute
/// every registry metric, and write the results back, omitting each at its
/// `omit_at` exactly like [`crate::write_metrics`]. Built-in attributes already
/// present are used as inputs.
pub fn apply_to_node(node: &mut Node, defs: &BTreeMap<String, MetricDef>, engine: &Engine) {
    let mut attrs: BTreeMap<String, f64> = BTreeMap::new();
    let mut strings: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &node.attrs {
        match v {
            AttrValue::Int(i) => {
                attrs.insert(k.clone(), *i as f64);
            }
            AttrValue::Float(f) => {
                attrs.insert(k.clone(), *f);
            }
            AttrValue::Str(s) => {
                strings.insert(k.clone(), s.clone());
            }
            AttrValue::Bool(_) => {}
        }
    }
    // Derived path fields (`path`/`name`/`stem`/`ext`/`dir`) so a formula can
    // branch on the file's location, the same vars `[rules.checks]` sees.
    for (k, v) in crate::nodepath::path_fields(node) {
        strings.insert(k.to_string(), v);
    }
    for (key, value) in engine.eval_node(&attrs, &strings) {
        let omit = defs.get(&key).map(|d| d.omit_at).unwrap_or(0.0);
        let a = num_attr(value);
        if a == num_attr(omit) {
            node.attrs.remove(&key);
        } else {
            node.attrs.insert(key, a);
        }
    }
}

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

    /// Evaluate every node-scope metric over `attrs` (a node's numeric values)
    /// plus `strings` (its string values + derived path fields, so a formula can
    /// branch on `path`/`name`/… e.g. `path.contains("/generated/") ? 0.0 : hk`).
    /// Returns only the **newly computed** keys. A formula that errors or yields a
    /// non-finite value contributes nothing (the metric is omitted for that node),
    /// mirroring the viewer's `evalCalc` and `omit_at` semantics.
    pub fn eval_node(
        &self,
        attrs: &BTreeMap<String, f64>,
        strings: &BTreeMap<String, String>,
    ) -> BTreeMap<String, f64> {
        // One context per node: the host functions ([`register_math`]) and the
        // node's inputs are set up once, then each computed metric is fed back
        // into the SAME context so later (dependency-ordered) formulas read it —
        // no per-formula context rebuild or function re-registration. The compiled
        // `Program`s are reused as-is (see [`Engine::compile`]).
        let mut ctx = Context::default();
        register_math(&mut ctx);
        for (k, v) in attrs {
            let _ = ctx.add_variable(k.as_str(), *v);
        }
        for (k, v) in strings {
            let _ = ctx.add_variable(k.as_str(), v.clone());
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
        register_math(&mut ctx);
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
        let program = Program::compile(&def.formula_cel).map_err(|e| RegistryError::Parse {
            key: key.clone(),
            message: e.to_string(),
        })?;
        programs.push((key, program));
    }
    Ok(programs)
}

#[cfg(test)]
#[path = "registry_test.rs"]
mod tests;

#[cfg(test)]
#[path = "registry_cover_test.rs"]
mod cover_tests;
