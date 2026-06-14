use anyhow::Result;
use code_ranker_plugin_api::{
    attrs::{AttrValue, ValueType},
    default_cycle_kinds, default_node_kinds,
    edge::Edge,
    graph::Graph,
    level::{AttributeSpec, EdgeKindSpec, Level},
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// The Python language plugin (registered by the CLI).
pub struct PythonPlugin;

impl LanguagePlugin for PythonPlugin {
    fn name(&self) -> &str {
        "python"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        ["pyproject.toml", "setup.py", "setup.cfg"]
            .iter()
            .any(|f| workspace.join(f).exists())
    }

    fn levels(&self) -> Vec<Level> {
        let mut edge_kinds = BTreeMap::new();
        edge_kinds.insert(
            "uses".to_string(),
            EdgeKindSpec {
                flow: true,
                label: Some("uses".into()),
                description: Some(
                    "Import dependency \u{2014} this file imports from the other.".into(),
                ),
            },
        );

        let mut node_attributes: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        node_attributes.insert("path".into(), AttributeSpec::new(ValueType::Str, "Path"));
        node_attributes.insert("loc".into(), AttributeSpec::new(ValueType::Int, "Lines"));
        node_attributes.insert(
            "visibility".into(),
            AttributeSpec::new(ValueType::Str, "Visibility"),
        );
        node_attributes.insert(
            "external".into(),
            AttributeSpec::new(ValueType::Bool, "External"),
        );

        vec![Level {
            name: "files".into(),
            edge_kinds,
            node_attributes,
            edge_attributes: BTreeMap::new(),
            attribute_groups: BTreeMap::new(),
            node_kinds: default_node_kinds(),
            cycle_kinds: default_cycle_kinds(),
            grouping: None,
        }]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        analyze(workspace, input.ignore_tests)
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        py_is_test_path(rel_path)
    }
}

/// Python test conventions: pytest/unittest files (`test_*.py`, `*_test.py`,
/// `conftest.py`) and anything under a `tests/` directory.
fn py_is_test_path(rel_path: &str) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path.split('/').any(|c| c == "tests" || c == "test")
        || file == "conftest.py"
        || (file.starts_with("test_") && file.ends_with(".py"))
        || file.ends_with("_test.py")
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

fn analyze(workspace: &Path, ignore_tests: bool) -> Result<Graph> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let py_files = collect_py_files(workspace, ignore_tests);
    let module_index = build_module_index(workspace, &py_files);

    // Track external nodes already added (by id) to avoid duplicates.
    let mut ext_seen: HashSet<String> = HashSet::new();

    for abs_path in &py_files {
        let Some(mod_path) = file_to_module_path(workspace, abs_path) else {
            continue;
        };
        parse_and_add(
            abs_path,
            &mod_path,
            &module_index,
            &mut nodes,
            &mut edges,
            &mut ext_seen,
        )?;
    }

    Ok(Graph { nodes, edges })
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

fn collect_py_files(workspace: &Path, ignore_tests: bool) -> Vec<PathBuf> {
    WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().is_some_and(|x| x == "py")
                && !is_skip_path(e.path(), workspace)
                && !(ignore_tests && is_test_file(e.path(), workspace))
        })
        .map(|e| e.into_path())
        .collect()
}

/// Workspace-relative test check used during the walk.
fn is_test_file(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .ok()
        .map(|rel| py_is_test_path(&rel.to_string_lossy().replace('\\', "/")))
        .unwrap_or(false)
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
    line: u32,          // 1-based line of the import statement
}

