use super::ids::crate_node_id;
use super::internal::{Edge, EdgeKind, GraphBuilder, Node, NodeKind};
use cargo_metadata::Metadata;
use std::collections::HashSet;

pub(crate) fn contribute(metadata: &Metadata, builder: &mut GraphBuilder) {
    let local: HashSet<_> = metadata.workspace_members.iter().collect();

    for pkg in &metadata.packages {
        let is_local = local.contains(&pkg.id);
        builder.add_node(Node {
            id: crate_node_id(&pkg.id.repr),
            kind: NodeKind::Crate,
            name: pkg.name.to_string(),
            path: pkg.manifest_path.to_string(),
            parent: None,
            external: (!is_local).then_some(true),
            version: Some(pkg.version.to_string()),
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            crate_label: None,
        });
    }

    let Some(resolve) = metadata.resolve.as_ref() else {
        return;
    };

    for node in &resolve.nodes {
        if !local.contains(&node.id) {
            continue;
        }
        let from = crate_node_id(&node.id.repr);
        for dep in &node.deps {
            builder.add_edge(Edge {
                from: from.clone(),
                to: crate_node_id(&dep.pkg.repr),
                kind: EdgeKind::Uses,
                visibility: None,
                line: None,
            });
        }
    }
}
