//! The Markdown link-graph (structure) builder.
//!
//! Markdown has no code metrics, so there is no parse for complexity. Instead we
//! recover the **documentation link graph**: each `.md` file is a node carrying
//! `loc`, and every Markdown link `[text](dest)` to another local `.md` file
//! becomes a `uses` edge. Links to URLs / anchors / non-`.md` targets are
//! ignored. The orchestrator derives coupling and cycles from these edges.

use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

struct Kinds {
    extensions: Vec<String>,
    skip_dirs: Vec<String>,
    uses_kind: String,
    loc_attr: String,
    headings_attr: String,
    max_depth_attr: String,
    code_lines_attr: String,
    links_attr: String,
    broken_links_attr: String,
}

static KINDS: LazyLock<Kinds> = LazyLock::new(|| {
    let cfg = crate::config::load(include_str!("config.toml"));
    let attr = |k: &'static str| {
        crate::config::attr_key(&cfg, k)
            .unwrap_or_else(|| panic!("markdown [node_attributes] is missing `{k}`"))
            .to_string()
    };
    Kinds {
        extensions: crate::config::string_list(&cfg, "extensions"),
        skip_dirs: crate::config::string_list(&cfg, "skip_dirs"),
        uses_kind: crate::config::edge_kind_id(&cfg, "uses")
            .expect("markdown [edge_kinds] is missing `uses`")
            .to_string(),
        loc_attr: attr("loc"),
        headings_attr: attr("headings"),
        max_depth_attr: attr("max_depth"),
        code_lines_attr: attr("code_lines"),
        links_attr: attr("links"),
        broken_links_attr: attr("broken_links"),
    }
});

/// The measured per-document metrics (besides `loc`).
#[derive(Default)]
struct DocMetrics {
    headings: i64,
    max_depth: i64,
    code_lines: i64,
    links: i64,
    broken_links: i64,
}

/// How a link destination classifies: a local existing `.md` (→ edge), a local
/// existing non-`.md` file, a local target that doesn't exist (broken), or an
/// external URL / `#anchor` / `mailto:` (neither edge nor broken).
enum Link {
    MdEdge(PathBuf),
    LocalOk,
    Broken,
    External,
}

pub(super) fn detect(workspace: &Path) -> bool {
    collect_files(workspace).next().is_some()
}

fn collect_files(workspace: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(move |e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| KINDS.extensions.iter().any(|e| e == x))
                && !is_skip_path(e.path(), workspace)
        })
        .map(|e| e.into_path())
}

fn is_skip_path(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .map(|rel| {
            rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s.starts_with('.') || KINDS.skip_dirs.iter().any(|d| d.as_str() == s)
            })
        })
        .unwrap_or(false)
}

pub(super) fn analyze(workspace: &Path) -> Result<Graph> {
    let files: Vec<PathBuf> = collect_files(workspace).collect();
    let mut by_rel: HashMap<String, PathBuf> = HashMap::new();
    for p in &files {
        if let Ok(rel) = p.strip_prefix(workspace) {
            by_rel.insert(rel.to_string_lossy().replace('\\', "/"), p.clone());
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    for abs in &files {
        let Ok(text) = std::fs::read_to_string(abs) else {
            continue;
        };
        let file_id = abs.to_string_lossy().into_owned();

        let mut m = DocMetrics::default();
        let mut in_fence = false;
        for line in text.lines() {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                continue; // the fence marker line is not counted as code
            }
            if in_fence {
                m.code_lines += 1;
                continue; // headings/links inside code blocks don't count
            }
            // ATX heading: 1–6 leading `#` then a space (or end of line).
            let hashes = t.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&hashes) && t[hashes..].chars().next().is_none_or(|c| c == ' ') {
                m.headings += 1;
                m.max_depth = m.max_depth.max(hashes as i64);
            }
            for dest in scan_link_dests(line) {
                m.links += 1;
                match classify_link(&dest, abs, workspace, &by_rel) {
                    Link::MdEdge(target) => {
                        let target_id = target.to_string_lossy().into_owned();
                        if target_id != file_id {
                            edges.push(Edge {
                                source: file_id.clone(),
                                target: target_id,
                                kind: KINDS.uses_kind.clone(),
                                line: None,
                                attrs: BTreeMap::new(),
                            });
                        }
                    }
                    Link::Broken => m.broken_links += 1,
                    Link::LocalOk | Link::External => {}
                }
            }
        }

        let mut attrs = BTreeMap::new();
        let mut put = |key: &str, v: i64| {
            if v != 0 {
                attrs.insert(key.to_string(), AttrValue::Int(v));
            }
        };
        put(&KINDS.loc_attr, text.lines().count().max(1) as i64);
        put(&KINDS.headings_attr, m.headings);
        put(&KINDS.max_depth_attr, m.max_depth);
        put(&KINDS.code_lines_attr, m.code_lines);
        put(&KINDS.links_attr, m.links);
        put(&KINDS.broken_links_attr, m.broken_links);

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
    }

    Ok(Graph { nodes, edges })
}

/// Classify a Markdown link destination: an existing local `.md` (→ a `uses`
/// edge), an existing local non-`.md` file, a local target that doesn't exist
/// (broken), or an external URL / `#anchor` / `mailto:`.
fn classify_link(
    dest: &str,
    from: &Path,
    workspace: &Path,
    by_rel: &HashMap<String, PathBuf>,
) -> Link {
    if dest.is_empty()
        || dest.contains("://")
        || dest.starts_with('#')
        || dest.starts_with("mailto:")
    {
        return Link::External;
    }
    let is_md = KINDS
        .extensions
        .iter()
        .any(|e| dest.ends_with(&format!(".{e}")));
    // Resolve relative to the linking file's directory, then by repo-relative path.
    let cand = from.parent().map(|d| d.join(dest));
    let on_disk = cand.as_ref().is_some_and(|c| c.is_file());
    let by_rel_hit = cand
        .as_ref()
        .and_then(|c| c.strip_prefix(workspace).ok())
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .and_then(|k| by_rel.get(&k).cloned())
        .or_else(|| by_rel.get(dest).cloned());
    if is_md {
        if let Some(p) = by_rel_hit {
            Link::MdEdge(p)
        } else if on_disk {
            // COVERAGE: a linked `.md` that exists on disk yet is absent from
            // `by_rel` means it was never collected (e.g. under a hidden/skipped
            // dir or outside the workspace). Reaching it needs a brittle on-disk
            // layout the collector deliberately excludes, so it stays uncovered.
            Link::MdEdge(cand.unwrap())
        } else {
            Link::Broken
        }
    } else if on_disk {
        Link::LocalOk
    } else {
        Link::Broken
    }
}

/// The inline-link destinations `[text](dest)` on one line, with a `"title"`
/// suffix and a trailing `#anchor` stripped. Image links `![]()` match too.
fn scan_link_dests(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut j = 0;
    while let Some(rel) = line[j..].find("](") {
        let start = j + rel + 2;
        if let Some(end_rel) = line[start..].find(')') {
            let raw = line[start..start + end_rel].trim();
            let dest = raw.split_whitespace().next().unwrap_or(raw);
            let dest = dest.split('#').next().unwrap_or(dest);
            if !dest.is_empty() {
                out.push(dest.to_string());
            }
            j = start + end_rel + 1;
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod structure_tests;
