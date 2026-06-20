//! Rule evaluation: cycle-budget and per-file threshold checks, producing
//! ranked `Violation`s.

use super::ignore::is_external;
use super::model::{MetricThresholds, RulesConfig};
use code_ranker_graph::CheckHit;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::{attrs::AttrValue, node::Node};
use std::collections::{BTreeMap, HashMap};

/// Read a numeric node attribute (`Int` or `Float`) as `f64`.
fn attr_num(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Violation {
    pub rule: String,
    /// Concern-group code (`SIZ` / `CPL` / `CPX` / `CYC`, or a custom check's
    /// free-form label). A `String` because custom `[rules.checks]` carry their
    /// own group, not one of a fixed built-in set.
    pub group: String,
    pub graph: &'static str,
    pub location: String,
    /// 1-based line within `location`'s file to pin the diagnostic at (the edge
    /// where a cycle can be broken). `None` for whole-file violations, where the
    /// file-scope metric has no single line — renderers default to line 1.
    pub line: Option<u32>,
    pub message: String,
    pub weight: f64,
    /// Diagnostic copy carried by the violation itself (custom checks set these;
    /// metric/cycle rules leave them `None` and resolve copy from specs via
    /// [`super::rules::rule_doc`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Violation {
    pub fn summary(&self) -> String {
        if self.location.is_empty() {
            self.message.clone()
        } else {
            format!("{}: {}", self.location, self.message)
        }
    }
}

pub fn check_violations(
    graphs: &BTreeMap<String, LevelGraph>,
    rules: &RulesConfig,
) -> Vec<Violation> {
    let mut vs = Vec::new();
    if let Some(level) = graphs.get("files") {
        check_level_violations("files", level, rules, &mut vs);
    }
    vs
}

fn check_level_violations(
    name: &'static str,
    level: &LevelGraph,
    rules: &RulesConfig,
    vs: &mut Vec<Violation>,
) {
    // Cycles: remaining groups are all of enabled kinds; report only those over
    // their kind's budget. Ranked by SCC size.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for cg in &level.cycles {
        *counts.entry(cg.kind.as_str()).or_insert(0) += 1;
    }
    for cg in &level.cycles {
        let count = counts[cg.kind.as_str()];
        let budget = rules.cycles.budget_for(&cg.kind).unwrap_or(0);
        if count as u32 <= budget {
            continue;
        }
        let mut message = describe_cycle(&cg.kind, &cg.nodes);
        if budget > 0 {
            message = format!("{message}  (over budget: {count} > {budget})");
        }
        let (location, line) = cycle_break_point(level, &cg.nodes);
        let id = cycle_rule_id(&cg.kind);
        push(
            vs,
            name,
            id,
            location,
            line,
            message,
            cg.nodes.len() as f64,
            super::rules::rule_group(id),
        );
    }

    // Compile the custom `[rules.checks]` once (with `[rules.defs]` helpers
    // expanded in). A predicate that fails to compile becomes a locationless
    // violation so the gate fails loudly with the reason, rather than silently
    // skipping a misspelled check.
    let mut checks = Vec::new();
    for (id, def) in &rules.checks {
        match code_ranker_graph::checks::compile(id, def, &rules.defs) {
            Ok(c) => checks.push(c),
            Err(e) => push(
                vs,
                name,
                &format!("check.{id}"),
                String::new(),
                None,
                e.to_string(),
                f64::INFINITY,
                "LNT",
            ),
        }
    }
    // A read-only view of the fully-built level (edges + file set), shared by
    // every check's predicate — this is the "second pass" over the materialized
    // graph. Built once; cheap to share.
    let graph = code_ranker_graph::GraphView::build(level);

    let bucket = &rules.thresholds.file;
    for node in &level.nodes {
        if is_external(node) {
            continue;
        }
        check_node_metrics(vs, name, "file", bucket, node, level);
        for check in &checks {
            if let Some(hit) = check.eval(node, &graph) {
                push_check(vs, name, node.id.clone(), hit);
            }
        }
    }
}

/// Turn a fired custom check into a violation. Boolean checks carry no breach
/// magnitude, so they share a uniform weight (ranked below magnitude-scaled
/// metric breaches by `check`'s worst-first sort).
fn push_check(vs: &mut Vec<Violation>, graph: &'static str, location: String, hit: CheckHit) {
    vs.push(Violation {
        rule: format!("check.{}", hit.id),
        group: hit.group,
        graph,
        location,
        line: None,
        message: hit.message,
        weight: 1.0,
        why: hit.why,
        fix: hit.fix,
    });
}

