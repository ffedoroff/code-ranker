// Import from the defining modules (not the crate-root re-exports) so this module
// depends "down" on `graph`/`snapshot` rather than "up" on the crate root — which
// would close a `root → hk → root` cycle.
use crate::graph::{Complexity, Coupling, EdgeKind, Graph, Loc, NodeId, NodeKind};
use crate::snapshot::PluginGraphs;
use std::collections::{HashMap, HashSet};

pub fn annotate_hk(graphs: &mut PluginGraphs) {
    annotate_graph_hk(&mut graphs.files);
}

/// Is this node an external dependency (a library node, not a project file)?
fn is_external(node: &crate::graph::Node) -> bool {
    node.kind == NodeKind::External || node.external.unwrap_or(false)
}

fn annotate_graph_hk(graph: &mut Graph) {
    // Edges into external libraries are tracked separately (`fan_out_external`)
    // and excluded from the internal fan-in/out that drives HK — HK measures
    // *internal* architectural coupling, not 3rd-party library usage.
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
        // `Contains` edges (Rust `mod foo;` declarations) are structural
        // ownership, not information flow — excluded from coupling.
        if edge.kind == EdgeKind::Contains {
            continue;
        }
        // External-library edges are split out into `fan_out_external` (below);
        // the rest (`uses`/`reexports`) count toward internal coupling.
        let to_external = external_ids.contains(edge.to.as_str());
        let from_external = external_ids.contains(edge.from.as_str());
        if to_external {
            fan_out_ext
                .entry(edge.from.clone())
                .or_default()
                .insert(edge.to.clone());
            continue;
        }
        if from_external {
            continue; // edges originating from a library are not project coupling
        }
        fan_out
            .entry(edge.from.clone())
            .or_default()
            .insert(edge.to.clone());
        fan_in
            .entry(edge.to.clone())
            .or_default()
            .insert(edge.from.clone());
    }

    for node in &mut graph.nodes {
        if is_external(node) {
            continue; // library nodes carry no coupling/HK metrics
        }
        let fi = fan_in.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let fo = fan_out.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let foe = fan_out_ext.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let struct_loc = node.loc; // structural LOC, present on aggregate (crate) nodes

        let cx = node.complexity.get_or_insert_with(Complexity::default);
        // When rust-code-analysis produced no LOC (e.g. synthetic crate nodes) but a
        // structural line count exists, mirror it into `complexity.loc` so the displayed
        // loc and hk always agree instead of one being blank.
        if cx.loc.is_none()
            && let Some(n) = struct_loc
            && n > 0
        {
            cx.loc = Some(Loc {
                source: n as f64,
                logical: 0.0,
                comments: 0.0,
                blank: 0.0,
            });
        }
        // Henry-Kafura: hk = loc × (fan_in × fan_out)². Uses the same loc that is
        // displayed; with no loc or no in/out coupling, hk is 0.
        let loc = cx.loc.as_ref().map(|l| l.source).unwrap_or(0.0);
        let hk = loc * ((fi * fo) as f64).powi(2);
        cx.coupling = Some(Coupling {
            fan_in: fi as u32,
            fan_out: fo as u32,
            fan_out_external: foe as u32,
            hk,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeKind, Loc, Node};

    fn module(id: &str, complexity_loc: Option<f64>, struct_loc: Option<u32>) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Module,
            name: id.into(),
            path: "p".into(),
            parent: None,
            external: None,
            visibility: None,
            loc: struct_loc,
            line: None,
            item_count: None,
            method_count: None,
            complexity: complexity_loc.map(|s| Complexity {
                loc: Some(Loc {
                    source: s,
                    logical: 0.0,
                    comments: 0.0,
                    blank: 0.0,
                }),
                ..Default::default()
            }),
            cycle_kind: None,
        }
    }

    fn uses(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Uses,
            unresolved: None,
            external: None,
            visibility: None,
        }
    }

    fn coupling<'a>(g: &'a Graph, id: &str) -> &'a Coupling {
        g.nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .complexity
            .as_ref()
            .unwrap()
            .coupling
            .as_ref()
            .unwrap()
    }

    #[test]
    fn hk_is_loc_times_fan_squared() {
        // A -> B -> C.  B has loc 10, fan_in 1, fan_out 1 → hk = 10·(1·1)² = 10.
        let mut g = PluginGraphs::default();
        g.files.nodes = vec![
            module("A", Some(4.0), Some(4)),
            module("B", Some(10.0), Some(10)),
            module("C", Some(5.0), Some(5)),
        ];
        g.files.edges = vec![uses("A", "B"), uses("B", "C")];
        annotate_graph_hk(&mut g.files);

        let b = coupling(&g.files, "B");
        assert_eq!((b.fan_in, b.fan_out), (1, 1));
        assert_eq!(b.hk, 10.0, "hk = loc(10) · (fan_in·fan_out)²");
    }

    #[test]
    fn hk_falls_back_to_structural_loc_for_crate_like_nodes() {
        // Y -> X -> Z.  X is crate-like: only structural node.loc, no complexity.loc.
        // It must keep an hk (fan_in 1, fan_out 1) AND surface that loc in complexity.loc.
        let mut g = PluginGraphs::default();
        g.files.nodes = vec![
            module("X", None, Some(10)),
            module("Y", Some(5.0), Some(5)),
            module("Z", Some(5.0), Some(5)),
        ];
        g.files.edges = vec![uses("Y", "X"), uses("X", "Z")];
        annotate_graph_hk(&mut g.files);

        let x = g.files.nodes.iter().find(|n| n.id == "X").unwrap();
        let xc = x.complexity.as_ref().unwrap();
        assert_eq!(
            xc.loc.as_ref().unwrap().source,
            10.0,
            "structural loc mirrored into complexity.loc so it is displayed"
        );
        let cp = xc.coupling.as_ref().unwrap();
        assert_eq!((cp.fan_in, cp.fan_out), (1, 1));
        assert_eq!(
            cp.hk, 10.0,
            "crate-like node keeps hk from its structural loc"
        );
    }

    #[test]
    fn hk_is_zero_without_any_loc() {
        // M -> N : M has neither complexity.loc nor structural loc → hk 0 despite fan_out.
        let mut g = PluginGraphs::default();
        g.files.nodes = vec![module("M", None, None), module("N", Some(3.0), Some(3))];
        g.files.edges = vec![uses("M", "N")];
        annotate_graph_hk(&mut g.files);
        let m = coupling(&g.files, "M");
        assert_eq!(m.fan_out, 1);
        assert_eq!(m.hk, 0.0, "no loc anywhere → hk 0");
    }
}
