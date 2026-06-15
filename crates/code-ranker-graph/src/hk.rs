//! Henry-Kafura coupling over flow edges. For each internal (non-external) node
//! we count unique flow partners (`fan_in` / `fan_out`), track outgoing edges to
//! external libraries separately (`fan_out_external`), and compute
//! `hk = sloc · (fan_in · fan_out)²`. Results are written into node `attrs` as
//! flat keys; zero values are omitted.

use crate::attrs::{attr_f64, is_external, num_attr};
use code_ranker_plugin_api::{attrs::AttrValue, graph::Graph, node::NodeId};
use std::collections::{HashMap, HashSet};

/// Annotate `fan_in` / `fan_out` / `fan_out_external` / `hk` on every internal
/// node, counting only flow edges. External nodes carry no coupling metrics.
pub fn annotate_hk(graph: &mut Graph, flow_kinds: &HashSet<String>) {
    let external_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| is_external(n))
        .map(|n| n.id.as_str())
        .collect();

    let mut fan_in: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
    let mut fan_out: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
    let mut fan_out_ext: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

    for edge in &graph.edges {
        if !flow_kinds.contains(&edge.kind) {
            continue;
        }
        let to_external = external_ids.contains(edge.target.as_str());
        let from_external = external_ids.contains(edge.source.as_str());
        if to_external {
            fan_out_ext
                .entry(edge.source.clone())
                .or_default()
                .insert(edge.target.clone());
            continue;
        }
        if from_external {
            continue;
        }
        fan_out
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
        fan_in
            .entry(edge.target.clone())
            .or_default()
            .insert(edge.source.clone());
    }

    for node in &mut graph.nodes {
        if is_external(node) {
            continue;
        }
        let fi = fan_in.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let fo = fan_out.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let foe = fan_out_ext.get(&node.id).map(|s| s.len()).unwrap_or(0);
        // HK uses the source line count (`sloc`); fall back to the structural
        // `loc` if the complexity pass produced no `sloc` for this file.
        let loc = attr_f64(node, "sloc")
            .or_else(|| attr_f64(node, "loc"))
            .unwrap_or(0.0);
        let hk = loc * ((fi * fo) as f64).powi(2);

        set_or_clear(node, "fan_in", fi as f64);
        set_or_clear(node, "fan_out", fo as f64);
        set_or_clear(node, "fan_out_external", foe as f64);
        if hk > 0.0 {
            node.attrs.insert("hk".to_string(), num_attr(hk));
        } else {
            node.attrs.remove("hk");
        }
    }
}

fn set_or_clear(node: &mut code_ranker_plugin_api::node::Node, key: &str, v: f64) {
    if v > 0.0 {
        node.attrs.insert(key.to_string(), AttrValue::Int(v as i64));
    } else {
        node.attrs.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_plugin_api::{edge::Edge, node::Node};

    fn file(id: &str, sloc: i64) -> Node {
        let mut n = Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: Default::default(),
        };
        n.attrs.insert("sloc".into(), AttrValue::Int(sloc));
        n
    }
    fn uses(from: &str, to: &str) -> Edge {
        Edge {
            source: from.into(),
            target: to.into(),
            kind: "uses".into(),
            line: None,
            attrs: Default::default(),
        }
    }
    fn flow() -> HashSet<String> {
        HashSet::from(["uses".to_string()])
    }

    #[test]
    fn hk_is_loc_times_fan_squared() {
        let mut g = Graph {
            nodes: vec![file("A", 4), file("B", 10), file("C", 5)],
            edges: vec![uses("A", "B"), uses("B", "C")],
        };
        annotate_hk(&mut g, &flow());
        let b = &g.nodes[1];
        assert_eq!(attr_f64(b, "fan_in"), Some(1.0));
        assert_eq!(attr_f64(b, "fan_out"), Some(1.0));
        assert_eq!(attr_f64(b, "hk"), Some(10.0));
    }

    #[test]
    fn external_target_counts_as_fan_out_external() {
        let mut g = Graph {
            nodes: vec![
                file("a", 5),
                Node {
                    id: "ext:x".into(),
                    kind: "external".into(),
                    name: "x".into(),
                    parent: None,
                    attrs: Default::default(),
                },
            ],
            edges: vec![uses("a", "ext:x")],
        };
        annotate_hk(&mut g, &flow());
        let a = &g.nodes[0];
        assert_eq!(attr_f64(a, "fan_out_external"), Some(1.0));
        assert_eq!(a.attrs.get("hk"), None, "no internal coupling → no hk");
    }
}
