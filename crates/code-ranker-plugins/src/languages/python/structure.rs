//! The Python dependency-graph (structure) builder.
//!
//! Imperative-only code: walk the workspace for `.py` files, map each to its
//! dotted module path, parse imports with `tree-sitter-python`, and resolve them
//! to `uses` edges between file nodes (with one `external` node per unresolved
//! top-level package). The thin `LanguagePlugin::analyze` in `mod.rs` calls
//! [`analyze`]; everything below is the machinery it drives.

use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Import-graph tree-sitter NODE-KIND strings the walk keys on, plus the
/// file-collection / skip-dir DATA lists, resolved once from `python/config.toml`.
/// Loaded here (self-contained, depending only on `crate::config`) rather than
/// via the parent `mod.rs`'s `CONFIG` to keep the Python module graph acyclic.
/// The walk LOGIC stays in Rust; *which* node kinds it matches, *which*
/// extensions it collects, and *which* directory names it prunes are data.
struct StructureKinds {
    import_statement: String,
    import_from_statement: String,
    dotted_name: String,
    aliased_import: String,
    /// Source-file extensions the walk collects (`["py"]`).
    extensions: Vec<String>,
    /// Directory names pruned by exact match during the walk (the leading-`.`
    /// rule is a separate syntax rule kept in `is_skip_path`).
    skip_dirs: Vec<String>,
    /// Test-path convention DATA the `py_is_test_path` predicate keys on (the
    /// predicate LOGIC stays in Rust). `test_dirs`: a path component matched
    /// exactly; `test_files`: a filename matched exactly; `test_prefixes`: a
    /// `.py` filename matched via `starts_with`; `test_suffixes`: a filename
    /// matched via `ends_with`.
    test_dirs: Vec<String>,
    test_files: Vec<String>,
    test_prefixes: Vec<String>,
    test_suffixes: Vec<String>,
    /// `ext:` node-id namespace prefix for an external node (`[ids].external`,
    /// inherited from `defaults.toml`).
    ext_prefix: String,
    /// The implicit package-init file stem (`pkg/__init__.py` → module `pkg`),
    /// from `package_init_file`.
    package_init_file: String,
    /// Source extensions with a leading dot (`[".py"]`), derived from
    /// `extensions` — the suffix `file_to_module_path` strips and
    /// `py_is_test_path` gates on. Not re-spelled in the config.
    dot_exts: Vec<String>,
    /// tree-sitter field names the import walk navigates (`[fields]`).
    field_name: String,
    field_module_name: String,
    /// Visibility output strings (`[visibility]`): `public` inherited from
    /// `defaults.toml`, `restricted` / `private` from `python/config.toml`.
    vis_public: String,
    vis_restricted: String,
    vis_private: String,
}

static KINDS: LazyLock<StructureKinds> = LazyLock::new(|| {
    let cfg = crate::config::load(include_str!("config.toml"));
    let s = crate::config::string_table(&cfg, "structure");
    let get = |k: &str| s.get(k).cloned().expect("[structure] key");
    let extensions = crate::config::string_list(&cfg, "extensions");
    let dot_exts = extensions.iter().map(|e| format!(".{e}")).collect();
    let f = crate::config::string_table(&cfg, "fields");
    let field = |k: &str| f.get(k).cloned().expect("[fields] key");
    let vis = crate::config::string_table(&cfg, "visibility");
    let vis_get = |k: &str| vis.get(k).cloned().expect("[visibility] key");
    StructureKinds {
        import_statement: get("import_statement"),
        import_from_statement: get("import_from_statement"),
        dotted_name: get("dotted_name"),
        aliased_import: get("aliased_import"),
        extensions,
        skip_dirs: crate::config::string_list(&cfg, "skip_dirs"),
        test_dirs: crate::config::string_list(&cfg, "test_dirs"),
        test_files: crate::config::string_list(&cfg, "test_files"),
        test_prefixes: crate::config::string_list(&cfg, "test_prefixes"),
        test_suffixes: crate::config::string_list(&cfg, "test_suffixes"),
        ext_prefix: crate::config::string_table(&cfg, "ids")
            .get("external")
            .cloned()
            .expect("python [ids].external (inherited from defaults.toml)"),
        package_init_file: cfg
            .get("package_init_file")
            .and_then(|v| v.as_str())
            .expect("python/config.toml `package_init_file`")
            .to_string(),
        dot_exts,
        field_name: field("name"),
        field_module_name: field("module_name"),
        vis_public: vis_get("public"),
        vis_restricted: vis_get("restricted"),
        vis_private: vis_get("private"),
    }
});

/// The `uses` edge-kind identifier the structure builder tags `uses` edges with,
/// resolved via `config::edge_kind_id` against the merged `[edge_kinds]` (the same
/// table `mod.rs::levels()` publishes), so the tagged `kind` and the level
/// descriptor can never drift. Mirrors `rust/collapse.rs`'s pattern: the `"uses"`
/// lookup key is the variant slot, validated against the published vocabulary.
fn uses_edge_kind() -> &'static str {
    static USES: LazyLock<()> = LazyLock::new(|| {
        let cfg = crate::config::load(include_str!("config.toml"));
        crate::config::edge_kind_id(&cfg, "uses")
            .unwrap_or_else(|| panic!("python/config.toml [edge_kinds] is missing `uses`"));
    });
    LazyLock::force(&USES);
    "uses"
}

