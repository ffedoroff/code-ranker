//! Shared C-family helpers (used by the C and C++ plugins).
//!
//! The dependency graph for C and C++ is the **`#include` graph**, which is
//! identical for both languages and is recovered by a grammar-independent text
//! scan (the preprocessor runs before parsing, so includes are plain directives):
//! `#include "x"` is a local include resolved to a project file (→ a `uses`
//! edge); `#include <x>` is a system/library include (→ one `external` node).
//! Each plugin supplies its own merged config (extensions, skip-dirs, test
//! conventions); the walk/resolution LOGIC lives here once.

use crate::config::IgnoreCfg;
use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// File-collection + resolution DATA read from a language's merged config.
pub struct Cfg {
    pub extensions: Vec<String>,
    pub skip_dirs: Vec<String>,
    pub test_dirs: Vec<String>,
    pub test_suffixes: Vec<String>,
    pub ext_prefix: String,
    pub uses_kind: String,
    pub loc_attr: String,
    pub external_attr: String,
    /// The preprocessor include keyword the scan keys on (`#include`) — DATA.
    pub include_directive: String,
}

impl Cfg {
    /// Read the shared C-family config slots from a merged `<lang>.toml`.
    pub fn from_config(cfg: &toml::Table) -> Self {
        let uses_kind = crate::config::edge_kind_id(cfg, "uses")
            .expect("c-family [edge_kinds] is missing `uses`")
            .to_string();
        let loc_attr = crate::config::attr_key(cfg, "loc")
            .expect("c-family [node_attributes] is missing `loc`")
            .to_string();
        let external_attr = crate::config::attr_key(cfg, "external")
            .expect("c-family [node_attributes] is missing `external`")
            .to_string();
        Cfg {
            extensions: crate::config::string_list(cfg, "extensions"),
            skip_dirs: crate::config::string_list(cfg, "skip_dirs"),
            test_dirs: crate::config::string_list(cfg, "test_dirs"),
            test_suffixes: crate::config::string_list(cfg, "test_suffixes"),
            ext_prefix: crate::config::string_table(cfg, "ids")
                .get("external")
                .cloned()
                .expect("c-family [ids].external (inherited from defaults.toml)"),
            include_directive: crate::config::string_table(cfg, "structure")
                .get("include_directive")
                .cloned()
                .expect("c-family [structure].include_directive"),
            uses_kind,
            loc_attr,
            external_attr,
        }
    }
}

/// C-family test conventions: files under a `test`/`tests` dir or whose name
/// carries a test suffix (`_test.c`, `.test.cpp`, …).
pub fn is_test_path(rel_path: &str, cfg: &Cfg) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path
        .split('/')
        .any(|c| cfg.test_dirs.iter().any(|d| d == c))
        || cfg.test_suffixes.iter().any(|s| file.ends_with(s.as_str()))
}

/// True when any source file with one of `cfg.extensions` exists under
/// `workspace` (used by `detect` — C/C++ have no universal manifest file).
pub fn detect(workspace: &Path, cfg: &Cfg, ignore: &IgnoreCfg) -> bool {
    !collect_files(workspace, cfg, false, ignore).is_empty()
}

fn collect_files(
    workspace: &Path,
    cfg: &Cfg,
    ignore_tests: bool,
    ignore: &IgnoreCfg,
) -> Vec<PathBuf> {
    crate::walk::collect(workspace, &cfg.skip_dirs, ignore, |p| {
        p.extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| cfg.extensions.iter().any(|e| e == x))
    })
    .into_iter()
    .filter(|p| !(ignore_tests && is_test_file(p, workspace, cfg)))
    .collect()
}

fn is_test_file(path: &Path, workspace: &Path, cfg: &Cfg) -> bool {
    path.strip_prefix(workspace)
        .ok()
        .map(|rel| is_test_path(&rel.to_string_lossy().replace('\\', "/"), cfg))
        .unwrap_or(false)
}

