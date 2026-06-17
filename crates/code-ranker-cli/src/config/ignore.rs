//! Graph filtering before cycles/metrics: ignore globs and dev-only crates.
//! Owns the external-node predicate. Test files are dropped earlier, by the
//! language plugin during its walk (see `PluginInput::ignore_tests`).

use super::model::IgnoreConfig;
use anyhow::{Context, Result};
use code_ranker_plugin_api::{attrs::AttrValue, graph::Graph, log, node::Node};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

pub(crate) fn is_external(node: &Node) -> bool {
    node.kind == "external" || matches!(node.attrs.get("external"), Some(AttrValue::Bool(true)))
}

/// Strip nodes/edges matching ignore globs or dev-only crates from the
/// structural graph (before cycles/metrics). Test files are already gone —
/// dropped by the language plugin during its walk.
pub fn apply_ignore(graph: &mut Graph, ignore: &IgnoreConfig, target: &Path) -> Result<usize> {
    // Test files are dropped earlier — by the language plugin during its walk,
    // which knows the language's test conventions (see `PluginInput::ignore_tests`
    // / `LanguagePlugin::is_test_path`). Here we only apply the language-agnostic
    // glob filter and dev-only-crate pruning of external nodes.
    let gs = if ignore.paths.is_empty() {
        None
    } else {
        Some(build_glob_set(&ignore.paths)?)
    };
    let dev_only = if ignore.dev_only_crates {
        collect_dev_only_crates(target)
    } else {
        HashSet::new()
    };
    if gs.is_none() && dev_only.is_empty() {
        return Ok(0);
    }
    Ok(filter_graph(graph, gs.as_ref(), &dev_only))
}

fn collect_dev_only_crates(target: &Path) -> HashSet<String> {
    let out = log::timed("cargo metadata (dev-only crates)", || {
        std::process::Command::new("cargo")
            .args(["metadata", "--format-version", "1"])
            .current_dir(target)
            .stderr(std::process::Stdio::null())
            .output()
            .expect("cargo metadata failed — is cargo installed?")
    });
    assert!(
        out.status.success(),
        "cargo metadata exited with {}",
        out.status
    );

    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata produced invalid JSON");

    let packages = meta["packages"].as_array().expect("packages array");
    let mut id_to_name: HashMap<&str, &str> = HashMap::new();
    for pkg in packages {
        if let (Some(id), Some(name)) = (pkg["id"].as_str(), pkg["name"].as_str()) {
            id_to_name.insert(id, name);
        }
    }

    let workspace_members: HashSet<&str> = meta["workspace_members"]
        .as_array()
        .expect("workspace_members array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    let nodes = meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve.nodes array");
    let mut adj: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for node in nodes {
        let Some(id) = node["id"].as_str() else {
            continue;
        };
        let Some(deps) = node["deps"].as_array() else {
            continue;
        };
        let edges = deps
            .iter()
            .filter_map(|dep| {
                let dep_id = dep["pkg"].as_str()?;
                let kinds = dep["dep_kinds"].as_array()?;
                let dev_only = kinds.iter().all(|k| k["kind"].as_str() == Some("dev"));
                Some((dep_id, dev_only))
            })
            .collect();
        adj.insert(id, edges);
    }

    let mut regular: HashSet<&str> = workspace_members.iter().copied().collect();
    let mut queue: VecDeque<&str> = regular.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &(dep_id, dev_only) in adj.get(id).map(Vec::as_slice).unwrap_or(&[]) {
            if !dev_only && regular.insert(dep_id) {
                queue.push_back(dep_id);
            }
        }
    }

    adj.keys()
        .filter(|&&id| !regular.contains(id))
        .filter_map(|&id| id_to_name.get(id).map(|n| n.to_string()))
        .collect()
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    Ok(b.build()?)
}

/// Ids are `{root}/sub/path` after relativize; strip the `{…}/` prefix for glob
/// matching. External ids (`ext:name`) are returned as-is.
fn strip_root_prefix(id: &str) -> &str {
    if id.starts_with('{')
        && let Some(idx) = id.find('}')
    {
        return id[idx + 1..].trim_start_matches('/');
    }
    id
}

