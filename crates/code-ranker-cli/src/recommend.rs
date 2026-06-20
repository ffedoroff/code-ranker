//! The recommendation engine behind the `prompt` and `scorecard` report formats.
//!
//! It is the console counterpart of the HTML viewer's Prompt Generator: the same
//! ranking (`reco_for` ≈ `recoFor` in `export-popup.js`) and the same Markdown
//! prompt (`compose_prompt` ≈ `composePrompt` + `buildContent`), plus a console
//! triage table (`render_scorecard`) that mirrors the viewer's per-preset badges.
//!
//! All of it is **advisory**, derived from the snapshot's language-calibrated
//! `node_attributes[*].thresholds` (the `info` / `warning` tiers) — never a gate.

use anyhow::{Result, bail};
use code_ranker_graph::level_graph::{CycleGroup, LevelGraph};
use code_ranker_plugin_api::{Preset, attrs::AttrValue, level::Thresholds, node::Node};
use std::collections::HashMap;

mod prompt;
mod scorecard;

pub use prompt::compose_prompt;
pub use scorecard::render_scorecard;

/// Which threshold tier drives an output. `Auto` resolves to `Warning` when any
/// module breaches it, else `Info` (the viewer's headline rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Auto,
}

/// Parse a `--severity` value (`info` / `warning` / `auto`). Invalid is fatal —
/// the tool never silently ignores an unknown rule knob.
pub fn parse_severity(s: &str) -> Result<Severity> {
    match s {
        "info" => Ok(Severity::Info),
        "warning" => Ok(Severity::Warning),
        "auto" => Ok(Severity::Auto),
        other => bail!("invalid --severity '{other}': expected info, warning, or auto"),
    }
}

/// A single ranking metric's recommendation: the candidate file nodes sorted
/// worst-first, plus how many cross the `warning` / `info` tiers. For the pseudo
/// metric `"cycle"` the candidates are the nodes in a dependency cycle (ranked by
/// HK) and both counts equal that set's size.
pub struct Reco<'a> {
    pub sorted: Vec<&'a Node>,
    pub warning_count: usize,
    pub info_count: usize,
}

/// Read a numeric node attribute (`Int`/`Float`) as `f64`, else `None`.
pub(super) fn num(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// A project source file (not a third-party library node).
pub(super) fn is_internal(node: &Node) -> bool {
    node.kind != "external"
}

/// Is this file node in a dependency cycle? (the orchestrator writes a `cycle`
/// string attribute on every cycle member).
pub(super) fn in_cycle(node: &Node) -> bool {
    matches!(node.attrs.get("cycle"), Some(AttrValue::Str(_)))
}

/// The two-tier thresholds for a metric: the metric's own, falling back to HK's,
/// then to a never-breached `{0, 0}` — mirroring the viewer's `recoFor`.
fn thresholds_for(level: &LevelGraph, metric: &str) -> Thresholds {
    level
        .node_attributes
        .get(metric)
        .and_then(|s| s.thresholds)
        .or_else(|| level.node_attributes.get("hk").and_then(|s| s.thresholds))
        .unwrap_or(Thresholds {
            info: 0.0,
            warning: 0.0,
        })
}

/// The short header label for a metric (falls back to its label, then the key).
pub(super) fn attr_short<'a>(level: &'a LevelGraph, metric: &'a str) -> &'a str {
    level
        .node_attributes
        .get(metric)
        .and_then(|s| s.short.as_deref().or(s.label.as_deref()))
        .unwrap_or(metric)
}

/// Strip a leading `{root}/` token from a relativized id, e.g.
/// `{target}/src/a.rs` → `src/a.rs`. A file node's id IS its path.
pub fn clean_path(id: &str) -> String {
    if let Some(rest) = id.strip_prefix('{')
        && let Some(idx) = rest.find("}/")
    {
        return rest[idx + 2..].to_string();
    }
    id.to_string()
}

/// Rank the file nodes for one metric, worst-first, and count tier breaches.
/// `"cycle"` is special-cased (cycle members ranked by HK).
pub fn reco_for<'a>(level: &'a LevelGraph, metric: &str) -> Reco<'a> {
    if metric == "cycle" {
        let mut sorted: Vec<&Node> = level
            .nodes
            .iter()
            .filter(|n| is_internal(n) && in_cycle(n))
            .collect();
        sorted.sort_by(|a, b| {
            num(b, "hk")
                .unwrap_or(0.0)
                .total_cmp(&num(a, "hk").unwrap_or(0.0))
        });
        let n = sorted.len();
        return Reco {
            sorted,
            warning_count: n,
            info_count: n,
        };
    }

    let th = thresholds_for(level, metric);
    let mut sorted: Vec<&Node> = level.nodes.iter().filter(|n| is_internal(n)).collect();
    // Worst-first by the metric, tie-broken by sloc then items (as in the viewer)
    // so equal scores still order deterministically.
    sorted.sort_by(|a, b| {
        let key = |n: &Node| {
            (
                num(n, metric).unwrap_or(0.0),
                num(n, "sloc").unwrap_or(0.0),
                num(n, "items").unwrap_or(0.0),
            )
        };
        let (am, as_, ai) = key(a);
        let (bm, bs, bi) = key(b);
        bm.total_cmp(&am)
            .then(bs.total_cmp(&as_))
            .then(bi.total_cmp(&ai))
    });
    let warning_count = sorted
        .iter()
        .filter(|n| num(n, metric).unwrap_or(0.0) > th.warning)
        .count();
    let info_count = sorted
        .iter()
        .filter(|n| num(n, metric).unwrap_or(0.0) > th.info)
        .count();
    Reco {
        sorted,
        warning_count,
        info_count,
    }
}

