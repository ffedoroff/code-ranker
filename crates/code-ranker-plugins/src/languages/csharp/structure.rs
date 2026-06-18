//! The C# dependency-graph (structure) builder.
//!
//! Imperative-only code: walk the workspace for `.cs` files, parse each with
//! `tree-sitter-c-sharp`, and record (a) the namespaces it declares and (b) its
//! `using` directives. A `using N;` then resolves to `uses` edges to every file
//! that declares namespace `N`; a `using` of a namespace declared nowhere in the
//! project (`System.*`, a NuGet package) becomes one `external` node.

use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

struct Kinds {
    extensions: Vec<String>,
    skip_dirs: Vec<String>,
    test_dirs: Vec<String>,
    test_suffixes: Vec<String>,
    ext_prefix: String,
    uses_kind: String,
    loc_attr: String,
    external_attr: String,
    using_directive: String,
    namespace_declaration: String,
    file_scoped_namespace_declaration: String,
    qualified_name: String,
    identifier: String,
    field_name: String,
}

static KINDS: LazyLock<Kinds> = LazyLock::new(|| {
    let cfg = crate::config::load(include_str!("config.toml"));
    let s = crate::config::string_table(&cfg, "structure");
    let get = |k: &str| s.get(k).cloned().expect("[structure] key");
    let f = crate::config::string_table(&cfg, "fields");
    Kinds {
        extensions: crate::config::string_list(&cfg, "extensions"),
        skip_dirs: crate::config::string_list(&cfg, "skip_dirs"),
        test_dirs: crate::config::string_list(&cfg, "test_dirs"),
        test_suffixes: crate::config::string_list(&cfg, "test_suffixes"),
        ext_prefix: crate::config::string_table(&cfg, "ids")
            .get("external")
            .cloned()
            .expect("csharp [ids].external (inherited from defaults.toml)"),
        uses_kind: crate::config::edge_kind_id(&cfg, "uses")
            .expect("csharp [edge_kinds] is missing `uses`")
            .to_string(),
        loc_attr: crate::config::attr_key(&cfg, "loc")
            .expect("csharp [node_attributes] is missing `loc`")
            .to_string(),
        external_attr: crate::config::attr_key(&cfg, "external")
            .expect("csharp [node_attributes] is missing `external`")
            .to_string(),
        using_directive: get("using_directive"),
        namespace_declaration: get("namespace_declaration"),
        file_scoped_namespace_declaration: get("file_scoped_namespace_declaration"),
        qualified_name: get("qualified_name"),
        identifier: get("identifier"),
        field_name: f.get("name").cloned().expect("[fields].name"),
    }
});

/// C# test conventions: files under `test`/`tests`, or `*Tests.cs` / `*Test.cs`.
pub(super) fn is_test_path(rel_path: &str) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path
        .split('/')
        .any(|c| KINDS.test_dirs.iter().any(|d| d == c))
        || KINDS
            .test_suffixes
            .iter()
            .any(|s| file.ends_with(s.as_str()))
}

pub(super) fn detect(workspace: &Path) -> bool {
    collect_files(workspace, false).next().is_some()
}

fn collect_files(workspace: &Path, ignore_tests: bool) -> impl Iterator<Item = PathBuf> + '_ {
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
                && !(ignore_tests && is_test_file(e.path(), workspace))
        })
        .map(|e| e.into_path())
}

fn is_test_file(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .ok()
        .map(|rel| is_test_path(&rel.to_string_lossy().replace('\\', "/")))
        .unwrap_or(false)
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

/// One file's parsed structure: its declared namespaces and `using` targets.
struct FileInfo {
    path: PathBuf,
    loc: i64,
    namespaces: Vec<String>,
    usings: Vec<(String, u32)>,
}

pub(super) fn analyze(workspace: &Path, ignore_tests: bool) -> Result<Graph> {
    let files: Vec<PathBuf> = collect_files(workspace, ignore_tests).collect();
    let infos: Vec<FileInfo> = files.iter().filter_map(|p| parse_file(p)).collect();

    // namespace → files declaring it.
    let mut ns_index: HashMap<String, Vec<String>> = HashMap::new();
    for fi in &infos {
        for ns in &fi.namespaces {
            ns_index
                .entry(ns.clone())
                .or_default()
                .push(fi.path.to_string_lossy().into_owned());
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut ext_seen: HashSet<String> = HashSet::new();

    for fi in &infos {
        let file_id = fi.path.to_string_lossy().into_owned();
        let mut attrs = BTreeMap::new();
        attrs.insert(KINDS.loc_attr.clone(), AttrValue::Int(fi.loc.max(1)));
        nodes.push(Node {
            id: file_id.clone(),
            kind: code_ranker_plugin_api::node::FILE.into(),
            name: fi
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            parent: None,
            attrs,
        });

        for (ns, line) in &fi.usings {
            match ns_index.get(ns) {
                Some(targets) => {
                    for t in targets {
                        if *t != file_id {
                            edges.push(Edge {
                                source: file_id.clone(),
                                target: t.clone(),
                                kind: KINDS.uses_kind.clone(),
                                line: Some(*line),
                                attrs: BTreeMap::new(),
                            });
                        }
                    }
                }
                None => {
                    let ext_id = format!("{}{ns}", KINDS.ext_prefix);
                    if ext_seen.insert(ext_id.clone()) {
                        let mut a = BTreeMap::new();
                        a.insert(KINDS.external_attr.clone(), AttrValue::Bool(true));
                        nodes.push(Node {
                            id: ext_id.clone(),
                            kind: code_ranker_plugin_api::node::EXTERNAL.into(),
                            name: ns.clone(),
                            parent: None,
                            attrs: a,
                        });
                    }
                    edges.push(Edge {
                        source: file_id.clone(),
                        target: ext_id,
                        kind: KINDS.uses_kind.clone(),
                        line: Some(*line),
                        attrs: BTreeMap::new(),
                    });
                }
            }
        }
    }

    Ok(Graph { nodes, edges })
}

fn parse_file(path: &Path) -> Option<FileInfo> {
    let source = std::fs::read(path).ok()?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(&source, None)?;
    let mut namespaces = Vec::new();
    let mut usings = Vec::new();
    walk(tree.root_node(), &source, &mut namespaces, &mut usings);
    Some(FileInfo {
        path: path.to_path_buf(),
        loc: source.iter().filter(|&&b| b == b'\n').count() as i64 + 1,
        namespaces,
        usings,
    })
}

fn walk(
    node: tree_sitter::Node,
    src: &[u8],
    namespaces: &mut Vec<String>,
    usings: &mut Vec<(String, u32)>,
) {
    let kind = node.kind();
    if kind == KINDS.namespace_declaration || kind == KINDS.file_scoped_namespace_declaration {
        if let Some(name) = node
            .child_by_field_name(&KINDS.field_name)
            .and_then(|n| n.utf8_text(src).ok())
        {
            namespaces.push(name.to_string());
        }
    } else if kind == KINDS.using_directive {
        // The target namespace is the `qualified_name`/`identifier` child (the
        // `name` field is only the alias in `using X = Y;`). The last such child
        // is the actual namespace in every form (`using N;`, `using static N;`,
        // `using X = N;`).
        let mut cur = node.walk();
        let last = node
            .children(&mut cur)
            .filter(|c| c.kind() == KINDS.qualified_name || c.kind() == KINDS.identifier)
            .last();
        if let Some(name) = last.and_then(|n| n.utf8_text(src).ok()) {
            usings.push((name.to_string(), node.start_position().row as u32 + 1));
        }
    }
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        walk(child, src, namespaces, usings);
    }
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod structure_tests;
