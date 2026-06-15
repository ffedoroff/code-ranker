//! `use`-resolution cluster, extracted from `module_graph.rs` to keep per-file
//! complexity under the project's thresholds. Pure code movement: resolves
//! pending `use` / bare paths against the owning crate's module index, the
//! workspace library indexes (cross-crate), and the extern-crate map, then
//! emits the resulting edges.

use super::shared::{
    ForeignLib, MAX_REEXPORT_DEPTH, PendingUse, ReexportMap, build_reexports, is_reexport,
};
use crate::internal::{Edge, EdgeKind, GraphBuilder, NodeId};
use std::collections::{HashMap, HashSet};
use syn::UseTree;

/// Flatten a `use` tree to `(path, is_glob)` leaves; `is_glob` marks the `::*`
/// terminator so resolution can tell a namespace pull apart from a named import.
pub(super) fn collect_use_paths(
    tree: &UseTree,
    prefix: Vec<String>,
    out: &mut Vec<(Vec<String>, bool)>,
) {
    match tree {
        UseTree::Path(p) => {
            let mut new_prefix = prefix;
            new_prefix.push(p.ident.to_string());
            collect_use_paths(&p.tree, new_prefix, out);
        }
        UseTree::Name(n) => {
            let mut path = prefix;
            path.push(n.ident.to_string());
            out.push((path, false));
        }
        UseTree::Rename(r) => {
            let mut path = prefix;
            path.push(r.ident.to_string());
            out.push((path, false));
        }
        UseTree::Glob(_) => {
            if !prefix.is_empty() {
                out.push((prefix, true));
            }
        }
        UseTree::Group(g) => {
            for sub in &g.items {
                collect_use_paths(sub, prefix.clone(), out);
            }
        }
    }
}

/// Lexical module a glob `use` pulls from, resolved against the current module
/// path (`crate::a::b` → `[a,b]`, `super::*` → parent, `self::x` → child). Returns
/// `None` for a path that doesn't denote an in-crate module.
fn glob_target_module(use_path: &[String], current_path: &[String]) -> Option<Vec<String>> {
    match use_path.first().map(String::as_str) {
        Some("crate") => Some(use_path[1..].to_vec()),
        Some("self") => {
            let mut p = current_path.to_vec();
            p.extend_from_slice(&use_path[1..]);
            Some(p)
        }
        Some("super") => {
            let mut p = current_path.to_vec();
            let mut tail = use_path;
            while tail.first().map(String::as_str) == Some("super") {
                p.pop()?;
                tail = &tail[1..];
            }
            p.extend_from_slice(tail);
            Some(p)
        }
        Some(_) => {
            // Bare first segment in a `use`: crate-relative child module (2018) —
            // a descendant, never an ancestor.
            let mut p = current_path.to_vec();
            p.extend_from_slice(use_path);
            Some(p)
        }
        None => None,
    }
}

/// True when a glob `use` pulls in a *strict ancestor* module's namespace
/// (`use super::*`, `use crate::<ancestor>::*`). This is structural scope-sugar
/// (the child reaching back into its enclosing module), not a real outward
/// dependency, so it is emitted as `EdgeKind::Super` rather than `Uses`.
pub(super) fn is_super_glob(pu: &PendingUse) -> bool {
    if !pu.glob {
        return false;
    }
    let Some(target) = glob_target_module(&pu.use_path, &pu.current_path) else {
        return false;
    };
    target.len() < pu.current_path.len() && pu.current_path[..target.len()] == target[..]
}

