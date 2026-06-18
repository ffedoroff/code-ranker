//! The Go dependency-graph (structure) builder.
//!
//! Imperative-only code: read the module path from `go.mod`, walk the workspace
//! for `.go` files, group them into packages by directory, parse each file's
//! imports, and resolve them to `uses` edges between file nodes — an import of an
//! internal package becomes edges to every `.go` file in that package's
//! directory; an import outside the module becomes one `external` node per import
//! path. `mod.rs`'s `analyze` calls [`analyze`].

use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

/// File-collection / import-graph DATA, resolved once from `go/config.toml`.
struct StructureKinds {
    extensions: Vec<String>,
    skip_dirs: Vec<String>,
    test_dirs: Vec<String>,
    test_suffixes: Vec<String>,
    ext_prefix: String,
    import_declaration: String,
    interpreted_string_literal: String,
}

static KINDS: LazyLock<StructureKinds> = LazyLock::new(|| {
    let cfg = crate::config::load(include_str!("config.toml"));
    let s = crate::config::string_table(&cfg, "structure");
    let get = |k: &str| s.get(k).cloned().expect("[structure] key");
    StructureKinds {
        extensions: crate::config::string_list(&cfg, "extensions"),
        skip_dirs: crate::config::string_list(&cfg, "skip_dirs"),
        test_dirs: crate::config::string_list(&cfg, "test_dirs"),
        test_suffixes: crate::config::string_list(&cfg, "test_suffixes"),
        ext_prefix: crate::config::string_table(&cfg, "ids")
            .get("external")
            .cloned()
            .expect("go [ids].external (inherited from defaults.toml)"),
        import_declaration: get("import_declaration"),
        interpreted_string_literal: get("interpreted_string_literal"),
    }
});

/// The `uses` edge-kind id, validated against the merged `[edge_kinds]`.
fn uses_edge_kind() -> &'static str {
    static USES: LazyLock<()> = LazyLock::new(|| {
        let cfg = crate::config::load(include_str!("config.toml"));
        crate::config::edge_kind_id(&cfg, "uses")
            .unwrap_or_else(|| panic!("go/config.toml [edge_kinds] is missing `uses`"));
    });
    LazyLock::force(&USES);
    "uses"
}

/// A node-attribute key, validated against `[node_attributes]` (from defaults).
fn attr_key(key: &'static str) -> &'static str {
    static CFG: LazyLock<toml::Table> =
        LazyLock::new(|| crate::config::load(include_str!("config.toml")));
    crate::config::attr_key(&CFG, key)
        .unwrap_or_else(|| panic!("go [node_attributes] is missing `{key}`"));
    key
}

/// Go test conventions: `*_test.go` files and anything under a `testdata/` dir.
pub(super) fn go_is_test_path(rel_path: &str) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path
        .split('/')
        .any(|c| KINDS.test_dirs.iter().any(|d| d == c))
        || KINDS
            .test_suffixes
            .iter()
            .any(|s| file.ends_with(s.as_str()))
}

pub(super) fn analyze(workspace: &Path, ignore_tests: bool) -> Result<Graph> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let module_path = read_module_path(workspace);
    let go_files = collect_go_files(workspace, ignore_tests);
    // package import path → the `.go` files that make it up.
    let pkg_index = build_package_index(workspace, &module_path, &go_files);

    let mut ext_seen: HashSet<String> = HashSet::new();

    for abs_path in &go_files {
        // COVERAGE: the `?` is defensive — `parse_and_add` only errors when
        // tree-sitter `parse()` returns None, which never happens for a readable
        // Go file, so this propagation path is unreachable in practice.
        parse_and_add(
            abs_path,
            &module_path,
            &pkg_index,
            &mut nodes,
            &mut edges,
            &mut ext_seen,
        )?;
    }

    Ok(Graph { nodes, edges })
}

// ── module path (go.mod) ────────────────────────────────────────────────────

/// The module path declared by `go.mod` (`module example.com/foo`), or empty when
/// there is no `go.mod` (then every import resolves as external).
fn read_module_path(workspace: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(workspace.join("go.mod")) else {
        return String::new();
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

// ── file discovery ──────────────────────────────────────────────────────────

fn collect_go_files(workspace: &Path, ignore_tests: bool) -> Vec<PathBuf> {
    WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| KINDS.extensions.iter().any(|e| e == x))
                && !is_skip_path(e.path(), workspace)
                && !(ignore_tests && is_test_file(e.path(), workspace))
        })
        .map(|e| e.into_path())
        .collect()
}