fn filter_graph(graph: &mut Graph, gs: Option<&GlobSet>, dev_only: &HashSet<String>) -> usize {
    let removed: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| {
            if is_external(n) {
                if !dev_only.is_empty()
                    && let Some(name) = n.id.strip_prefix("ext:")
                {
                    let base = name.split('@').next().unwrap_or(name);
                    return dev_only.contains(base);
                }
                return false;
            }
            if let Some(gs) = gs
                && gs.is_match(strip_root_prefix(&n.id))
            {
                return true;
            }
            false
        })
        .map(|n| n.id.clone())
        .collect();
    if removed.is_empty() {
        return 0;
    }
    let before = graph.nodes.len();
    graph.nodes.retain(|n| !removed.contains(&n.id));
    graph
        .edges
        .retain(|e| !removed.contains(&e.source) && !removed.contains(&e.target));
    before - graph.nodes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
        let mut n = Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: Default::default(),
        };
        for (k, v) in attrs {
            n.attrs.insert((*k).into(), v.clone());
        }
        n
    }

    #[test]
    fn strip_root_prefix_token_and_external() {
        assert_eq!(strip_root_prefix("{target}/src/a.rs"), "src/a.rs");
        assert_eq!(strip_root_prefix("ext:serde"), "ext:serde");
        assert_eq!(strip_root_prefix("plain/path.rs"), "plain/path.rs");
    }

    #[test]
    fn build_glob_set_rejects_invalid_pattern() {
        assert!(build_glob_set(&["generated/**".into()]).is_ok());
        assert!(build_glob_set(&["a[".into()]).is_err());
    }

    #[test]
    fn apply_ignore_strips_glob_matches_and_their_edges() {
        let mut g = Graph {
            nodes: vec![
                file_node("{target}/src/keep.rs", &[]),
                file_node("{target}/generated/gen.rs", &[]),
            ],
            edges: vec![code_ranker_plugin_api::edge::Edge {
                source: "{target}/src/keep.rs".into(),
                target: "{target}/generated/gen.rs".into(),
                kind: "uses".into(),
                line: None,
                attrs: Default::default(),
            }],
        };
        let ignore = IgnoreConfig {
            paths: vec!["generated/**".into()],
            ..Default::default()
        };
        let removed = apply_ignore(&mut g, &ignore, Path::new("/x")).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(g.nodes.len(), 1);
        assert!(g.edges.is_empty(), "edge into a removed node is dropped");
    }

    /// `filter_graph` prunes external nodes whose crate is in the dev-only set
    /// (matched by the `ext:<name>[@version]` base), keeps the rest. Drives the
    /// dev-only branch directly with an in-memory set — no `cargo metadata`.
    #[test]
    fn filter_graph_drops_dev_only_external_crates() {
        let ext = |id: &str| file_node(id, &[("external", AttrValue::Bool(true))]);
        let mut g = Graph {
            nodes: vec![
                file_node("{target}/src/a.rs", &[]),
                ext("ext:devdep@2.0"),
                ext("ext:realdep"),
            ],
            edges: vec![],
        };
        let dev_only: HashSet<String> = ["devdep".to_string()].into_iter().collect();
        let removed = filter_graph(&mut g, None, &dev_only);
        assert_eq!(removed, 1, "only the dev-only external is dropped");
        assert!(
            g.nodes.iter().any(|n| n.id == "ext:realdep"),
            "a non-dev external is kept"
        );
        assert!(
            !g.nodes.iter().any(|n| n.id == "ext:devdep@2.0"),
            "the dev-only external (matched by its base name) is dropped"
        );
    }

    /// With nothing matching, `filter_graph` removes nothing and returns 0 (the
    /// `removed.is_empty()` early return).
    #[test]
    fn filter_graph_removes_nothing_when_no_match() {
        let mut g = Graph {
            nodes: vec![file_node(
                "ext:keep",
                &[("external", AttrValue::Bool(true))],
            )],
            edges: vec![],
        };
        assert_eq!(filter_graph(&mut g, None, &HashSet::new()), 0);
        assert_eq!(g.nodes.len(), 1);
    }
}
