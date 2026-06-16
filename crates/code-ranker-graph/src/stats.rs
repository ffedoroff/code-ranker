//! Per-graph aggregate stats: the mean of each tracked numeric metric across
//! the project's file nodes. Zero/missing values are excluded from a metric's
//! average (matching the historical behavior); a metric is emitted only when
//! its average is positive.

use crate::attrs::{attr_f64, is_external, num_attr};
use code_ranker_plugin_api::{attrs::AttrValue, graph::Graph};
use std::collections::BTreeMap;

/// Compute the mean of each metric in `stat_keys` over all internal (file)
/// nodes. Zero/missing values are excluded (historical behaviour); a metric is
/// emitted only when its average is positive. The key set is data-driven — the
/// caller passes the tier-2 stat metrics (from the registry) plus coupling keys —
/// so this module names no metric.
pub fn compute_stats(graph: &Graph, stat_keys: &[String]) -> BTreeMap<String, AttrValue> {
    let mut stats = BTreeMap::new();
    for key in stat_keys {
        let vals: Vec<f64> = graph
            .nodes
            .iter()
            .filter(|n| !is_external(n))
            .filter_map(|n| attr_f64(n, key))
            .filter(|v| v.is_finite() && *v > 0.0)
            .collect();
        if vals.is_empty() {
            continue;
        }
        let avg = vals.iter().sum::<f64>() / vals.len() as f64;
        if avg > 0.0 {
            stats.insert(key.clone(), num_attr(avg));
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_plugin_api::node::Node;

    fn file(id: &str, cyclomatic: Option<i64>) -> Node {
        let mut n = Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: Default::default(),
        };
        if let Some(c) = cyclomatic {
            n.attrs.insert("cyclomatic".into(), AttrValue::Int(c));
        }
        n
    }

    #[test]
    fn average_excludes_zero_and_missing() {
        let g = Graph {
            nodes: vec![
                file("a", Some(2)),
                file("b", Some(4)),
                file("z", Some(0)),
                file("n", None),
            ],
            edges: vec![],
        };
        let s = compute_stats(&g, &["cyclomatic".to_string()]);
        assert_eq!(s.get("cyclomatic"), Some(&AttrValue::Int(3)));
    }

    #[test]
    fn empty_graph_has_no_stats() {
        let g = Graph::default();
        assert!(compute_stats(&g, &["cyclomatic".to_string()]).is_empty());
    }
}