/// A node-attribute key, validated against `[node_attributes]` (inherited from
/// `defaults.toml`) so an inserted attr can never use an undeclared key. Mirrors
/// `uses_edge_kind`.
fn attr_key(key: &'static str) -> &'static str {
    static CFG: LazyLock<toml::Table> =
        LazyLock::new(|| crate::config::load(include_str!("config.toml")));
    crate::config::attr_key(&CFG, key)
        .unwrap_or_else(|| panic!("python [node_attributes] is missing `{key}`"));
    key
}

/// Python test conventions: pytest/unittest files (`test_*.py`, `*_test.py`,
/// `conftest.py`) and anything under a `tests/` directory.
pub(super) fn py_is_test_path(rel_path: &str) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path
        .split('/')
        .any(|c| KINDS.test_dirs.iter().any(|d| d == c))
        || KINDS.test_files.iter().any(|f| f == file)
        || (KINDS.dot_exts.iter().any(|e| file.ends_with(e.as_str()))
            && KINDS
                .test_prefixes
                .iter()
                .any(|p| file.starts_with(p.as_str())))
        || KINDS
            .test_suffixes
            .iter()
            .any(|s| file.ends_with(s.as_str()))
}

pub(super) fn analyze(
    workspace: &Path,
    ignore_tests: bool,
    ignore: &crate::config::IgnoreCfg,
) -> Result<Graph> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let py_files = collect_py_files(workspace, ignore_tests, ignore);
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

fn collect_py_files(
    workspace: &Path,
    ignore_tests: bool,
    ignore: &crate::config::IgnoreCfg,
) -> Vec<PathBuf> {
    crate::walk::collect(workspace, &KINDS.skip_dirs, ignore, |p| {
        p.extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| KINDS.extensions.iter().any(|e| e == x))
    })
    .into_iter()
    .filter(|p| !(ignore_tests && is_test_file(p, workspace)))
    .collect()
}

/// Workspace-relative test check used during the walk.
fn is_test_file(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .ok()
        .map(|rel| py_is_test_path(&rel.to_string_lossy().replace('\\', "/")))
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
    if *last == KINDS.package_init_file {
        parts.pop();
    } else if let Some(stem) = KINDS
        .dot_exts
        .iter()
        .find_map(|e| last.strip_suffix(e.as_str()))
    {
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
    file_attrs.insert(
        attr_key("visibility").to_string(),
        AttrValue::Str(vis_str.into()),
    );
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

    // Walk tree for imports only.
    let imports = extract_imports(&tree.root_node(), &source);

    for imp in &imports {
        let targets = resolve_import(&imp.base, &imp.names, mod_path, module_index);
        if targets.is_empty() {
            // Unresolved → external (3rd-party / stdlib). One External node per top-level package.
            if let Some(top) = external_top_level(&imp.base) {
                let ext_id = format!("{}{top}", KINDS.ext_prefix);
                if ext_seen.insert(ext_id.clone()) {
                    let mut ext_attrs = BTreeMap::new();
                    ext_attrs.insert(attr_key("external").to_string(), AttrValue::Bool(true));
                    nodes.push(Node {
                        id: ext_id.clone(),
                        kind: code_ranker_plugin_api::node::EXTERNAL.into(),
                        name: top,
                        parent: None,
                        attrs: ext_attrs,
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: ext_id,
                    kind: uses_edge_kind().into(),
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
                    kind: uses_edge_kind().into(),
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
        let kind = child.kind();
        if kind == KINDS.import_statement {
            // import a.b.c  OR  import a, b
            let line = child.start_position().row as u32 + 1;
            let mut ic = child.walk();
            for c in child.children(&mut ic) {
                let actual = if c.kind() == KINDS.aliased_import {
                    c.child_by_field_name(&KINDS.field_name).unwrap_or(c)
                } else {
                    c
                };
                if actual.kind() == KINDS.dotted_name
                    && let Ok(t) = actual.utf8_text(source)
                {
                    imports.push(ExtractedImport {
                        base: t.to_string(),
                        names: vec![],
                        line,
                    });
                }
            }
        } else if kind == KINDS.import_from_statement {
            let base = child
                .child_by_field_name(&KINDS.field_module_name)
                .and_then(|n| n.utf8_text(source).ok())
                .unwrap_or("")
                .to_string();

            let mut names = Vec::new();
            let mut ic = child.walk();
            for c in child.children(&mut ic) {
                let actual = if c.kind() == KINDS.aliased_import {
                    c.child_by_field_name(&KINDS.field_name).unwrap_or(c)
                } else {
                    c
                };
                if actual.kind() == KINDS.dotted_name
                    && actual.start_byte()
                        != child
                            .child_by_field_name(&KINDS.field_module_name)
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
        } else {
            visit_imports(child, source, imports);
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
    // Output strings are DATA (`[visibility]`); the `_`/`__` naming-convention
    // LOGIC stays here as a Python syntax rule.
    if name.starts_with("__") && !name.ends_with("__") {
        &KINDS.vis_private
    } else if name.starts_with('_') {
        &KINDS.vis_restricted
    } else {
        &KINDS.vis_public
    }
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod structure_tests;