fn parse_and_add(
    abs_path: &Path,
    mod_path: &str,
    module_index: &HashMap<String, PathBuf>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    ext_seen: &mut HashSet<String>,
) -> Result<()> {
    let source = std::fs::read(abs_path)?;

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed: {}", abs_path.display()))?;

    let loc = source.iter().filter(|&&b| b == b'\n').count() as i64 + 1;
    // NEW id scheme: plain absolute path (no "file:" prefix).
    let file_id = abs_path.to_string_lossy().into_owned();

    let parts: Vec<&str> = mod_path.split('.').collect();
    let vis_str = py_visibility_str(parts[parts.len() - 1]);

    let mut file_attrs = BTreeMap::new();
    file_attrs.insert("visibility".to_string(), AttrValue::Str(vis_str.into()));
    file_attrs.insert("loc".to_string(), AttrValue::Int(loc));

    nodes.push(Node {
        id: file_id.clone(),
        kind: "file".into(),
        name: abs_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        parent: None,
        attrs: file_attrs,
    });

    // Walk tree for imports only.
    let imports = extract_imports(&tree.root_node(), &source);

    for imp in &imports {
        let targets = resolve_import(&imp.base, &imp.names, mod_path, module_index);
        if targets.is_empty() {
            // Unresolved → external (3rd-party / stdlib). One External node per top-level package.
            if let Some(top) = external_top_level(&imp.base) {
                let ext_id = format!("ext:{top}");
                if ext_seen.insert(ext_id.clone()) {
                    let mut ext_attrs = BTreeMap::new();
                    ext_attrs.insert("external".to_string(), AttrValue::Bool(true));
                    nodes.push(Node {
                        id: ext_id.clone(),
                        kind: "external".into(),
                        name: top,
                        parent: None,
                        attrs: ext_attrs,
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: ext_id,
                    kind: "uses".into(),
                    line: Some(imp.line),
                    attrs: BTreeMap::new(),
                });
            }
            continue;
        }
        for target_path in targets {
            let target_id = target_path.to_string_lossy().into_owned();
            if target_id != file_id {
                edges.push(Edge {
                    source: file_id.clone(),
                    target: target_id,
                    kind: "uses".into(),
                    line: Some(imp.line),
                    attrs: BTreeMap::new(),
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
                let line = child.start_position().row as u32 + 1;
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
                            line,
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
                    let line = child.start_position().row as u32 + 1;
                    imports.push(ExtractedImport { base, names, line });
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
        // Also add the base itself (might import symbols from it).
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

fn py_visibility_str(name: &str) -> &'static str {
    if name.starts_with("__") && !name.ends_with("__") {
        "private"
    } else if name.starts_with('_') {
        "restricted"
    } else {
        "public"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_test_support::{has_edge, has_node, write_file as write};
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
    fn py_visibility_str_classifies_by_underscore_convention() {
        assert_eq!(py_visibility_str("public"), "public");
        assert_eq!(py_visibility_str("__private"), "private");
        assert_eq!(py_visibility_str("_protected"), "restricted");
        // dunder names (e.g. __init__) are not "private" (they have trailing __)
        // but they start with '_' so they are "restricted" by the heuristic.
        assert_eq!(py_visibility_str("__init__"), "restricted");
    }

    #[test]
    fn py_is_test_path_matches_pytest_conventions() {
        for p in [
            "tests/test_foo.py",
            "pkg/test_bar.py",
            "pkg/bar_test.py",
            "conftest.py",
            "pkg/tests/helper.py",
        ] {
            assert!(py_is_test_path(p), "should be a test: {p}");
        }
        for p in ["pkg/app.py", "pkg/latest.py", "pkg/contest.py"] {
            assert!(!py_is_test_path(p), "should not be a test: {p}");
        }
    }

    // ── end-to-end: a tiny package through analyze() ────────────────────────

    #[test]
    fn analyze_builds_a_file_graph_with_imports_and_externals() {
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

        let plugin = PythonPlugin;
        let input = PluginInput::default();
        let g = plugin
            .analyze(root, "files", &input)
            .expect("python plugin runs");

        // Only "file" + "external" kind nodes — no module/class/function nodes.
        assert!(
            g.nodes
                .iter()
                .all(|n| n.kind == "file" || n.kind == "external"),
            "graph holds only file/external nodes"
        );

        // file→file import edge a.py → b.py (new id scheme: bare absolute path).
        let a_id = root.join("pkg/a.py").to_string_lossy().into_owned();
        let b_id = root.join("pkg/b.py").to_string_lossy().into_owned();
        assert!(
            g.edges
                .iter()
                .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses"),
            "expected import edge a.py → b.py"
        );

        // external stdlib import `os` becomes one External node.
        assert!(has_node(&g, "ext:os"), "external node for os");
        assert!(
            g.edges
                .iter()
                .any(|e| e.source == a_id && e.target == "ext:os" && e.kind == "uses"),
            "external edge a.py → os"
        );

        // Check structural attrs on a file node.
        let a_node = g.nodes.iter().find(|n| n.id == a_id).unwrap();
        assert!(a_node.attrs.contains_key("visibility"), "visibility attr");
        assert!(a_node.attrs.contains_key("loc"), "loc attr");
    }

    #[test]
    fn import_in_comment_or_string_is_not_an_edge() {
        // Layer-1 metamorphic FP guard (docs/metric-correctness.md): an import that
        // appears only in a comment, a docstring, or a string literal must NOT
        // create a dependency edge — imports are read from AST nodes, not text.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "pkg/__init__.py", "");
        write(
            root,
            "pkg/a.py",
            "\"\"\"from pkg import b and import os\"\"\"\n\
             # from pkg import b\n\
             note = \"from pkg import b\"\n\
             \n\
             def helper():\n\
             \x20   return note\n",
        );
        write(root, "pkg/b.py", "def greet():\n    return \"hi\"\n");

        let g = PythonPlugin
            .analyze(root, "files", &PluginInput::default())
            .expect("python plugin runs");

        let a_id = root.join("pkg/a.py").to_string_lossy().into_owned();
        let b_id = root.join("pkg/b.py").to_string_lossy().into_owned();
        assert!(
            !g.edges.iter().any(|e| e.source == a_id && e.target == b_id),
            "an import in a comment/docstring/string must not produce an edge"
        );
        assert!(
            !has_node(&g, "ext:os"),
            "`import os` only in text must not produce an external node"
        );
    }

    #[test]
    fn edges_scale_with_real_imports() {
        // Layer-2 generative (docs/metric-correctness.md): `a` really imports `n`
        // sibling modules; the edge count from `a` must equal `n`, swept over a grid.
        for n in 0..5 {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            write(root, "pkg/__init__.py", "");
            let mut a = String::new();
            for i in 0..n {
                a.push_str(&format!("from pkg import b{i}\n"));
                write(
                    root,
                    &format!("pkg/b{i}.py"),
                    &format!("def g{i}():\n    return {i}\n"),
                );
            }
            a.push_str("def helper():\n    return 1\n");
            write(root, "pkg/a.py", &a);

            let g = PythonPlugin
                .analyze(root, "files", &PluginInput::default())
                .expect("python plugin runs");

            let a_id = root.join("pkg/a.py").to_string_lossy().into_owned();
            // Count edges to the `b{i}` submodules specifically — `from pkg import
            // b{i}` also links the `pkg` package (`__init__.py`), which is correct
            // and not what this scaling property is about.
            let got = (0..n)
                .filter(|i| {
                    let b_id = root
                        .join(format!("pkg/b{i}.py"))
                        .to_string_lossy()
                        .into_owned();
                    g.edges
                        .iter()
                        .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses")
                })
                .count();
            assert_eq!(
                got, n,
                "expected an import edge to each of the {n} b-modules, got {got}"
            );
        }
    }

    #[test]
    fn aliased_and_namespace_imports_produce_edges() {
        // The walker resolves through `aliased_import` to the real module, so an
        // `as` rename is transparent: `import pkg.b as bb` (namespace-style) and
        // `from pkg import b as bb` (named alias) both yield the a.py → b.py edge.
        let cases: &[(&str, &str)] = &[
            ("module alias", "import pkg.b as bb\nbb\n"),
            ("from-import alias", "from pkg import b as bb\nbb\n"),
        ];
        let mut fails = Vec::new();
        for (label, a_src) in cases {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            write(root, "pkg/__init__.py", "");
            write(root, "pkg/b.py", "def greet():\n    return \"hi\"\n");
            write(root, "pkg/a.py", a_src);

            let g = PythonPlugin
                .analyze(root, "files", &PluginInput::default())
                .expect("python plugin runs");
            let a_id = root.join("pkg/a.py").to_string_lossy().into_owned();
            let b_id = root.join("pkg/b.py").to_string_lossy().into_owned();
            if !has_edge(&g, &a_id, &b_id, "uses") {
                fails.push(format!("{label}: expected edge a.py → b.py"));
            }
        }
        assert!(
            fails.is_empty(),
            "aliased imports not resolved:\n{}",
            fails.join("\n")
        );
    }
}
