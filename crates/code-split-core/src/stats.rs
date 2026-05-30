use crate::graph::{AvgCoupling, Graph, GraphStats, Halstead, Loc, Maintainability, Node};

pub fn annotate_stats(graph: &mut Graph) {
    fn avg<F>(nodes: &[Node], f: F) -> f64
    where
        F: Fn(&Node) -> Option<f64>,
    {
        let vals: Vec<f64> = nodes
            .iter()
            .filter_map(f)
            .filter(|v| v.is_finite() && *v > 0.0)
            .collect();
        if vals.is_empty() {
            return 0.0;
        }
        vals.iter().sum::<f64>() / vals.len() as f64
    }

    let nodes = &graph.nodes;

    let cyclomatic = avg(nodes, |n| n.complexity.as_ref().map(|c| c.cyclomatic));
    let cognitive = avg(nodes, |n| n.complexity.as_ref().map(|c| c.cognitive));

    let fan_in = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.fan_in as f64)
    });
    let fan_out = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.fan_out as f64)
    });
    let hk = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.hk)
    });
    let coupling = (fan_in > 0.0 || fan_out > 0.0 || hk > 0.0).then_some(AvgCoupling {
        fan_in,
        fan_out,
        hk,
    });

    let mi = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.maintainability.as_ref())
            .map(|m| m.mi)
    });
    let mi_sei = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.maintainability.as_ref())
            .map(|m| m.mi_sei)
    });
    let maintainability = (mi > 0.0).then_some(Maintainability { mi, mi_sei });

    let loc_source = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.source)
    });
    let loc_comments = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.comments)
    });
    let loc_blank = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.blank)
    });
    let loc = (loc_source > 0.0).then_some(Loc {
        source: loc_source,
        logical: 0.0,
        comments: loc_comments,
        blank: loc_blank,
    });

    let h_length = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.length)
    });
    let h_vocabulary = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.vocabulary)
    });
    let h_volume = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.volume)
    });
    let h_effort = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.effort)
    });
    let h_time = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.time)
    });
    let h_bugs = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.bugs)
    });
    let halstead = (h_volume > 0.0).then_some(Halstead {
        length: h_length,
        vocabulary: h_vocabulary,
        volume: h_volume,
        effort: h_effort,
        time: h_time,
        bugs: h_bugs,
    });

    if cyclomatic == 0.0
        && cognitive == 0.0
        && coupling.is_none()
        && maintainability.is_none()
        && loc.is_none()
        && halstead.is_none()
    {
        return;
    }

    graph.stats = Some(GraphStats {
        cyclomatic,
        cognitive,
        coupling,
        maintainability,
        loc,
        halstead,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Complexity, Coupling, NodeKind};

    fn node(id: &str, complexity: Option<Complexity>) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Fn,
            name: id.into(),
            path: String::new(),
            parent: None,
            external: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity,
            cycle_kind: None,
        }
    }

    fn graph_of(nodes: Vec<Node>) -> Graph {
        Graph {
            nodes,
            edges: Vec::new(),
            cycles: Vec::new(),
            stats: None,
        }
    }

    #[test]
    fn empty_graph_leaves_stats_none() {
        let mut g = graph_of(vec![]);
        annotate_stats(&mut g);
        assert!(g.stats.is_none());
    }

    #[test]
    fn nodes_without_complexity_leave_stats_none() {
        let mut g = graph_of(vec![node("a", None), node("b", None)]);
        annotate_stats(&mut g);
        assert!(g.stats.is_none(), "no metrics → no stats block");
    }

    #[test]
    fn all_zero_metrics_leave_stats_none() {
        let mut g = graph_of(vec![node(
            "a",
            Some(Complexity {
                cyclomatic: 0.0,
                ..Default::default()
            }),
        )]);
        annotate_stats(&mut g);
        assert!(g.stats.is_none(), "all-zero metrics → early return");
    }

    #[test]
    fn cyclomatic_average_excludes_zero_and_missing() {
        // 2.0 and 4.0 count; the 0.0 node is filtered (avg over > 0 only) and
        // the complexity-less node contributes nothing → (2+4)/2 = 3.0.
        let mut g = graph_of(vec![
            node(
                "a",
                Some(Complexity {
                    cyclomatic: 2.0,
                    ..Default::default()
                }),
            ),
            node(
                "b",
                Some(Complexity {
                    cyclomatic: 4.0,
                    ..Default::default()
                }),
            ),
            node(
                "z",
                Some(Complexity {
                    cyclomatic: 0.0,
                    ..Default::default()
                }),
            ),
            node("n", None),
        ]);
        annotate_stats(&mut g);
        let stats = g.stats.expect("metrics present → stats block");
        assert_eq!(stats.cyclomatic, 3.0, "(2+4)/2, zero excluded");
    }

    #[test]
    fn coupling_is_averaged_and_attached() {
        let mut g = graph_of(vec![
            node(
                "a",
                Some(Complexity {
                    coupling: Some(Coupling {
                        fan_in: 2,
                        fan_out: 4,
                        hk: 10.0,
                    }),
                    ..Default::default()
                }),
            ),
            node(
                "b",
                Some(Complexity {
                    coupling: Some(Coupling {
                        fan_in: 4,
                        fan_out: 8,
                        hk: 30.0,
                    }),
                    ..Default::default()
                }),
            ),
        ]);
        annotate_stats(&mut g);
        let c = g.stats.unwrap().coupling.expect("coupling averaged");
        assert_eq!(c.fan_in, 3.0);
        assert_eq!(c.fan_out, 6.0);
        assert_eq!(c.hk, 20.0);
    }

    #[test]
    fn maintainability_attached_when_mi_positive() {
        let mut g = graph_of(vec![node(
            "a",
            Some(Complexity {
                maintainability: Some(Maintainability {
                    mi: 80.0,
                    mi_sei: 70.0,
                }),
                ..Default::default()
            }),
        )]);
        annotate_stats(&mut g);
        let m = g.stats.unwrap().maintainability.expect("mi > 0 → attached");
        assert_eq!(m.mi, 80.0);
        assert_eq!(m.mi_sei, 70.0);
    }

    #[test]
    fn halstead_attached_when_volume_positive() {
        let mut g = graph_of(vec![node(
            "a",
            Some(Complexity {
                halstead: Some(Halstead {
                    length: 10.0,
                    vocabulary: 6.0,
                    volume: 40.0,
                    effort: 100.0,
                    time: 5.0,
                    bugs: 0.1,
                }),
                ..Default::default()
            }),
        )]);
        annotate_stats(&mut g);
        let h = g.stats.unwrap().halstead.expect("volume > 0 → attached");
        assert_eq!(h.volume, 40.0);
        assert_eq!(h.length, 10.0);
    }

    #[test]
    fn halstead_absent_when_volume_zero_but_other_metrics_present() {
        // cyclomatic keeps the stats block alive; volume 0 closes the halstead gate.
        let mut g = graph_of(vec![node(
            "a",
            Some(Complexity {
                cyclomatic: 3.0,
                halstead: Some(Halstead {
                    length: 5.0,
                    vocabulary: 0.0,
                    volume: 0.0,
                    effort: 0.0,
                    time: 0.0,
                    bugs: 0.0,
                }),
                ..Default::default()
            }),
        )]);
        annotate_stats(&mut g);
        let stats = g.stats.expect("cyclomatic keeps stats present");
        assert_eq!(stats.cyclomatic, 3.0);
        assert!(stats.halstead.is_none(), "volume 0 → halstead gate closed");
    }
}
