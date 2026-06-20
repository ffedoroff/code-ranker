//! Pure numeric / formula evaluation helpers for the metric [`super::Engine`].
//!
//! Behavior-preserving extraction of the registry's stateless building blocks:
//! formula compilation ordering ([`topo_order`] + [`references`]), program
//! execution coercion ([`exec_f64`]), the host-function registrars
//! ([`register_math`] / [`register_agg`]), and the population reducers
//! ([`reduce`] / [`percentile`]). Kept in a sibling module so the parent file
//! stays focused on the [`super::Engine`] / [`super::Populations`] data types.

use super::model::{MetricDef, RegistryError};
use cel::{Context, Program, Value};
use std::collections::BTreeMap;

/// Register `agg(key, reducer, population)` bound to this graph's populations.
/// `reducer` is `sum`/`avg`/`mean`/`min`/`max`/`count`/`median`/`p<q>`;
/// `population` is `all`/`not_empty`. An empty population or unknown reducer
/// yields `NaN` (→ the metric is omitted), never a panic.
pub(crate) fn register_agg(ctx: &mut Context, pops: std::sync::Arc<Populations>) {
    use std::sync::Arc;
    ctx.add_function(
        "agg",
        move |key: Arc<String>, reducer: Arc<String>, population: Arc<String>| -> f64 {
            pops.reduce_for(&key, &reducer, &population)
        },
    );
}

/// Reduce a value population to a scalar. Empty population or unknown reducer →
/// `None` (→ omit). Percentiles use the numpy-default linear interpolation (R-7).
pub(crate) fn reduce(vals: &[f64], reducer: &str) -> Option<f64> {
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
pub(crate) fn percentile(vals: &[f64], q: f64) -> Option<f64> {
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
pub(crate) fn exec_f64(program: &Program, ctx: &Context) -> Option<f64> {
    match program.execute(ctx) {
        Ok(Value::Float(v)) if v.is_finite() => Some(v),
        Ok(Value::Int(v)) => Some(v as f64),
        Ok(Value::UInt(v)) => Some(v as f64),
        // Non-finite floats and non-numeric results carry no signal.
        _ => None,
    }
}

/// Register the math host functions CEL itself lacks (each the exact `f64` op the
/// Rust engines use, so a transcribed formula is bit-identical). Shared by the
/// metric engine and the `[rules.checks]` predicate context (see
/// [`crate::checks`]) so both speak the same arithmetic.
pub(crate) fn register_math(ctx: &mut Context) {
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
pub(crate) fn references(formula: &str, key: &str) -> bool {
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
pub(crate) fn topo_order(
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

/// The value populations an aggregate reduces over, per metric key. Two flavours
/// per the metric's no-signal value (`omit_at`):
/// - `not_empty` — only nodes whose value carries signal (≠ `omit_at`);
/// - `all` — every applicable (internal) node, missing values counted at the
///   floor (`omit_at`), so e.g. files with zero coupling honestly weigh in.
///
/// Lives beside [`reduce`]/[`register_agg`] (its reduce target) so the trio stays
/// in one module rather than splitting a mutual dependency across files.
#[derive(Debug, Clone, Default)]
pub struct Populations {
    not_empty: BTreeMap<String, Vec<f64>>,
    all: BTreeMap<String, Vec<f64>>,
}

impl Populations {
    /// Reduce one metric's population to a scalar, as the `agg(key, reducer,
    /// population)` host function does. Unknown key / empty population / unknown
    /// reducer → `NaN`. Pure (no interior state), so a caller may memoize the
    /// result — it is identical for every node in a run.
    pub(crate) fn reduce_for(&self, key: &str, reducer: &str, population: &str) -> f64 {
        let table = match population {
            "all" => &self.all,
            _ => &self.not_empty,
        };
        table
            .get(key)
            .and_then(|vals| reduce(vals, reducer))
            .unwrap_or(f64::NAN)
    }

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