fn is_test_file(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .ok()
        .map(|rel| go_is_test_path(&rel.to_string_lossy().replace('\\', "/")))
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

// ── package index ───────────────────────────────────────────────────────────

/// The import path of the package a file belongs to: `<module>/<dir-rel-to-root>`
/// (or just `<module>` for a file in the module root). `None` when there is no
/// module path.
fn file_to_package_path(workspace: &Path, module_path: &str, path: &Path) -> Option<String> {
    if module_path.is_empty() {
        return None;
    }
    let rel = path.strip_prefix(workspace).ok()?;
    let dir = rel.parent();
    let dir_str = dir
        .map(|d| d.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if dir_str.is_empty() {
        Some(module_path.to_string())
    } else {
        Some(format!("{module_path}/{dir_str}"))
    }
}

fn build_package_index(
    workspace: &Path,
    module_path: &str,
    go_files: &[PathBuf],
) -> HashMap<String, Vec<PathBuf>> {
    let mut idx: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for p in go_files {
        if let Some(pkg) = file_to_package_path(workspace, module_path, p) {
            idx.entry(pkg).or_default().push(p.clone());
        }
    }
    idx
}

// ── per-file parsing ────────────────────────────────────────────────────────

fn parse_and_add(
    abs_path: &Path,
    module_path: &str,
    pkg_index: &HashMap<String, Vec<PathBuf>>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    ext_seen: &mut HashSet<String>,
) -> Result<()> {
    let source = std::fs::read(abs_path)?;

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed: {}", abs_path.display()))?;

    let loc = source.iter().filter(|&&b| b == b'\n').count() as i64 + 1;
    let file_id = abs_path.to_string_lossy().into_owned();

    let mut file_attrs = BTreeMap::new();
    file_attrs.insert(attr_key("loc").to_string(), AttrValue::Int(loc));
    nodes.push(Node {
        id: file_id.clone(),
        kind: code_ranker_plugin_api::node::FILE.into(),
        name: abs_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        parent: None,
        attrs: file_attrs,
    });

    for (import_path, line) in extract_imports(&tree.root_node(), &source) {
        let internal = !module_path.is_empty()
            && (import_path == module_path || import_path.starts_with(&format!("{module_path}/")));
        if internal {
            if let Some(targets) = pkg_index.get(&import_path) {
                for t in targets {
                    let target_id = t.to_string_lossy().into_owned();
                    if target_id != file_id {
                        edges.push(Edge {
                            source: file_id.clone(),
                            target: target_id,
                            kind: uses_edge_kind().into(),
                            line: Some(line),
                            attrs: BTreeMap::new(),
                        });
                    }
                }
            }
        } else {
            let ext_id = format!("{}{import_path}", KINDS.ext_prefix);
            if ext_seen.insert(ext_id.clone()) {
                let mut ext_attrs = BTreeMap::new();
                ext_attrs.insert(attr_key("external").to_string(), AttrValue::Bool(true));
                nodes.push(Node {
                    id: ext_id.clone(),
                    kind: code_ranker_plugin_api::node::EXTERNAL.into(),
                    name: import_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&import_path)
                        .to_string(),
                    parent: None,
                    attrs: ext_attrs,
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: ext_id,
                kind: uses_edge_kind().into(),
                line: Some(line),
                attrs: BTreeMap::new(),
            });
        }
    }

    Ok(())
}

// ── tree-sitter extraction (imports only) ───────────────────────────────────

/// Every imported package path with the 1-based line of its `import_spec`.
fn extract_imports(root: &tree_sitter::Node, source: &[u8]) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    visit_imports(root, source, &mut out);
    out
}

fn visit_imports<'t>(node: &tree_sitter::Node<'t>, source: &[u8], out: &mut Vec<(String, u32)>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == KINDS.import_declaration {
            collect_import_paths(&child, source, out);
        } else {
            visit_imports(&child, source, out);
        }
    }
}

fn collect_import_paths<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    out: &mut Vec<(String, u32)>,
) {
    // The import path is an `interpreted_string_literal` somewhere under the
    // declaration (a single `import "x"` or an `import ( … )` spec list). Each
    // such literal is one imported package.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == KINDS.interpreted_string_literal {
            if let Ok(t) = child.utf8_text(source) {
                let path = t.trim_matches('"').to_string();
                if !path.is_empty() {
                    out.push((path, child.start_position().row as u32 + 1));
                }
            }
        } else {
            collect_import_paths(&child, source, out);
        }
    }
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod structure_tests;
