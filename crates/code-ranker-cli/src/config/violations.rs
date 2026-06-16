//! Rule evaluation: cycle-budget and per-file threshold checks, producing
//! ranked `Violation`s.

use super::ignore::is_external;
use super::model::{MetricThresholds, RulesConfig};
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
    pub group: &'static str,
    pub graph: &'static str,
    pub location: String,
    /// 1-based line within `location`'s file to pin the diagnostic at (the edge
    /// where a cycle can be broken). `None` for whole-file violations, where the
    /// file-scope metric has no single line — renderers default to line 1.
    pub line: Option<u32>,
    pub message: String,
    pub weight: f64,
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
        push(
            vs,
            name,
            cycle_rule_id(&cg.kind),
            location,
            line,
            message,
            cg.nodes.len() as f64,
        );
    }

    let bucket = &rules.thresholds.file;
    for node in &level.nodes {
        if is_external(node) {
            continue;
        }
        check_node_metrics(vs, name, "file", bucket, node);
    }
}

fn check_node_metrics(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    scope: &str,
    t: &MetricThresholds,
    node: &Node,
) {
    // Walk the per-file metric vocabulary (data-driven); for each one the user
    // gave a limit, read its node attribute (the metric key doubles as the
    // attribute key) and breach when it exceeds the limit. A new registry metric
    // is thresholdable automatically.
    for tm in super::metrics::threshold_metrics() {
        let Some(limit) = t.get(&tm.key) else {
            continue;
        };
        if let Some(value) = attr_num(node, &tm.key)
            && value > limit
        {
            push_threshold(
                vs,
                graph,
                &format!("threshold.{scope}.{}", tm.key),
                node.id.clone(),
                &tm.label,
                value,
                limit,
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

fn push_threshold(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    id: &str,
    location: String,
    metric: &str,
    value: f64,
    limit: f64,
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
    push(vs, graph, id, location, None, message, ratio);
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
) {
    let group = super::rules::rule_group(id);
    vs.push(Violation {
        rule: id.to_string(),
        group,
        graph,
        location,
        line,
        message,
        weight,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::CycleRule;
    use code_ranker_graph::level_graph::CycleGroup;

    fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
        let mut n = Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: Default::default(),
        };
        for (k, v) in attrs {
            n.attrs.insert((*k).into(), v.clone());
        }
        n
    }

    fn level_with(nodes: Vec<Node>, cycles: Vec<CycleGroup>) -> BTreeMap<String, LevelGraph> {
        let level = LevelGraph {
            nodes,
            cycles,
            ..Default::default()
        };
        BTreeMap::from([("files".to_string(), level)])
    }

    #[test]
    fn check_reports_enabled_cycle_group() {
        let graphs = level_with(
            vec![],
            vec![CycleGroup {
                kind: "chain".into(),
                nodes: vec!["a".into(), "b".into(), "c".into()],
            }],
        );
        let vs = check_violations(&graphs, &RulesConfig::default());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "cycle.chain");
        assert_eq!(vs[0].group, "CYC");
    }

    #[test]
    fn cycle_budget_allows_up_to_n() {
        let cycles: Vec<CycleGroup> = (0..3)
            .map(|i| CycleGroup {
                kind: "chain".into(),
                nodes: vec![format!("a{i}"), format!("b{i}"), format!("c{i}")],
            })
            .collect();
        let graphs = level_with(vec![], cycles);
        let mut rules = RulesConfig::default();
        rules.cycles.chain = CycleRule::Max(3);
        assert!(check_violations(&graphs, &rules).is_empty());
        rules.cycles.chain = CycleRule::Max(2);
        assert_eq!(check_violations(&graphs, &rules).len(), 3);
    }

    #[test]
    fn cycle_violation_points_at_breaking_edge_line() {
        use code_ranker_plugin_api::edge::Edge;
        let edge = |s: &str, t: &str, line: u32| Edge {
            source: s.into(),
            target: t.into(),
            kind: "uses".into(),
            line: Some(line),
            attrs: Default::default(),
        };
        let level = LevelGraph {
            nodes: vec![
                file_node("{target}/a.rs", &[]),
                file_node("{target}/b.rs", &[]),
            ],
            edges: vec![
                edge("{target}/a.rs", "{target}/b.rs", 12),
                edge("{target}/b.rs", "{target}/a.rs", 7),
            ],
            cycles: vec![CycleGroup {
                kind: "mutual".into(),
                nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
            }],
            ..Default::default()
        };
        let graphs = BTreeMap::from([("files".to_string(), level)]);
        let vs = check_violations(&graphs, &RulesConfig::default());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "cycle.mutual");
        // First edge in the level's order whose endpoints are both in the cycle
        // is a.rs -> b.rs at line 12.
        assert_eq!(vs[0].location, "{target}/a.rs");
        assert_eq!(vs[0].line, Some(12));
    }

    #[test]
    fn check_reports_node_threshold_breach() {
        let graphs = level_with(
            vec![
                file_node("hot.rs", &[("cognitive", AttrValue::Int(50))]),
                file_node("cold.rs", &[("cognitive", AttrValue::Int(5))]),
            ],
            vec![],
        );
        let mut rules = RulesConfig::default();
        rules.thresholds.file.set("cognitive".into(), 25.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "threshold.file.cognitive");
        assert_eq!(vs[0].group, "CPX");
        assert!(vs[0].location.contains("hot.rs"));
    }

    #[test]
    fn sloc_threshold_reads_sloc_attr() {
        // `sloc` is a first-class threshold metric (the headline of the
        // open-vocabulary change), distinct from the raw `loc` line count.
        let graphs = level_with(
            vec![file_node("big.rs", &[("sloc", AttrValue::Int(1200))])],
            vec![],
        );
        let mut rules = RulesConfig::default();
        rules.thresholds.file.set("sloc".into(), 800.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "threshold.file.sloc");
        assert_eq!(vs[0].group, "SIZ");
    }

    #[test]
    fn loc_threshold_reads_loc_attr() {
        let graphs = level_with(
            vec![file_node("big.rs", &[("loc", AttrValue::Int(900))])],
            vec![],
        );
        let mut rules = RulesConfig::default();
        rules.thresholds.file.set("loc".into(), 500.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "threshold.file.loc");
        assert_eq!(vs[0].group, "SIZ");
    }
}
