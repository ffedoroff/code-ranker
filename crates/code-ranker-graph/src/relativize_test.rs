use super::*;
use crate::level_graph::CycleGroup;

#[test]
fn relativize_path_under_target_uses_token() {
    let got = relativize_path("/p/src/main.rs", Path::new("/p"), &BTreeMap::new());
    assert_eq!(got, "{target}/src/main.rs");
}

#[test]
fn relativize_path_longest_root_wins() {
    let roots = BTreeMap::from([
        ("home".to_string(), "/home/u".to_string()),
        ("registry".to_string(), "/home/u/.cargo".to_string()),
    ]);
    let got = relativize_path("/home/u/.cargo/x.rs", Path::new("/p"), &roots);
    assert_eq!(got, "{registry}/x.rs");
}

#[test]
fn relativize_level_rewrites_ids_edges_and_cycles() {
    use code_ranker_plugin_api::edge::Edge;
    let mut level = LevelGraph::default();
    level.nodes.push(Node {
        id: "/p/src/a.rs".into(),
        kind: "file".into(),
        name: "a.rs".into(),
        parent: None,
        attrs: Default::default(),
    });
    level.nodes.push(Node {
        id: "ext:serde".into(),
        kind: "external".into(),
        name: "serde".into(),
        parent: None,
        attrs: Default::default(),
    });
    level.edges.push(Edge {
        source: "/p/src/a.rs".into(),
        target: "ext:serde".into(),
        kind: "uses".into(),
        line: None,
        attrs: Default::default(),
    });
    level.cycles.push(CycleGroup {
        kind: "mutual".into(),
        nodes: vec!["/p/src/a.rs".into()],
    });
    relativize_level(&mut level, Path::new("/p"), &BTreeMap::new());
    assert_eq!(level.nodes[0].id, "{target}/src/a.rs");
    assert_eq!(level.nodes[1].id, "ext:serde");
    assert_eq!(level.edges[0].source, "{target}/src/a.rs");
    assert_eq!(level.edges[0].target, "ext:serde");
    assert_eq!(level.cycles[0].nodes[0], "{target}/src/a.rs");
}

#[test]
fn relativize_path_empty_and_unmatched_pass_through() {
    // empty input is returned as-is; a path under neither target nor any root
    // falls through unchanged.
    assert_eq!(relativize_path("", Path::new("/p"), &BTreeMap::new()), "");
    assert_eq!(
        relativize_path("/elsewhere/x.rs", Path::new("/p"), &BTreeMap::new()),
        "/elsewhere/x.rs"
    );
}

#[test]
fn relativize_graph_remaps_parent_and_path_attr() {
    let roots = BTreeMap::from([("reg".to_string(), "/reg".to_string())]);
    let mut child_attrs = BTreeMap::new();
    // a `path` attr under a root → rewritten to `{reg}/…`
    child_attrs.insert("path".to_string(), AttrValue::Str("/reg/dep/lib.rs".into()));
    let mut redundant_attrs = BTreeMap::new();
    // a `path` attr equal to the node's own (relativized) id → dropped
    redundant_attrs.insert("path".to_string(), AttrValue::Str("/p/b.rs".into()));

    let mut graph = Graph {
        nodes: vec![
            Node {
                id: "/p/a.rs".into(),
                kind: "file".into(),
                name: "a.rs".into(),
                parent: Some("/p/mod.rs".into()),
                attrs: child_attrs,
            },
            Node {
                id: "/p/mod.rs".into(),
                kind: "file".into(),
                name: "mod.rs".into(),
                parent: None,
                attrs: Default::default(),
            },
            Node {
                id: "/p/b.rs".into(),
                kind: "file".into(),
                name: "b.rs".into(),
                parent: None,
                attrs: redundant_attrs,
            },
        ],
        edges: vec![],
    };
    relativize_graph(&mut graph, Path::new("/p"), &roots);

    let a = graph.nodes.iter().find(|n| n.name == "a.rs").unwrap();
    assert_eq!(a.id, "{target}/a.rs");
    assert_eq!(
        a.parent.as_deref(),
        Some("{target}/mod.rs"),
        "parent remapped"
    );
    assert_eq!(
        a.attrs.get("path"),
        Some(&AttrValue::Str("{reg}/dep/lib.rs".into())),
        "path attr relativized against the root"
    );

    let b = graph.nodes.iter().find(|n| n.name == "b.rs").unwrap();
    assert!(
        !b.attrs.contains_key("path"),
        "a path attr equal to the node's own id is dropped as redundant"
    );
}
