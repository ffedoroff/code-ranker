use crate::ids::crate_node_id;
use anyhow::{Context, Result};
use cargo_metadata::{Metadata, Package, PackageId, Target};
use code_split_core::{Edge, EdgeKind, GraphBuilder, Node, NodeId, NodeKind, Visibility};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned as _;
use syn::{Item, ItemMod, UseTree, Visibility as SynVis};

pub(crate) fn contribute(metadata: &Metadata, builder: &mut GraphBuilder) -> Result<()> {
    let local: HashSet<&PackageId> = metadata.workspace_members.iter().collect();
    for pkg in &metadata.packages {
        if !local.contains(&pkg.id) {
            continue;
        }
        let extern_crates = build_extern_crate_map(pkg, metadata);
        process_package(pkg, &extern_crates, builder)
            .with_context(|| format!("processing package {}", pkg.name))?;
    }
    aggregate_crate_loc(builder);
    Ok(())
}

/// Sum module LOC into each crate node.
/// File nodes no longer exist in the Rust plugin — LOC lives on Module nodes.
fn aggregate_crate_loc(builder: &mut GraphBuilder) {
    use code_split_core::NodeKind;
    // Collect (crate_id, loc) from root-level module nodes (depth == 1 below crate).
    // Root module ids have the form "mod:{pkg_repr}::{target_name}" (no further "::" segments
    // in the path part), and their parent is "crate:{pkg_repr}".
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
    // Sum all module LOC for nested modules by walking every module's parent chain is
    // expensive; instead sum only root-target modules (direct children of crate nodes).
    // Root modules already carry the full file LOC because walk_file sets it on them.
    // Deeper inline modules have their own LOC (span-based) which must NOT be added again.
    let mut crate_loc: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
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

fn build_extern_crate_map(pkg: &Package, metadata: &Metadata) -> HashMap<String, NodeId> {
    let mut map = HashMap::new();
    let Some(resolve) = &metadata.resolve else {
        return map;
    };
    let Some(node) = resolve.nodes.iter().find(|n| n.id == pkg.id) else {
        return map;
    };
    for dep in &node.deps {
        map.insert(dep.name.clone(), crate_node_id(&dep.pkg.repr));
    }
    map
}

fn process_package(
    pkg: &Package,
    extern_crates: &HashMap<String, NodeId>,
    builder: &mut GraphBuilder,
) -> Result<()> {
    let crate_id = crate_node_id(&pkg.id.repr);
    // Guard against walking the same source file twice when a package has
    // multiple targets (e.g. lib + bin) that both declare the same modules.
    let mut visited_files: HashSet<PathBuf> = HashSet::new();

    for target in &pkg.targets {
        if !is_supported_target(target) {
            continue;
        }
        let root_mod_id = module_node_id(&pkg.id.repr, &target.name, &[]);
        let root_label = format!("{} ({})", target.name, target_kind_label(target));

        builder.add_node(Node {
            id: root_mod_id.clone(),
            kind: NodeKind::Module,
            name: root_label,
            path: target.src_path.to_string(),
            parent: Some(crate_id.clone()),
            external: None,
            visibility: Some(Visibility::Public),
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });
        builder.add_edge(Edge {
            from: crate_id.clone(),
            to: root_mod_id.clone(),
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
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
            &mut module_index,
            &mut pending_uses,
            builder,
            &mut visited_files,
        )?;

        emit_uses(&pending_uses, &module_index, extern_crates, builder);
    }

    Ok(())
}

#[derive(Debug)]
struct PendingUse {
    from_mod_id: NodeId,
    current_path: Vec<String>,
    use_path: Vec<String>,
    visibility: Visibility,
}

fn convert_visibility(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(r) => {
            let s = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            match s.as_str() {
                "crate" => Visibility::Crate,
                "super" => Visibility::Super,
                "self" | "" => Visibility::Private,
                _ => Visibility::Restricted { path: s },
            }
        }
        SynVis::Inherited => Visibility::Private,
    }
}

fn is_reexport(v: &Visibility) -> bool {
    !matches!(v, Visibility::Private)
}

#[allow(clippy::too_many_arguments)]
fn walk_file(
    file_path: &Path,
    parent_mod_id: &NodeId,
    parent_mod_path: &[String],
    pkg: &Package,
    target: &Target,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    if !visited_files.insert(file_path.to_path_buf()) {
        return Ok(());
    }
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let parsed =
        syn::parse_file(&content).with_context(|| format!("parsing {}", file_path.display()))?;

    let loc = content.lines().count() as u32;
    let item_count = count_items(&parsed.items) as u32;
    // Annotate the parent module node with LOC and item_count from this file.
    // In Rust a file IS its module — no separate File node is needed.
    if let Some(node) = builder
        .nodes_mut()
        .iter_mut()
        .find(|n| n.id == *parent_mod_id)
    {
        node.loc = Some(loc);
        node.item_count = Some(item_count);
        node.path = file_path.display().to_string();
    }

    walk_items(
        &parsed.items,
        parent_mod_id,
        parent_mod_path,
        file_path,
        pkg,
        target,
        module_index,
        pending_uses,
        builder,
        visited_files,
    )
}

#[allow(clippy::too_many_arguments)]
fn walk_items(
    items: &[Item],
    current_mod_id: &NodeId,
    current_mod_path: &[String],
    enclosing_file: &Path,
    pkg: &Package,
    target: &Target,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    for item in items {
        match item {
            Item::Mod(m) => {
                process_mod(
                    m,
                    current_mod_id,
                    current_mod_path,
                    enclosing_file,
                    pkg,
                    target,
                    module_index,
                    pending_uses,
                    builder,
                    visited_files,
                )?;
            }
            Item::Use(u) => {
                let mut paths = Vec::new();
                collect_use_paths(&u.tree, Vec::new(), &mut paths);
                let vis = convert_visibility(&u.vis);
                for use_path in paths {
                    pending_uses.push(PendingUse {
                        from_mod_id: current_mod_id.clone(),
                        current_path: current_mod_path.to_vec(),
                        use_path,
                        visibility: vis.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_mod(
    m: &ItemMod,
    parent_mod_id: &NodeId,
    parent_mod_path: &[String],
    enclosing_file: &Path,
    pkg: &Package,
    target: &Target,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    let sub_name = m.ident.to_string();
    let mut sub_path = parent_mod_path.to_vec();
    sub_path.push(sub_name.clone());
    let sub_mod_id = module_node_id(&pkg.id.repr, &target.name, &sub_path);

    let (loc, line) = if m.content.is_some() {
        let span = m.span();
        let start = span.start().line as u32;
        let end = span.end().line as u32;
        (Some(end - start + 1), Some(start))
    } else {
        (None, None)
    };
    builder.add_node(Node {
        id: sub_mod_id.clone(),
        kind: NodeKind::Module,
        name: sub_name.clone(),
        path: enclosing_file.display().to_string(),
        parent: Some(parent_mod_id.clone()),
        external: None,
        visibility: Some(convert_visibility(&m.vis)),
        loc,
        line,
        item_count: None,
        method_count: None,
        complexity: None,
        cycle_kind: None,
    });
    builder.add_edge(Edge {
        from: parent_mod_id.clone(),
        to: sub_mod_id.clone(),
        kind: EdgeKind::Contains,
        unresolved: None,
        external: None,
        visibility: None,
    });
    module_index.insert(sub_path.clone(), sub_mod_id.clone());

    if let Some((_, items)) = &m.content {
        walk_items(
            items,
            &sub_mod_id,
            &sub_path,
            enclosing_file,
            pkg,
            target,
            module_index,
            pending_uses,
            builder,
            visited_files,
        )?;
    } else if let Some(sub_file) = resolve_submodule_path(enclosing_file, &sub_name) {
        walk_file(
            &sub_file,
            &sub_mod_id,
            &sub_path,
            pkg,
            target,
            module_index,
            pending_uses,
            builder,
            visited_files,
        )?;
    }
    Ok(())
}

fn collect_use_paths(tree: &UseTree, prefix: Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        UseTree::Path(p) => {
            let mut new_prefix = prefix;
            new_prefix.push(p.ident.to_string());
            collect_use_paths(&p.tree, new_prefix, out);
        }
        UseTree::Name(n) => {
            let mut path = prefix;
            path.push(n.ident.to_string());
            out.push(path);
        }
        UseTree::Rename(r) => {
            let mut path = prefix;
            path.push(r.ident.to_string());
            out.push(path);
        }
        UseTree::Glob(_) => {
            if !prefix.is_empty() {
                out.push(prefix);
            }
        }
        UseTree::Group(g) => {
            for sub in &g.items {
                collect_use_paths(sub, prefix.clone(), out);
            }
        }
    }
}

fn emit_uses(
    pending: &[PendingUse],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    builder: &mut GraphBuilder,
) {
    // Dedup on (from, to, kind) — same target via `use` and `pub use` should produce both edges.
    let mut seen: HashSet<(NodeId, NodeId, EdgeKind)> = HashSet::new();
    for pu in pending {
        let Some(target_id) =
            resolve_use_path(&pu.use_path, &pu.current_path, module_index, extern_crates)
        else {
            continue;
        };
        if target_id == pu.from_mod_id {
            continue;
        }
        let kind = if is_reexport(&pu.visibility) {
            EdgeKind::Reexports
        } else {
            EdgeKind::Uses
        };
        if !seen.insert((pu.from_mod_id.clone(), target_id.clone(), kind)) {
            continue;
        }
        builder.add_edge(Edge {
            from: pu.from_mod_id.clone(),
            to: target_id,
            kind,
            unresolved: None,
            external: None,
            visibility: if kind == EdgeKind::Reexports {
                Some(pu.visibility.clone())
            } else {
                None
            },
        });
    }
}

fn resolve_use_path(
    use_path: &[String],
    current_path: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
) -> Option<NodeId> {
    if use_path.is_empty() {
        return None;
    }
    let first = use_path[0].as_str();
    let rest = &use_path[1..];

    match first {
        "crate" => walk_module_index(&[], rest, module_index),
        "self" => walk_module_index(current_path, rest, module_index),
        "super" => {
            let mut path = current_path.to_vec();
            let mut tail = rest;
            while tail.first().map(|s| s.as_str()) == Some("super") {
                path.pop()?;
                tail = &tail[1..];
            }
            path.pop()?;
            walk_module_index(&path, tail, module_index)
        }
        "std" | "core" | "alloc" | "proc_macro" | "test" => None,
        other => {
            // Rust 2018+ child-path: `use foo::bar` inside a module that
            // declares `mod foo;` resolves to `self::foo::bar`. We test
            // for this by checking whether `current_path :: first` is a
            // known module in the index. If so, walk the path relative
            // to `current_path`. Otherwise treat the first segment as an
            // external crate name.
            let mut probe = current_path.to_vec();
            probe.push(first.to_string());
            if module_index.contains_key(&probe) {
                return walk_module_index(current_path, use_path, module_index);
            }
            extern_crates.get(other).cloned()
        }
    }
}

fn walk_module_index(
    base: &[String],
    tail: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
) -> Option<NodeId> {
    let mut path = base.to_vec();
    if let Some(id) = module_index.get(&path) {
        let mut best = id.clone();
        for seg in tail {
            path.push(seg.clone());
            match module_index.get(&path) {
                Some(id) => best = id.clone(),
                None => break,
            }
        }
        Some(best)
    } else {
        None
    }
}

fn resolve_submodule_path(parent_file: &Path, mod_name: &str) -> Option<PathBuf> {
    let parent_dir = parent_file.parent()?;
    let parent_stem = parent_file.file_stem()?.to_str()?;

    let search_dir = if matches!(parent_stem, "lib" | "main" | "mod") {
        parent_dir.to_path_buf()
    } else {
        parent_dir.join(parent_stem)
    };

    let candidate_a = search_dir.join(format!("{mod_name}.rs"));
    if candidate_a.exists() {
        return Some(candidate_a);
    }
    let candidate_b = search_dir.join(mod_name).join("mod.rs");
    if candidate_b.exists() {
        return Some(candidate_b);
    }
    None
}

fn is_supported_target(target: &Target) -> bool {
    target.kind.iter().any(|k| {
        matches!(
            k.as_str(),
            "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro" | "bin"
        )
    })
}

fn target_kind_label(target: &Target) -> &str {
    target
        .kind
        .iter()
        .map(String::as_str)
        .find(|k| {
            matches!(
                *k,
                "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro" | "bin"
            )
        })
        .unwrap_or("?")
}

fn module_node_id(pkg_id_repr: &str, target_name: &str, path: &[String]) -> String {
    if path.is_empty() {
        format!("mod:{pkg_id_repr}::{target_name}")
    } else {
        format!("mod:{pkg_id_repr}::{target_name}::{}", path.join("::"))
    }
}

fn count_items(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|i| {
            matches!(
                i,
                Item::Fn(_)
                    | Item::Struct(_)
                    | Item::Enum(_)
                    | Item::Trait(_)
                    | Item::Impl(_)
                    | Item::Type(_)
                    | Item::Const(_)
                    | Item::Static(_)
                    | Item::Mod(_)
                    | Item::Macro(_)
                    | Item::Union(_)
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn use_paths(src: &str) -> Vec<Vec<String>> {
        let f = syn::parse_file(src).unwrap();
        let mut out = Vec::new();
        for item in &f.items {
            if let Item::Use(u) = item {
                collect_use_paths(&u.tree, Vec::new(), &mut out);
            }
        }
        out
    }

    #[test]
    fn flattens_simple_use() {
        let paths = use_paths("use foo::bar::Baz;");
        assert_eq!(paths, vec![vec!["foo", "bar", "Baz"]]);
    }

    #[test]
    fn flattens_group() {
        let paths = use_paths("use foo::{bar, baz::Qux};");
        assert_eq!(paths, vec![vec!["foo", "bar"], vec!["foo", "baz", "Qux"],]);
    }

    #[test]
    fn flattens_glob() {
        let paths = use_paths("use foo::bar::*;");
        assert_eq!(paths, vec![vec!["foo", "bar"]]);
    }

    #[test]
    fn resolves_crate_path() {
        let mut idx: HashMap<Vec<String>, NodeId> = HashMap::new();
        idx.insert(vec![], "ROOT".into());
        idx.insert(vec!["a".into()], "A".into());
        idx.insert(vec!["a".into(), "b".into()], "AB".into());
        let r = resolve_use_path(
            &["crate".into(), "a".into(), "b".into()],
            &[],
            &idx,
            &HashMap::new(),
        );
        assert_eq!(r.as_deref(), Some("AB"));
    }

    #[test]
    fn resolves_super_super_to_root_sibling() {
        // `super::super::x` from a::b climbs twice to the root, then resolves `x`.
        let mut idx: HashMap<Vec<String>, NodeId> = HashMap::new();
        idx.insert(vec![], "ROOT".into());
        idx.insert(vec!["a".into()], "A".into());
        idx.insert(vec!["a".into(), "b".into()], "AB".into());
        idx.insert(vec!["x".into()], "X".into());
        let r = resolve_use_path(
            &["super".into(), "super".into(), "x".into()],
            &["a".into(), "b".into()],
            &idx,
            &HashMap::new(),
        );
        assert_eq!(r.as_deref(), Some("X"));
    }

    #[test]
    fn resolves_extern_crate() {
        let mut externs: HashMap<String, NodeId> = HashMap::new();
        externs.insert("serde".into(), "crate:serde".into());
        let r = resolve_use_path(
            &["serde".into(), "Deserialize".into()],
            &[],
            &HashMap::new(),
            &externs,
        );
        assert_eq!(r.as_deref(), Some("crate:serde"));
    }

    #[test]
    fn ignores_std() {
        let r = resolve_use_path(
            &["std".into(), "collections".into()],
            &[],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(r, None);
    }
}
