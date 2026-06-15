use super::ids::crate_node_id;
use super::internal::{Edge, EdgeKind, GraphBuilder, Node, NodeId, NodeKind, Visibility};
use anyhow::{Context, Result};
use cargo_metadata::{Metadata, Package, PackageId, Target};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

mod resolve;
mod shared;
mod walk;

use resolve::emit_uses;
use shared::{
    ForeignLib, PendingUse, build_reexports, crate_label, is_lib_target, module_node_id,
    target_kind_label,
};
use walk::walk_file;

pub(crate) fn contribute(
    metadata: &Metadata,
    ignore_tests: bool,
    builder: &mut GraphBuilder,
) -> Result<()> {
    let local: HashSet<&PackageId> = metadata.workspace_members.iter().collect();

    // Phase A — build every crate/module node and per-target module index, and
    // collect all pending `use` / bare-path references. Nothing is resolved yet:
    // cross-crate resolution needs the *other* crates' module indexes, so every
    // node must already exist.
    let mut works: Vec<TargetWork> = Vec::new();
    // Each local crate's library (module index + `pub use` re-export table),
    // keyed by its package repr, so a `use other_crate::sub::Item` resolves to
    // the submodule file that owns `Item` — and a `use other_crate::ReExported`
    // follows that crate's `pub use` chain to the defining file — instead of
    // collapsing onto the crate root.
    let mut lib_index: HashMap<String, ForeignLib> = HashMap::new();

    for pkg in &metadata.packages {
        if !local.contains(&pkg.id) {
            continue;
        }
        let (extern_crates, dep_pkg_by_name) = build_dep_maps(pkg, metadata);
        let crate_id = crate_node_id(&pkg.id.repr);
        let mut visited_files: HashSet<PathBuf> = HashSet::new();

        for target in &pkg.targets {
            if !is_supported_target(target) {
                continue;
            }
            let root_mod_id =
                module_node_id(&pkg.id.repr, target_kind_label(target), &target.name, &[]);
            let root_label = format!("{} ({})", target.name, target_kind_label(target));
            builder.add_node(Node {
                id: root_mod_id.clone(),
                kind: NodeKind::Module,
                name: root_label,
                path: target.src_path.to_string(),
                parent: Some(crate_id.clone()),
                external: None,
                version: None,
                visibility: Some(Visibility::Public),
                loc: None,
                line: None,
                item_count: None,
                unsafe_count: None,
                crate_label: Some(crate_label(pkg, target)),
            });
            builder.add_edge(Edge {
                from: crate_id.clone(),
                to: root_mod_id.clone(),
                kind: EdgeKind::Contains,
                visibility: None,
                line: None,
            });

            let mut module_index: HashMap<Vec<String>, NodeId> = HashMap::new();
            module_index.insert(vec![], root_mod_id.clone());
            let mut pending_uses: Vec<PendingUse> = Vec::new();

            let src = target.src_path.clone().into_std_path_buf();
            walk_file(
                &src,
                &root_mod_id,
                &[],
                pkg,
                target,
                ignore_tests,
                &mut module_index,
                &mut pending_uses,
                builder,
                &mut visited_files,
            )
            .with_context(|| format!("processing package {}", pkg.name))?;

            // The importable target (lib / proc-macro) is what `use <crate>::…`
            // from another crate resolves into; a bin target is not addressable
            // by name, so only libs feed the workspace index.
            if is_lib_target(target) {
                lib_index.insert(
                    pkg.id.repr.clone(),
                    ForeignLib {
                        index: module_index.clone(),
                        reexports: build_reexports(&pending_uses),
                    },
                );
            }
            works.push(TargetWork {
                extern_crates: extern_crates.clone(),
                dep_pkg_by_name: dep_pkg_by_name.clone(),
                module_index,
                pending_uses,
            });
        }
    }

    // Phase B — resolve every pending use against (1) the owning crate's module
    // index (intra-crate / crate / self / super), (2) the workspace library
    // indexes (cross-crate, submodule-precise), and (3) the extern-crate map
    // (registry deps → crate root).
    for w in &works {
        emit_uses(
            &w.pending_uses,
            &w.module_index,
            &w.extern_crates,
            &w.dep_pkg_by_name,
            &lib_index,
            builder,
        );
    }

    aggregate_crate_loc(builder);
    Ok(())
}

/// Per-target work carried from Phase A (node building) to Phase B (use
/// resolution), so cross-crate resolution can see every crate's module index.
struct TargetWork {
    extern_crates: HashMap<String, NodeId>,
    dep_pkg_by_name: HashMap<String, String>,
    module_index: HashMap<Vec<String>, NodeId>,
    pending_uses: Vec<PendingUse>,
}

/// Sum module LOC into each crate node.
fn aggregate_crate_loc(builder: &mut GraphBuilder) {
    // Collect (crate_id, loc) from root-level module nodes (direct children of crate nodes).
    let entries: Vec<(String, u32)> = builder
        .nodes_mut()
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .filter_map(|n| {
            let loc = n.loc?;
            let parent = n.parent.as_deref()?;
            parent
                .starts_with("crate:")
                .then(|| (parent.to_string(), loc))
        })
        .collect();
    let mut crate_loc: HashMap<String, u32> = HashMap::new();
    for (crate_id, loc) in entries {
        crate_loc
            .entry(crate_id)
            .and_modify(|v| *v += loc)
            .or_insert(loc);
    }
    for node in builder.nodes_mut().iter_mut() {
        if node.kind == NodeKind::Crate
            && let Some(total) = crate_loc.get(&node.id)
        {
            node.loc = Some(*total);
        }
    }
}

/// Build, from the resolve graph, both dependency maps for `pkg`: the direct
/// dependency's *code* name (the `extern crate` name, hyphens normalized to
/// underscores) → its crate-root node id (registry fallback) and → its package
/// repr (to locate a local crate's library module index). Renamed deps map by
/// the rename, matching how `use <name>::…` refers to them.
fn build_dep_maps(
    pkg: &Package,
    metadata: &Metadata,
) -> (HashMap<String, NodeId>, HashMap<String, String>) {
    let mut extern_map = HashMap::new();
    let mut pkg_map = HashMap::new();
    let Some(resolve) = &metadata.resolve else {
        return (extern_map, pkg_map);
    };
    let Some(node) = resolve.nodes.iter().find(|n| n.id == pkg.id) else {
        return (extern_map, pkg_map);
    };
    for dep in &node.deps {
        extern_map.insert(dep.name.clone(), crate_node_id(&dep.pkg.repr));
        pkg_map.insert(dep.name.clone(), dep.pkg.repr.clone());
    }
    (extern_map, pkg_map)
}

fn is_supported_target(target: &Target) -> bool {
    target.kind.iter().any(|k| {
        matches!(
            k.as_str(),
            "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro" | "bin"
        )
    })
}
