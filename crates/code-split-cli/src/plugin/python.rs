use anyhow::Result;
use code_split_core::{
    GraphBuilder, NodeKind, PluginGraphs, StageTime,
    graph::{Edge, EdgeKind, Node, Visibility},
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::logger;
use crate::plugin::finalize::finalize_file_graph;

pub fn run(workspace: &Path, _local_only: bool) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings = Vec::new();
    let mut builder = GraphBuilder::new();

    let t = logger::Timer::start("python: scan + parse + build file graph");

    let py_files = collect_py_files(workspace);
    let module_index = build_module_index(workspace, &py_files);

    for abs_path in &py_files {
        let Some(mod_path) = file_to_module_path(workspace, abs_path) else {
            continue;
        };
        let _ = parse_and_add(abs_path, &mod_path, &module_index, &mut builder);
    }

    let n = builder.node_count();
    let detail = format!("{n} nodes from {} files", py_files.len());
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "python".into(),
        ms,
        detail,
    });

    {
        let t = logger::Timer::start("complexity: cyclomatic / cognitive / halstead / MI / LOC");
        let annotated = match code_split_complexity::analyze_python(workspace, &mut builder) {
            Ok(n) => n,
            Err(e) => {
                logger::info(&format!("complexity skipped: {e:#}"));
                0
            }
        };
        let detail = format!("{annotated} nodes annotated");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "complexity".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting file graph");
    let files = finalize_file_graph(builder.build());
    let detail = format!("files={} edges={}", files.nodes.len(), files.edges.len());
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "projection".into(),
        ms,
        detail,
    });

    Ok((PluginGraphs { files }, timings))
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

fn collect_py_files(workspace: &Path) -> Vec<PathBuf> {
    WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().is_some_and(|x| x == "py")
                && !is_skip_path(e.path(), workspace)
        })
        .map(|e| e.into_path())
        .collect()
}