/// Build the `#include` dependency graph for a C-family workspace.
pub fn analyze(
    workspace: &Path,
    ignore_tests: bool,
    cfg: &Cfg,
    ignore: &IgnoreCfg,
) -> Result<Graph> {
    let files: Vec<PathBuf> = collect_files(workspace, cfg, ignore_tests, ignore);
    // rel-path → abs (for `"a/b.h"` resolution) and basename → abs list (fallback).
    let mut by_rel: HashMap<String, PathBuf> = HashMap::new();
    let mut by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for p in &files {
        if let Ok(rel) = p.strip_prefix(workspace) {
            by_rel.insert(rel.to_string_lossy().replace('\\', "/"), p.clone());
        }
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            by_name.entry(name.to_string()).or_default().push(p.clone());
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut ext_seen: HashSet<String> = HashSet::new();

    for abs in &files {
        let Ok(source) = std::fs::read_to_string(abs) else {
            continue;
        };
        let file_id = abs.to_string_lossy().into_owned();
        let loc = source.lines().count() as i64;
        let mut attrs = BTreeMap::new();
        attrs.insert(cfg.loc_attr.clone(), AttrValue::Int(loc.max(1)));
        nodes.push(Node {
            id: file_id.clone(),
            kind: code_ranker_plugin_api::node::FILE.into(),
            name: abs
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            parent: None,
            attrs,
        });

        for (inc, system, line) in scan_includes(&source, &cfg.include_directive) {
            if system {
                add_external(
                    &inc,
                    line,
                    cfg,
                    &file_id,
                    &mut nodes,
                    &mut edges,
                    &mut ext_seen,
                );
                continue;
            }
            match resolve_local(&inc, abs, workspace, &by_rel, &by_name) {
                Some(target) => {
                    let target_id = target.to_string_lossy().into_owned();
                    if target_id != file_id {
                        edges.push(Edge {
                            source: file_id.clone(),
                            target: target_id,
                            kind: cfg.uses_kind.clone(),
                            line: Some(line),
                            attrs: BTreeMap::new(),
                        });
                    }
                }
                // Unresolved local include → treat as external so the dependency is visible.
                None => add_external(
                    &inc,
                    line,
                    cfg,
                    &file_id,
                    &mut nodes,
                    &mut edges,
                    &mut ext_seen,
                ),
            }
        }
    }

    Ok(Graph { nodes, edges })
}

fn add_external(
    inc: &str,
    line: u32,
    cfg: &Cfg,
    file_id: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    ext_seen: &mut HashSet<String>,
) {
    let ext_id = format!("{}{inc}", cfg.ext_prefix);
    if ext_seen.insert(ext_id.clone()) {
        let mut a = BTreeMap::new();
        a.insert(cfg.external_attr.clone(), AttrValue::Bool(true));
        nodes.push(Node {
            id: ext_id.clone(),
            kind: code_ranker_plugin_api::node::EXTERNAL.into(),
            name: inc.rsplit('/').next().unwrap_or(inc).to_string(),
            parent: None,
            attrs: a,
        });
    }
    edges.push(Edge {
        source: file_id.to_string(),
        target: ext_id,
        kind: cfg.uses_kind.clone(),
        line: Some(line),
        attrs: BTreeMap::new(),
    });
}

/// Resolve a local `#include "x"` to a project file: relative to the including
/// file's directory first, then by full relative path, then by unique basename.
///
// COVERAGE: every resolution path here is exercised by the `cfamily` tests, but
// `llvm-cov` reports the two closing braces of the early-returning `if let` blocks
// (the implicit fall-through after a `return`) as uncovered. That is a region
// artifact, not a real gap — there is no statement on those lines to test.
fn resolve_local(
    inc: &str,
    including: &Path,
    workspace: &Path,
    by_rel: &HashMap<String, PathBuf>,
    by_name: &HashMap<String, Vec<PathBuf>>,
) -> Option<PathBuf> {
    if let Some(dir) = including.parent() {
        let cand = dir.join(inc);
        if let Ok(rel) = cand.strip_prefix(workspace) {
            let key = rel.to_string_lossy().replace('\\', "/");
            // Normalise `a/../b` style by checking the by_rel index for the joined path.
            if let Some(p) = by_rel.get(&key) {
                return Some(p.clone());
            }
        }
        if cand.is_file() {
            return Some(cand);
        }
    }
    if let Some(p) = by_rel.get(inc) {
        return Some(p.clone());
    }
    let base = inc.rsplit('/').next().unwrap_or(inc);
    match by_name.get(base) {
        Some(v) if v.len() == 1 => Some(v[0].clone()),
        _ => None,
    }
}

/// Scan source text for `#include` directives. Returns `(target, is_system, line)`
/// for each — `is_system` is the `<...>` form, else the `"..."` form. The include
/// keyword is DATA (`include_directive`); the `#` and quote/bracket delimiters
/// stay inline as single-char preprocessor syntax.
fn scan_includes(source: &str, include_directive: &str) -> Vec<(String, bool, u32)> {
    let mut out = Vec::new();
    for (i, raw) in source.lines().enumerate() {
        let line = raw.trim_start();
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix(include_directive) else {
            continue;
        };
        let rest = rest.trim_start();
        let (open, close) = match rest.chars().next() {
            Some('"') => ('"', '"'),
            Some('<') => ('<', '>'),
            _ => continue,
        };
        if let Some(end) = rest[1..].find(close) {
            let target = &rest[1..1 + end];
            if !target.is_empty() {
                out.push((target.to_string(), open == '<', i as u32 + 1));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod cfamily_tests;