/// The concern group for a threshold key: a registry metric carries its own; a
/// custom `[metrics.<key>]` is mapped from its spec's group (e.g. `loc` → `SIZ`),
/// so a custom-metric breach groups alongside related built-ins instead of `?`.
fn metric_group(level: &LevelGraph, key: &str) -> &'static str {
    if let Some(m) = super::metrics::threshold_metric(key) {
        return m.group;
    }
    let spec_group = level
        .node_attributes
        .get(key)
        .and_then(|s| s.group.as_deref());
    super::metrics::concern_group(spec_group)
}

fn check_node_metrics(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    scope: &str,
    t: &MetricThresholds,
    node: &Node,
    level: &LevelGraph,
) {
    // Walk the limits the user actually set (data-driven). The metric key doubles
    // as the node-attribute key, so read it and breach when it exceeds the limit.
    // The label comes from the registry vocabulary when known, else the key itself
    // — so a project `[metrics.<key>]` is checked exactly like a built-in.
    for (key, limit) in &t.limits {
        if let Some(value) = attr_num(node, key)
            && value > *limit
        {
            let label = super::metrics::threshold_metric(key)
                .map(|m| m.label)
                .unwrap_or_else(|| key.clone());
            push_threshold(
                vs,
                graph,
                &format!("threshold.{scope}.{key}"),
                node.id.clone(),
                &label,
                value,
                *limit,
                metric_group(level, key),
            );
        }
    }
}

/// Pick a concrete spot to break a cycle: the first edge (in the level's stable
/// edge order) whose endpoints are both cycle members. Returns that edge's
/// source node id as the location and its declaration line, if the plugin
/// recorded one. Falls back to the first member with no line if no internal
/// edge is found (shouldn't happen for a real cycle).
fn cycle_break_point(level: &LevelGraph, nodes: &[String]) -> (String, Option<u32>) {
    let in_cycle = |id: &str| nodes.iter().any(|n| n == id);
    if let Some(e) = level
        .edges
        .iter()
        .find(|e| in_cycle(&e.source) && in_cycle(&e.target))
    {
        return (e.source.clone(), e.line);
    }
    (nodes.first().cloned().unwrap_or_default(), None)
}

fn describe_cycle(kind: &str, nodes: &[String]) -> String {
    let preview: Vec<&str> = nodes.iter().take(4).map(String::as_str).collect();
    let truncated = nodes.len() > preview.len();
    match kind {
        "mutual" => format!("mutual cycle between {}", preview.join(" ↔ ")),
        "chain" => {
            let chain = preview.join(" → ");
            let tail = if truncated {
                format!(" → … ({} nodes total)", nodes.len())
            } else {
                " → (back to start)".to_string()
            };
            format!("chain cycle: {chain}{tail}")
        }
        _ => {
            let extra = if truncated {
                format!(" (+{} more)", nodes.len() - preview.len())
            } else {
                String::new()
            };
            format!("cycle: {}{extra}", preview.join(" ↔ "))
        }
    }
}

fn cycle_rule_id(kind: &str) -> &'static str {
    match kind {
        "mutual" => "cycle.mutual",
        "chain" => "cycle.chain",
        _ => "cycle.unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn push_threshold(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    id: &str,
    location: String,
    metric: &str,
    value: f64,
    limit: f64,
    group: &str,
) {
    let ratio = if limit > 0.0 {
        value / limit
    } else {
        f64::INFINITY
    };
    // Integer-valued metrics (loc, cyclomatic, …) print without decimals; metrics
    // that can be fractional (mi, bugs, volume, …) keep two so the breach reads true.
    let decimals = if value.fract() == 0.0 && limit.fract() == 0.0 {
        0
    } else {
        2
    };
    let message = format!(
        "{metric} {value:.decimals$} exceeds limit {limit:.decimals$} ({ratio:.1}× over budget)"
    );
    push(vs, graph, id, location, None, message, ratio, group);
}

#[allow(clippy::too_many_arguments)]
fn push(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    id: &str,
    location: String,
    line: Option<u32>,
    message: String,
    weight: f64,
    group: &str,
) {
    vs.push(Violation {
        rule: id.to_string(),
        group: group.to_string(),
        graph,
        location,
        line,
        message,
        weight,
        why: None,
        fix: None,
    });
}

#[cfg(test)]
#[path = "violations_test.rs"]
mod tests;