pub(super) fn emit_uses(
    pending: &[PendingUse],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    dep_pkg_by_name: &HashMap<String, String>,
    lib_index: &HashMap<String, ForeignLib>,
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
        } else if is_super_glob(pu) {
            EdgeKind::Super
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
            line: pu.line,
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_use_path(
    use_path: &[String],
    current_path: &[String],
    module_index: &HashMap<Vec<String>, NodeId>,
    extern_crates: &HashMap<String, NodeId>,
    dep_pkg_by_name: &HashMap<String, String>,
    lib_index: &HashMap<String, ForeignLib>,
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
            // the path through that crate's library, following its `pub use`
            // re-exports so the edge lands on the file that owns the item
            // (a re-exported `other_crate::Symbol` → its defining file, not the
            // crate root; a path stopping at a non-module, non-re-exported item
            // still falls back to the crate root).
            if let Some(dep_repr) = dep_pkg_by_name.get(other)
                && let Some(foreign) = lib_index.get(dep_repr)
            {
                return walk_foreign(&[], rest, &foreign.index, &foreign.reexports, 0);
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

/// Resolve `base ++ tail` within a **foreign** crate's library, following its
/// `pub use` re-exports so a re-exported `other_crate::Symbol` lands on the file
/// that defines `Symbol` rather than the foreign crate root. Self-contained: it
/// consults only the foreign crate's index and re-export table (a foreign
/// re-export of a *third* crate is left at the foreign module — a rare,
/// acceptable degradation).
fn walk_foreign(
    base: &[String],
    tail: &[String],
    index: &HashMap<Vec<String>, NodeId>,
    reexports: &ReexportMap,
    depth: usize,
) -> Option<NodeId> {
    let (node, stop_path, consumed) = walk_detailed(base, tail, index)?;
    if consumed >= tail.len() {
        return Some(node);
    }
    if depth < MAX_REEXPORT_DEPTH
        && let Some(entries) = reexports.get(&stop_path)
    {
        let sym = &tail[consumed];
        for (exported, source) in entries {
            if exported == sym
                && let Some(redirected) =
                    resolve_foreign_source(source, &stop_path, index, reexports, depth + 1)
                && redirected != node
            {
                return Some(redirected);
            }
        }
    }
    Some(node)
}

/// Resolve a `pub use` source path *within* a foreign crate (handles
/// `crate` / `self` / `super` / submodule prefixes). Keyword/external paths
/// yield `None`, so the caller keeps the facade module.
fn resolve_foreign_source(
    use_path: &[String],
    current_path: &[String],
    index: &HashMap<Vec<String>, NodeId>,
    reexports: &ReexportMap,
    depth: usize,
) -> Option<NodeId> {
    if use_path.is_empty() {
        return None;
    }
    let first = use_path[0].as_str();
    let rest = &use_path[1..];
    match first {
        "crate" => walk_foreign(&[], rest, index, reexports, depth),
        "self" => walk_foreign(current_path, rest, index, reexports, depth),
        "super" => {
            let mut path = current_path.to_vec();
            let mut tail = rest;
            while tail.first().map(|s| s.as_str()) == Some("super") {
                path.pop()?;
                tail = &tail[1..];
            }
            path.pop()?;
            walk_foreign(&path, tail, index, reexports, depth)
        }
        "std" | "core" | "alloc" | "proc_macro" | "test" => None,
        _ => {
            let mut probe = current_path.to_vec();
            probe.push(first.to_string());
            if index.contains_key(&probe) {
                walk_foreign(current_path, use_path, index, reexports, depth)
            } else {
                None
            }
        }
    }
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
    lib_index: &HashMap<String, ForeignLib>,
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

#[cfg(test)]
mod tests {
    use super::super::shared::{ForeignLib, PendingUse, ReexportMap};
    use super::*;
    use crate::internal::{NodeId, Visibility};
    use std::collections::HashMap;

    #[test]
    fn super_glob_only_marks_ancestor_namespace_pulls() {
        let pu = |use_path: &[&str], current: &[&str], glob: bool| PendingUse {
            from_mod_id: "x".into(),
            current_path: current.iter().map(|s| s.to_string()).collect(),
            use_path: use_path.iter().map(|s| s.to_string()).collect(),
            visibility: Visibility::Private,
            bare: false,
            glob,
            line: None,
        };
        // `use super::*` and `use crate::<ancestor>::*` from a child -> super.
        assert!(is_super_glob(&pu(&["super"], &["assets", "lazy"], true)));
        assert!(is_super_glob(&pu(
            &["crate", "assets"],
            &["assets", "lazy"],
            true
        )));
        // Globbing a *child* module (descendant) is not a super pull.
        assert!(!is_super_glob(&pu(&["serialized"], &["assets"], true)));
        // A specific (non-glob) import of a parent item is a real dependency.
        assert!(!is_super_glob(&pu(
            &["crate", "syntax_mapping"],
            &["syntax_mapping", "builtin"],
            false
        )));
        // A glob of an unrelated/extern module is not an ancestor pull.
        assert!(!is_super_glob(&pu(
            &["rayon", "prelude"],
            &["assets"],
            true
        )));
    }

    #[test]
    fn resolve_use_path_simple_cases() {
        // Single-shot resolutions over a bare module index + externs, with no
        // deps / foreign libs / re-exports in play. Those richer mechanisms keep
        // their own dedicated tests below (follows_reexport_to_definer,
        // resolves_cross_crate_*), since each needs a distinct fixture and asserts
        // more than one outcome — collapsing them here would hurt clarity.
        let s = |x: &str| x.to_string();
        // (label, use_path, current_module, index_entries, extern_entries, want)
        struct Case {
            label: &'static str,
            path: Vec<String>,
            current: Vec<String>,
            index: Vec<(Vec<String>, &'static str)>,
            externs: Vec<(&'static str, &'static str)>,
            want: Option<&'static str>,
        }
        let cases = vec![
            Case {
                label: "crate::a::b → AB",
                path: vec![s("crate"), s("a"), s("b")],
                current: vec![],
                index: vec![
                    (vec![], "ROOT"),
                    (vec![s("a")], "A"),
                    (vec![s("a"), s("b")], "AB"),
                ],
                externs: vec![],
                want: Some("AB"),
            },
            Case {
                label: "super::super::x → root sibling X",
                path: vec![s("super"), s("super"), s("x")],
                current: vec![s("a"), s("b")],
                index: vec![
                    (vec![], "ROOT"),
                    (vec![s("a")], "A"),
                    (vec![s("a"), s("b")], "AB"),
                    (vec![s("x")], "X"),
                ],
                externs: vec![],
                want: Some("X"),
            },
            Case {
                label: "extern crate serde::Deserialize",
                path: vec![s("serde"), s("Deserialize")],
                current: vec![],
                index: vec![],
                externs: vec![("serde", "crate:serde")],
                want: Some("crate:serde"),
            },
            Case {
                label: "std is suppressed",
                path: vec![s("std"), s("collections")],
                current: vec![],
                index: vec![],
                externs: vec![],
                want: None,
            },
        ];
        let mut fails = Vec::new();
        for c in &cases {
            let idx: HashMap<Vec<String>, NodeId> = c
                .index
                .iter()
                .cloned()
                .map(|(k, v)| (k, v.into()))
                .collect();
            let externs: HashMap<String, NodeId> = c
                .externs
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).into()))
                .collect();
            let got = resolve_use_path(
                &c.path,
                &c.current,
                &idx,
                &externs,
                &HashMap::new(),
                &HashMap::new(),
                &ReexportMap::new(),
                0,
            );
            if got.as_deref() != c.want {
                fails.push(format!(
                    "{}: want {:?}, got {:?}",
                    c.label,
                    c.want,
                    got.as_deref()
                ));
            }
        }
        assert!(
            fails.is_empty(),
            "resolve_use_path cases failed:\n{}",
            fails.join("\n")
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
            vec![(
                "DomainError".into(),
                vec!["error".into(), "DomainError".into()],
            )],
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
    fn resolve_use_path_handles_intra_crate_bare_path() {
        let mut index: HashMap<Vec<String>, NodeId> = HashMap::new();
        index.insert(vec![], "mod:crate".into());
        index.insert(vec!["commands".into()], "mod:commands".into());
        let externs: HashMap<String, NodeId> = HashMap::new();
        let no_deps: HashMap<String, String> = HashMap::new();
        let no_libs: HashMap<String, ForeignLib> = HashMap::new();
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
        let mut lib_index: HashMap<String, ForeignLib> = HashMap::new();
        lib_index.insert(
            "api 1.0".into(),
            ForeignLib {
                index: foreign,
                reexports: ReexportMap::new(),
            },
        );

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
    fn resolves_cross_crate_reexport_to_definer() {
        // Foreign crate `sec`: its root re-exports `AccessScope` (defined in the
        // `access_scope` submodule) via `pub use access_scope::AccessScope`.
        let mut foreign: HashMap<Vec<String>, NodeId> = HashMap::new();
        foreign.insert(vec![], "mod:sec::lib".into());
        foreign.insert(
            vec!["access_scope".into()],
            "mod:sec::lib::access_scope".into(),
        );
        let mut rx = ReexportMap::new();
        rx.insert(
            vec![],
            vec![(
                "AccessScope".into(),
                vec!["access_scope".into(), "AccessScope".into()],
            )],
        );
        let mut lib_index: HashMap<String, ForeignLib> = HashMap::new();
        lib_index.insert(
            "sec 1.0".into(),
            ForeignLib {
                index: foreign,
                reexports: rx,
            },
        );
        let mut dep_pkg_by_name: HashMap<String, String> = HashMap::new();
        dep_pkg_by_name.insert("sec".into(), "sec 1.0".into());
        let mut externs: HashMap<String, NodeId> = HashMap::new();
        externs.insert("sec".into(), "crate:sec".into());

        // `use sec::AccessScope` → the defining file, not the facade crate root.
        assert_eq!(
            resolve_use_path(
                &["sec".into(), "AccessScope".into()],
                &[],
                &HashMap::new(),
                &externs,
                &dep_pkg_by_name,
                &lib_index,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("mod:sec::lib::access_scope")
        );
        // A symbol the foreign crate does NOT re-export stays at the crate root.
        assert_eq!(
            resolve_use_path(
                &["sec".into(), "NotReexported".into()],
                &[],
                &HashMap::new(),
                &externs,
                &dep_pkg_by_name,
                &lib_index,
                &ReexportMap::new(),
                0,
            )
            .as_deref(),
            Some("mod:sec::lib")
        );
    }

    fn use_paths(src: &str) -> Vec<Vec<String>> {
        let f = syn::parse_file(src).unwrap();
        let mut out = Vec::new();
        for item in &f.items {
            if let syn::Item::Use(u) = item {
                collect_use_paths(&u.tree, Vec::new(), &mut out);
            }
        }
        out.into_iter().map(|(p, _)| p).collect()
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
}