/// Cycle groups ranked worst-first for the ADP (cycle) preset: `chain` cycles
/// before `mutual`, larger SCCs before smaller, so `--top 1` surfaces the single
/// biggest chain. Ties broken by the first node id for determinism.
fn ranked_cycle_groups(level: &LevelGraph) -> Vec<&CycleGroup> {
    let mut groups: Vec<&CycleGroup> = level.cycles.iter().collect();
    groups.sort_by(|a, b| {
        let chain = |g: &CycleGroup| u8::from(g.kind == "chain");
        chain(b)
            .cmp(&chain(a))
            .then(b.nodes.len().cmp(&a.nodes.len()))
            .then(a.nodes.first().cmp(&b.nodes.first()))
    });
    groups
}

/// The top-N cycle groups (see [`ranked_cycle_groups`]), each paired with its
/// member nodes ordered by HK (worst first). A node id with no matching node is
/// skipped. This is the unit the ADP preset recommends on: `--top` counts
/// **cycles**, and every member of each selected cycle is listed.
pub(super) fn top_cycle_groups(
    level: &LevelGraph,
    n_groups: usize,
) -> Vec<(&CycleGroup, Vec<&Node>)> {
    let by_id: HashMap<&str, &Node> = level.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    ranked_cycle_groups(level)
        .into_iter()
        .take(n_groups)
        .map(|g| {
            let mut members: Vec<&Node> = g
                .nodes
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            members.sort_by(|a, b| {
                num(b, "hk")
                    .unwrap_or(0.0)
                    .total_cmp(&num(a, "hk").unwrap_or(0.0))
            });
            (g, members)
        })
        .collect()
}

/// How many modules a tier selects for a metric's reco.
pub(super) fn tier_count(reco: &Reco, sev: Severity) -> usize {
    match sev {
        Severity::Warning => reco.warning_count,
        Severity::Info => reco.info_count,
        Severity::Auto => {
            if reco.warning_count > 0 {
                reco.warning_count
            } else {
                reco.info_count
            }
        }
    }
}

/// The principle with the most violations: highest `warning` count, tie-broken by
/// `info` count, then by catalog order (the first preset wins on a tie). `None`
/// only if there are no presets.
pub fn worst_preset(level: &LevelGraph, presets: &[Preset]) -> Option<String> {
    let mut best: Option<(&Preset, usize, usize)> = None;
    for p in presets {
        let r = reco_for(level, &p.sort_metric);
        // Strictly-greater so the FIRST preset wins on a tie (catalog order).
        let better = match best {
            None => true,
            Some((_, bw, bi)) => (r.warning_count, r.info_count) > (bw, bi),
        };
        if better {
            best = Some((p, r.warning_count, r.info_count));
        }
    }
    best.map(|(p, _, _)| p.id.clone())
        .or_else(|| presets.first().map(|p| p.id.clone()))
}

/// Count of project source files in the level.
pub(super) fn file_count(level: &LevelGraph) -> usize {
    level.nodes.iter().filter(|n| is_internal(n)).count()
}

/// Format a metric value: abbreviate large numbers to K/M/G when the attribute
/// is flagged `abbreviate`, else a plain rounded integer.
pub(super) fn fmt_val(level: &LevelGraph, metric: &str, v: f64) -> String {
    let abbreviate = level
        .node_attributes
        .get(metric)
        .and_then(|s| s.abbreviate)
        .unwrap_or(false);
    if abbreviate && v.abs() >= 1000.0 {
        for (suf, div) in [("G", 1e9), ("M", 1e6), ("K", 1e3)] {
            if v.abs() >= div {
                let n = v / div;
                let s = format!("{n:.1}");
                let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
                return format!("{s}{suf}");
            }
        }
    }
    format!("{}", v.round() as i64)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "recommend_test.rs"]
mod tests;
