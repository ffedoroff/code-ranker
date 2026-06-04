use super::ids::crate_node_id;
use super::internal::{Edge, EdgeKind, GraphBuilder, Node, NodeId, NodeKind, Visibility};
use anyhow::{Context, Result};
use cargo_metadata::{Metadata, Package, PackageId, Target};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned as _;
use syn::{Item, ItemMod, UseTree, Visibility as SynVis};

pub(crate) fn contribute(metadata: &Metadata, builder: &mut GraphBuilder) -> Result<()> {
    let local: HashSet<&PackageId> = metadata.workspace_members.iter().collect();

    // Phase A — build every crate/module node and per-target module index, and
    // collect all pending `use` / bare-path references. Nothing is resolved yet:
    // cross-crate resolution needs the *other* crates' module indexes, so every
    // node must already exist.
    let mut works: Vec<TargetWork> = Vec::new();
    // Each local crate's library module index, keyed by its package repr, so a
    // `use other_crate::sub::Item` can resolve to the submodule file that owns
    // `Item` instead of collapsing onto the crate root.
    let mut lib_index: HashMap<String, HashMap<Vec<String>, NodeId>> = HashMap::new();

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
                crate_label: Some(crate_label(pkg, target)),
            });
            builder.add_edge(Edge {
                from: crate_id.clone(),
                to: root_mod_id.clone(),
                kind: EdgeKind::Contains,
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
            )
            .with_context(|| format!("processing package {}", pkg.name))?;

            // The importable target (lib / proc-macro) is what `use <crate>::…`
            // from another crate resolves into; a bin target is not addressable
            // by name, so only libs feed the workspace index.
            if is_lib_target(target) {
                lib_index.insert(pkg.id.repr.clone(), module_index.clone());
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

/// Human-readable owning-crate (compilation unit) label for a target. A package
/// can produce several crates — a library plus one or more binaries — so the
/// label is per-target: the library uses the package name, binaries get a
/// `(bin …)` suffix that keeps the package name as a prefix (globally unique
/// among workspace members, where package names are unique).
fn crate_label(pkg: &Package, target: &Target) -> String {
    let pkg_name = pkg.name.to_string();
    if is_lib_target(target) {
        pkg_name
    } else if target.name == pkg_name {
        format!("{pkg_name} (bin)")
    } else {
        format!("{pkg_name} (bin {})", target.name)
    }
}

/// A target addressable by name from another crate (lib / proc-macro), as
/// opposed to a `bin` (which cannot be `use`d by name).
fn is_lib_target(target: &Target) -> bool {
    target.kind.iter().any(|k| {
        matches!(
            k.as_str(),
            "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro"
        )
    })
}

#[derive(Debug)]
struct PendingUse {
    from_mod_id: NodeId,
    current_path: Vec<String>,
    use_path: Vec<String>,
    visibility: Visibility,
    /// `true` for a crate-qualified path captured from an expression/type
    /// (`other_crate::item`) rather than a `use` statement.
    bare: bool,
}

/// Collects every qualified path (≥ 2 segments) in a parsed file.
#[derive(Default)]
struct CratePathCollector {
    paths: std::collections::BTreeSet<Vec<String>>,
}

impl<'ast> syn::visit::Visit<'ast> for CratePathCollector {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.segments.len() >= 2 {
            self.paths
                .insert(path.segments.iter().map(|s| s.ident.to_string()).collect());
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        // `#[derive(...)]` arguments are an opaque token stream that the default
        // traversal never parses into paths, so a crate used *only* via a
        // qualified derive (e.g. `#[derive(serde::Serialize)]` with no `use
        // serde`) would otherwise produce no edge. Parse the derive list as a
        // comma-separated path list and record each qualified path.
        if attr.path().is_ident("derive")
            && let Ok(paths) = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
        {
            for p in &paths {
                if p.segments.len() >= 2 {
                    self.paths
                        .insert(p.segments.iter().map(|s| s.ident.to_string()).collect());
                }
            }
        }
        // Other attributes (`#[tokio::main]`, `#[serde(...)]`, …) keep the
        // default visit, which already routes the attribute's own path through
        // `visit_path`.
        syn::visit::visit_attribute(self, attr);
    }
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
    if let Some(node) = builder
        .nodes_mut()
        .iter_mut()
        .find(|n| n.id == *parent_mod_id)
    {
        node.loc = Some(loc);
        node.item_count = Some(item_count);
        node.path = file_path.display().to_string();
    }

    // Capture bare-path references used in expressions/types without a `use`.
    let mut collector = CratePathCollector::default();
    syn::visit::Visit::visit_file(&mut collector, &parsed);
    for path in collector.paths {
        pending_uses.push(PendingUse {
            from_mod_id: parent_mod_id.clone(),
            current_path: parent_mod_path.to_vec(),
            use_path: path,
            visibility: Visibility::Private,
            bare: true,
        });
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
                        bare: false,
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
    let sub_mod_id =
        module_node_id(&pkg.id.repr, target_kind_label(target), &target.name, &sub_path);

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
        version: None,
        visibility: Some(convert_visibility(&m.vis)),
        loc,
        line,
        item_count: None,
        crate_label: Some(crate_label(pkg, target)),
    });
    builder.add_edge(Edge {
        from: parent_mod_id.clone(),
        to: sub_mod_id.clone(),
        kind: EdgeKind::Contains,
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
    } else if let Some(sub_file) = mod_file_path(m, enclosing_file, &sub_name) {
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

/// Per-module re-export table: module path → list of `(exported_symbol,
/// source_use_path)` captured from `pub use` statements. Lets resolution follow
/// `crate::Item` / `super::Item` to the file that *defines* `Item` instead of
/// anchoring on the (facade) module that merely re-exports it.
type ReexportMap = HashMap<Vec<String>, Vec<(String, Vec<String>)>>;

/// Depth guard for following re-export chains (`pub use a::X` → `pub use b::X` …).
const MAX_REEXPORT_DEPTH: usize = 8;

fn build_reexports(pending: &[PendingUse]) -> ReexportMap {
    let mut map: ReexportMap = HashMap::new();
    for pu in pending {
        if pu.bare || !is_reexport(&pu.visibility) {
            continue;
        }
        if let Some(sym) = pu.use_path.last() {
            map.entry(pu.current_path.clone())
                .or_default()
                .push((sym.clone(), pu.use_path.clone()));
        }
    }
    map
}

fn emit_uses(
    pending: &[PendingUse],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    dep_pkg_by_name: &HashMap<String, String>,
    lib_index: &HashMap<String, HashMap<Vec<String>, NodeId>>,
    builder: &mut GraphBuilder,
) {
    let reexports = build_reexports(pending);
    let mut seen: HashSet<(NodeId, NodeId, String)> = HashSet::new();
    for pu in pending {
        let Some(target_id) = resolve_use_path(
            &pu.use_path,
            &pu.current_path,
            module_index,
            extern_crates,
            dep_pkg_by_name,
            lib_index,
            &reexports,
            0,
        ) else {
            continue;
        };
        if target_id == pu.from_mod_id {
            continue;
        }
        let kind = if !pu.bare && is_reexport(&pu.visibility) {
            EdgeKind::Reexports
        } else {
            EdgeKind::Uses
        };
        let kind_str = format!("{kind:?}");
        if !seen.insert((pu.from_mod_id.clone(), target_id.clone(), kind_str)) {
            continue;
        }
        builder.add_edge(Edge {
            from: pu.from_mod_id.clone(),
            to: target_id,
            kind,
            visibility: if matches!(kind, EdgeKind::Reexports) {
                Some(pu.visibility.clone())
            } else {
                None
            },
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_use_path(
    use_path: &[String],
    current_path: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    dep_pkg_by_name: &HashMap<String, String>,
    lib_index: &HashMap<String, HashMap<Vec<String>, NodeId>>,
    reexports: &ReexportMap,
    depth: usize,
) -> Option<NodeId> {
    if use_path.is_empty() {
        return None;
    }
    let first = use_path[0].as_str();
    let rest = &use_path[1..];

    match first {
        "crate" => resolve_in_index(
            &[],
            rest,
            module_index,
            extern_crates,
            dep_pkg_by_name,
            lib_index,
            reexports,
            depth,
        ),
        "self" => resolve_in_index(
            current_path,
            rest,
            module_index,
            extern_crates,
            dep_pkg_by_name,
            lib_index,
            reexports,
            depth,
        ),
        "super" => {
            let mut path = current_path.to_vec();
            let mut tail = rest;
            while tail.first().map(|s| s.as_str()) == Some("super") {
                path.pop()?;
                tail = &tail[1..];
            }
            path.pop()?;
            resolve_in_index(
                &path,
                tail,
                module_index,
                extern_crates,
                dep_pkg_by_name,
                lib_index,
                reexports,
                depth,
            )
        }
        "std" | "core" | "alloc" | "proc_macro" | "test" => None,
        other => {
            let mut probe = current_path.to_vec();
            probe.push(first.to_string());
            if module_index.contains_key(&probe) {
                return resolve_in_index(
                    current_path,
                    use_path,
                    module_index,
                    extern_crates,
                    dep_pkg_by_name,
                    lib_index,
                    reexports,
                    depth,
                );
            }
            // Cross-crate into another local workspace crate: walk the rest of
            // the path through that crate's library module index, so the edge
            // lands on the submodule file that owns the item (falling back to
            // the crate root when the path stops at a non-module item).
            if let Some(dep_repr) = dep_pkg_by_name.get(other)
                && let Some(foreign) = lib_index.get(dep_repr)
            {
                return walk_module_index(&[], rest, foreign);
            }
            // Registry dependency (or a local crate with no library target):
            // collapse onto the crate root node.
            extern_crates.get(other).cloned()
        }
    }
}

/// Walk `base ++ tail` through the module tree, returning the deepest matching
/// module node, the path that reached it, and how many `tail` segments were
/// consumed (a trailing item like a struct/fn leaves a leftover segment).
fn walk_detailed(
    base: &[String],
    tail: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
) -> Option<(NodeId, Vec<String>, usize)> {
    let mut cur = base.to_vec();
    let mut node = module_index.get(&cur)?.clone();
    let mut consumed = 0usize;
    for seg in tail {
        let mut probe = cur.clone();
        probe.push(seg.clone());
        match module_index.get(&probe) {
            Some(id) => {
                node = id.clone();
                cur = probe;
                consumed += 1;
            }
            None => break,
        }
    }
    Some((node, cur, consumed))
}

fn walk_module_index(
    base: &[String],
    tail: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
) -> Option<NodeId> {
    walk_detailed(base, tail, module_index).map(|(node, _, _)| node)
}

/// Resolve a path within the owning crate's module tree, following `pub use`
/// re-exports for a trailing symbol so the edge lands on the file that *defines*
/// the symbol rather than a facade module that re-exports it.
#[allow(clippy::too_many_arguments)]
fn resolve_in_index(
    base: &[String],
    tail: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    dep_pkg_by_name: &HashMap<String, String>,
    lib_index: &HashMap<String, HashMap<Vec<String>, NodeId>>,
    reexports: &ReexportMap,
    depth: usize,
) -> Option<NodeId> {
    let (node, stop_path, consumed) = walk_detailed(base, tail, module_index)?;
    if consumed >= tail.len() {
        // Fully resolved to a module (e.g. `use crate::a::b` where `b` is a mod).
        return Some(node);
    }
    // A leftover segment is a non-module item (struct/fn/const/…). If the module
    // we stopped at re-exports it via `pub use`, follow that to the definer.
    if depth < MAX_REEXPORT_DEPTH
        && let Some(entries) = reexports.get(&stop_path)
    {
        let sym = &tail[consumed];
        for (exported, source) in entries {
            if exported != sym {
                continue;
            }
            if let Some(redirected) = resolve_use_path(
                source,
                &stop_path,
                module_index,
                extern_crates,
                dep_pkg_by_name,
                lib_index,
                reexports,
                depth + 1,
            ) && redirected != node
            {
                return Some(redirected);
            }
        }
    }
    Some(node)
}

/// Resolve the file backing `mod <name>;`. Honours an explicit
/// `#[path = "rel/or/abs.rs"]` attribute (relative to the directory of the file
/// containing the declaration) before falling back to the default
/// `name.rs` / `name/mod.rs` lookup. Without this, a `#[path]` module — and
/// every edge inside it — would be silently dropped.
fn mod_file_path(m: &ItemMod, enclosing_file: &Path, sub_name: &str) -> Option<PathBuf> {
    if let Some(rel) = mod_path_attr(m) {
        let base = enclosing_file.parent().unwrap_or_else(|| Path::new(""));
        let candidate = base.join(&rel);
        return candidate.exists().then_some(candidate);
    }
    resolve_submodule_path(enclosing_file, sub_name)
}

/// Read the string value of a `#[path = "..."]` attribute on a module, if present.
fn mod_path_attr(m: &ItemMod) -> Option<String> {
    for attr in &m.attrs {
        if attr.path().is_ident("path")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Some(s.value());
        }
    }
    None
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

/// A target's node-id namespace. A package can expose several targets that share
/// a name (e.g. a lib `bat` and a bin `bat`); keying module ids on the name alone
/// collapses their roots into one node, so `crate::X` in the lib mis-resolves to
/// the bin's `main.rs` (a library cannot depend on a binary). Disambiguate by the
/// target kind so each target gets its own module tree.
fn target_ns(pkg_id_repr: &str, target_kind: &str, target_name: &str) -> String {
    format!("mod:{pkg_id_repr}::{target_kind}:{target_name}")
}

fn module_node_id(
    pkg_id_repr: &str,
    target_kind: &str,
    target_name: &str,
    path: &[String],
) -> String {
    let ns = target_ns(pkg_id_repr, target_kind, target_name);
    if path.is_empty() {
        ns
    } else {
        format!("{ns}::{}", path.join("::"))
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
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
        );
        assert_eq!(r.as_deref(), Some("AB"));
    }

    #[test]
    fn resolves_super_super_to_root_sibling() {
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
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
        );
        assert_eq!(r.as_deref(), Some("X"));
    }

    #[test]
    fn same_named_lib_and_bin_get_distinct_ids() {
        // A package with a lib `bat` and a bin `bat` must not share a module-id
        // namespace, or `crate::X` in the lib resolves to the bin's `main.rs`.
        assert_ne!(
            module_node_id("bat 1.0", "lib", "bat", &[]),
            module_node_id("bat 1.0", "bin", "bat", &[]),
        );
        assert_ne!(
            module_node_id("bat 1.0", "lib", "bat", &["theme".into()]),
            module_node_id("bat 1.0", "bin", "bat", &["theme".into()]),
        );
    }

    #[test]
    fn follows_reexport_to_definer() {
        // domain/ has children error, local_client. `domain/mod.rs` re-exports
        // `DomainError` from `error`. A sibling's `use super::DomainError` must
        // resolve to `domain::error` (the definer), not `domain` (the facade).
        let mut idx: HashMap<Vec<String>, NodeId> = HashMap::new();
        idx.insert(vec![], "ROOT".into());
        idx.insert(vec!["domain".into()], "DOMAIN".into());
        idx.insert(vec!["domain".into(), "error".into()], "ERROR".into());
        idx.insert(vec!["domain".into(), "local_client".into()], "LC".into());

        // `pub use error::DomainError;` declared inside the `domain` module.
        let mut rx = ReexportMap::new();
        rx.insert(
            vec!["domain".into()],
            vec![("DomainError".into(), vec!["error".into(), "DomainError".into()])],
        );

        // From `domain::local_client`, `use super::DomainError`.
        let r = resolve_use_path(
            &["super".into(), "DomainError".into()],
            &["domain".into(), "local_client".into()],
            &idx,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &rx,
            0,
        );
        assert_eq!(r.as_deref(), Some("ERROR"));

        // Without the re-export table it falls back to the facade module.
        let r0 = resolve_use_path(
            &["super".into(), "DomainError".into()],
            &["domain".into(), "local_client".into()],
            &idx,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
        );
        assert_eq!(r0.as_deref(), Some("DOMAIN"));
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
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
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
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
        );
        assert_eq!(r, None);
    }

    #[test]
    fn resolve_use_path_handles_intra_crate_bare_path() {
        let mut index: HashMap<Vec<String>, NodeId> = HashMap::new();
        index.insert(vec![], "mod:crate".into());
        index.insert(vec!["commands".into()], "mod:commands".into());
        let externs: HashMap<String, NodeId> = HashMap::new();
        let no_deps: HashMap<String, String> = HashMap::new();
        let no_libs: HashMap<String, HashMap<Vec<String>, NodeId>> = HashMap::new();
        assert_eq!(
            resolve_use_path(
                &["commands".into(), "run".into()],
                &[],
                &index,
                &externs,
                &no_deps,
                &no_libs,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("mod:commands")
        );
        let mut externs2: HashMap<String, NodeId> = HashMap::new();
        externs2.insert("once_cell".into(), "crate:once_cell".into());
        assert_eq!(
            resolve_use_path(
                &["once_cell".into(), "sync".into()],
                &[],
                &index,
                &externs2,
                &no_deps,
                &no_libs,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("crate:once_cell")
        );
    }

    #[test]
    fn resolves_cross_crate_use_to_submodule_file() {
        // The foreign crate's library module index: root + a `node` submodule.
        let mut foreign: HashMap<Vec<String>, NodeId> = HashMap::new();
        foreign.insert(vec![], "mod:api::lib".into());
        foreign.insert(vec!["node".into()], "mod:api::lib::node".into());
        let mut lib_index: HashMap<String, HashMap<Vec<String>, NodeId>> = HashMap::new();
        lib_index.insert("api 1.0".into(), foreign);

        let mut dep_pkg_by_name: HashMap<String, String> = HashMap::new();
        dep_pkg_by_name.insert("api".into(), "api 1.0".into());
        // Fallback crate-root node, used only when the path stops above any submodule.
        let mut externs: HashMap<String, NodeId> = HashMap::new();
        externs.insert("api".into(), "crate:api".into());

        // `use api::node::Node` lands on the `node` submodule (not the crate root).
        assert_eq!(
            resolve_use_path(
                &["api".into(), "node".into(), "Node".into()],
                &[],
                &HashMap::new(),
                &externs,
                &dep_pkg_by_name,
                &lib_index,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("mod:api::lib::node")
        );
        // `use api::TopItem` (no matching submodule) falls back to the crate root.
        assert_eq!(
            resolve_use_path(
                &["api".into(), "TopItem".into()],
                &[],
                &HashMap::new(),
                &externs,
                &dep_pkg_by_name,
                &lib_index,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("mod:api::lib")
        );
    }

    #[test]
    fn collector_captures_qualified_paths() {
        let f = syn::parse_file(
            "fn run() { let _ = once_cell::sync::Lazy::new(|| 1); commands::go(); plain(); }",
        )
        .unwrap();
        let mut c = CratePathCollector::default();
        syn::visit::Visit::visit_file(&mut c, &f);
        assert!(
            c.paths.contains(&vec![
                "once_cell".into(),
                "sync".into(),
                "Lazy".into(),
                "new".into()
            ]),
            "got {:?}",
            c.paths
        );
        assert!(
            c.paths.contains(&vec!["commands".into(), "go".into()]),
            "got {:?}",
            c.paths
        );
        assert!(
            !c.paths.iter().any(|p| p == &vec!["plain".to_string()]),
            "single-segment call ignored"
        );
    }

    #[test]
    fn collector_captures_qualified_derive_paths() {
        // A crate referenced only through a qualified derive (no `use`) must
        // still produce a path — the derive arguments are otherwise opaque tokens.
        let f = syn::parse_file(
            "#[derive(Debug, serde::Serialize, thiserror::Error)] struct S;",
        )
        .unwrap();
        let mut c = CratePathCollector::default();
        syn::visit::Visit::visit_file(&mut c, &f);
        assert!(
            c.paths.contains(&vec!["serde".into(), "Serialize".into()]),
            "got {:?}",
            c.paths
        );
        assert!(
            c.paths.contains(&vec!["thiserror".into(), "Error".into()]),
            "got {:?}",
            c.paths
        );
        // The bare `Debug` derive (single segment, std prelude) is not an edge.
        assert!(
            !c.paths.iter().any(|p| p == &vec!["Debug".to_string()]),
            "single-segment derive ignored"
        );
    }
}