fn is_skip_path(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .map(|rel| {
            rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s.starts_with('.') || s == "venv" || s == "__pycache__" || s == "node_modules"
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Module path helpers
// ---------------------------------------------------------------------------

/// `parser/shops/amazon/pdp.py` → `"parser.shops.amazon.pdp"`
/// `parser/shops/amazon/__init__.py` → `"parser.shops.amazon"`
fn file_to_module_path(workspace: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    let last = parts.last_mut()?;
    if *last == "__init__.py" {
        parts.pop();
    } else if let Some(stem) = last.strip_suffix(".py") {
        *last = stem.to_string();
    } else {
        return None;
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("."))
}

fn build_module_index(workspace: &Path, py_files: &[PathBuf]) -> HashMap<String, PathBuf> {
    py_files
        .iter()
        .filter_map(|p| file_to_module_path(workspace, p).map(|m| (m, p.clone())))
        .collect()
}

// ---------------------------------------------------------------------------
// Per-file parsing
// ---------------------------------------------------------------------------

struct ExtractedImport {
    base: String,       // "parser.shops.amazon" or ".." or ".utils"
    names: Vec<String>, // imported names; empty for plain `import X`
}

fn parse_and_add(
    abs_path: &Path,
    mod_path: &str,
    module_index: &HashMap<String, PathBuf>,
    builder: &mut GraphBuilder,
) -> Result<()> {
    let source = std::fs::read(abs_path)?;

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed: {}", abs_path.display()))?;

    let loc = source.iter().filter(|&&b| b == b'\n').count() as u32 + 1;
    let file_id = format!("file:{}", abs_path.to_string_lossy());

    let parts: Vec<&str> = mod_path.split('.').collect();

    builder.add_node(Node {
        id: file_id.clone(),
        kind: NodeKind::File,
        name: abs_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: abs_path.to_string_lossy().into_owned(),
        parent: None,
        external: None,
        visibility: Some(py_visibility(parts[parts.len() - 1])),
        loc: Some(loc),
        line: None,
        item_count: None,
        method_count: None,
        complexity: None,
        cycle_kind: None,
    });

    // Walk tree for imports only (the file graph has no function/class nodes).
    let imports = extract_imports(&tree.root_node(), &source);

    for imp in &imports {
        let targets = resolve_import(&imp.base, &imp.names, mod_path, module_index);
        if targets.is_empty() {
            // Unresolved → an external (3rd-party / stdlib) dependency. Record it
            // at depth 1: one `External` node per top-level package.
            if let Some(top) = external_top_level(&imp.base) {
                let ext_id = format!("ext:{top}");
                builder.add_node(Node {
                    id: ext_id.clone(),
                    kind: NodeKind::External,
                    name: top,
                    path: String::new(),
                    parent: None,
                    external: Some(true),
                    visibility: None,
                    loc: None,
                    line: None,
                    item_count: None,
                    method_count: None,
                    complexity: None,
                    cycle_kind: None,
                });
                builder.add_edge(Edge {
                    from: file_id.clone(),
                    to: ext_id,
                    kind: EdgeKind::Uses,
                    unresolved: None,
                    external: Some(true),
                    visibility: None,
                });
            }
            continue;
        }
        for target_path in targets {
            let target_id = format!("file:{}", target_path.to_string_lossy());
            if target_id != file_id {
                builder.add_edge(Edge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: EdgeKind::Uses,
                    unresolved: None,
                    external: None,
                    visibility: None,
                });
            }
        }
    }

    Ok(())
}

/// Top-level package name for an unresolved import, or `None` for relative
/// imports (which are always project-internal and never external libraries).
fn external_top_level(base: &str) -> Option<String> {
    if base.starts_with('.') || base.is_empty() {
        return None;
    }
    Some(base.split('.').next().unwrap_or(base).to_string())
}

// ---------------------------------------------------------------------------
// Tree-sitter extraction (imports only)
// ---------------------------------------------------------------------------

fn extract_imports(root: &tree_sitter::Node, source: &[u8]) -> Vec<ExtractedImport> {
    let mut imports = Vec::new();
    visit_imports(root, source, &mut imports);
    imports
}

fn visit_imports<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    imports: &mut Vec<ExtractedImport>,
) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();

    for child in &children {
        match child.kind() {
            "import_statement" => {
                // import a.b.c  OR  import a, b
                let mut ic = child.walk();
                for c in child.children(&mut ic) {
                    let actual = if c.kind() == "aliased_import" {
                        c.child_by_field_name("name").unwrap_or(c)
                    } else {
                        c
                    };
                    if actual.kind() == "dotted_name"
                        && let Ok(t) = actual.utf8_text(source)
                    {
                        imports.push(ExtractedImport {
                            base: t.to_string(),
                            names: vec![],
                        });
                    }
                }
            }
            "import_from_statement" => {
                let base = child
                    .child_by_field_name("module_name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("")
                    .to_string();

                let mut names = Vec::new();
                let mut ic = child.walk();
                for c in child.children(&mut ic) {
                    let actual = if c.kind() == "aliased_import" {
                        c.child_by_field_name("name").unwrap_or(c)
                    } else {
                        c
                    };
                    if actual.kind() == "dotted_name"
                        && actual.start_byte()
                            != child
                                .child_by_field_name("module_name")
                                .map_or(0, |n| n.start_byte())
                        && let Ok(t) = actual.utf8_text(source)
                    {
                        names.push(t.to_string());
                    }
                }

                if !base.is_empty() {
                    imports.push(ExtractedImport { base, names });
                }
            }
            _ => {
                visit_imports(child, source, imports);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Import resolution
// ---------------------------------------------------------------------------

/// Resolve one import record to a set of target file paths in this project.
fn resolve_import(
    base: &str,
    names: &[String],
    current_mod: &str,
    index: &HashMap<String, PathBuf>,
) -> Vec<PathBuf> {
    let abs_base = absolute_base(base, current_mod);
    let mut results: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut try_add = |mod_path: &str| {
        if let Some(p) = index.get(mod_path)
            && seen.insert(p.clone())
        {
            results.push(p.clone());
        }
    };

    if names.is_empty() {
        // plain `import X.Y.Z`
        try_add(&abs_base);
    } else {
        for name in names {
            let full = if abs_base.is_empty() {
                name.clone()
            } else {
                format!("{abs_base}.{name}")
            };
            try_add(&full);
        }
        // Also add the base itself (might import symbols from it)
        if !abs_base.is_empty() {
            try_add(&abs_base);
        }
    }

    results
}

/// Turn a possibly-relative base like `"."`, `".utils"`, `"..shops"` into
/// an absolute dotted module path using `current_mod` as the anchor.
fn absolute_base(base: &str, current_mod: &str) -> String {
    if !base.starts_with('.') {
        return base.to_string();
    }

    let dots = base.chars().take_while(|&c| c == '.').count();
    let suffix = base[dots..].to_string(); // part after dots (may be empty)

    let parts: Vec<&str> = current_mod.split('.').collect();
    let keep = parts.len().saturating_sub(dots);
    let pkg = parts[..keep].join(".");

    if suffix.is_empty() {
        pkg
    } else if pkg.is_empty() {
        suffix
    } else {
        format!("{pkg}.{suffix}")
    }
}

// ---------------------------------------------------------------------------
// Visibility heuristic
// ---------------------------------------------------------------------------

fn py_visibility(name: &str) -> Visibility {
    if name.starts_with("__") && !name.ends_with("__") {
        Visibility::Private
    } else if name.starts_with('_') {
        Visibility::Restricted {
            path: "module".into(),
        }
    } else {
        Visibility::Public
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_split_core::graph::Graph;
    use std::fs;
    use tempfile::TempDir;

    // ── pure helpers ────────────────────────────────────────────────────────

    #[test]
    fn file_to_module_path_maps_files_and_packages() {
        let ws = Path::new("/proj");
        let cases: Vec<(&str, Option<&str>)> = vec![
            (
                "/proj/parser/shops/amazon/pdp.py",
                Some("parser.shops.amazon.pdp"),
            ),
            ("/proj/pkg/__init__.py", Some("pkg")), // package → drops __init__
            ("/proj/top.py", Some("top")),          // top-level module
            ("/proj/__init__.py", None),            // root package → no path
            ("/proj/notes.txt", None),              // not a .py file
        ];
        for (path, expected) in cases {
            let got = file_to_module_path(ws, Path::new(path));
            assert_eq!(got.as_deref(), expected, "for {path}");
        }
    }

    #[test]
    fn is_skip_path_skips_dot_and_vendor_dirs() {
        let ws = Path::new("/proj");
        for p in [
            "/proj/.git/x.py",
            "/proj/venv/x.py",
            "/proj/__pycache__/x.py",
            "/proj/sub/node_modules/x.py",
        ] {
            assert!(is_skip_path(Path::new(p), ws), "should skip {p}");
        }
        assert!(
            !is_skip_path(Path::new("/proj/src/app.py"), ws),
            "normal source is not skipped"
        );
    }

    #[test]
    fn absolute_base_resolves_relative_imports() {
        let cur = "a.b.c";
        let cases: Vec<(&str, &str, &str)> = vec![
            ("pkg.sub", "x.y", "pkg.sub"), // absolute import is unchanged
            (".", cur, "a.b"),             // one dot → drop the current module
            (".utils", cur, "a.b.utils"),  // one dot + suffix
            ("..shops", cur, "a.shops"),   // two dots + suffix
        ];
        for (base, current, expected) in cases {
            assert_eq!(
                absolute_base(base, current),
                expected,
                "base={base:?} cur={current:?}"
            );
        }
    }

    #[test]
    fn external_top_level_skips_relative_and_takes_top_segment() {
        assert_eq!(external_top_level("numpy.linalg").as_deref(), Some("numpy"));
        assert_eq!(external_top_level("requests").as_deref(), Some("requests"));
        assert_eq!(external_top_level(".utils"), None);
        assert_eq!(external_top_level(""), None);
    }

    #[test]
    fn resolve_import_finds_submodule_and_package() {
        let index: HashMap<String, PathBuf> = HashMap::from([
            ("pkg.b".to_string(), PathBuf::from("/p/pkg/b.py")),
            ("pkg".to_string(), PathBuf::from("/p/pkg/__init__.py")),
        ]);
        let got = resolve_import("pkg", &["b".to_string()], "pkg.a", &index);
        assert!(got.contains(&PathBuf::from("/p/pkg/b.py")), "submodule b");
        assert!(
            got.contains(&PathBuf::from("/p/pkg/__init__.py")),
            "package pkg"
        );
    }

    #[test]
    fn py_visibility_classifies_by_underscore_convention() {
        assert_eq!(py_visibility("public"), Visibility::Public);
        assert_eq!(py_visibility("__private"), Visibility::Private);
        assert_eq!(
            py_visibility("_protected"),
            Visibility::Restricted {
                path: "module".into()
            }
        );
    }

    // ── end-to-end: a tiny package through run() ─────────────────────────────

    fn write(dir: &Path, rel: &str, contents: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }

    fn has_node(g: &Graph, id: &str) -> bool {
        g.nodes.iter().any(|n| n.id == id)
    }

    #[test]
    fn run_builds_a_file_graph_with_imports_and_externals() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "pkg/__init__.py", "");
        write(
            root,
            "pkg/a.py",
            "import os\n\
             from pkg import b\n\
             \n\
             def helper():\n\
             \x20   return b.greet()\n",
        );
        write(root, "pkg/b.py", "def greet():\n    return \"hi\"\n");

        let (graphs, _timings) = run(root, false).expect("python plugin runs");
        let g = &graphs.files;

        // Only File + External nodes — no module/class/function nodes.
        assert!(
            g.nodes
                .iter()
                .all(|n| matches!(n.kind, NodeKind::File | NodeKind::External)),
            "files graph holds only File/External nodes"
        );

        // file→file import edge a.py → b.py.
        let a_id = format!("file:{}", root.join("pkg/a.py").to_string_lossy());
        let b_id = format!("file:{}", root.join("pkg/b.py").to_string_lossy());
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == a_id && e.to == b_id && e.kind == EdgeKind::Uses),
            "expected import edge a.py → b.py"
        );

        // external stdlib import `os` becomes one External node, flagged on the edge.
        assert!(has_node(g, "ext:os"), "external node for os");
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == a_id && e.to == "ext:os" && e.external == Some(true)),
            "external edge a.py → os flagged external"
        );
    }
}
