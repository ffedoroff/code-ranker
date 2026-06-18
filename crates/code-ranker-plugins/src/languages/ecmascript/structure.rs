//! The ECMAScript import/dependency-graph structure builder.
//!
//! The grammar-agnostic walker + resolver that turns an ECMAScript workspace
//! into an [`api::Graph`] of `file` + `external` nodes connected by `"uses"`
//! edges. The concrete tree-sitter grammar is **injected by the caller** (the
//! JS / TS plugins via [`analyze_ecmascript`]'s `lang_for_ext`), so this module
//! names no language. Reads its node-kind vocabulary from
//! [`super::cfg::CONFIG`]'s `[structure]` table — it depends on `cfg` downward,
//! never back up on `mod.rs` (keeping the ECMAScript module graph acyclic).

use anyhow::Result;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Walk skip lists the ECMAScript file collector prunes on, resolved once from
/// `ecmascript/config.toml`. `dirs` match a path component exactly; `suffixes`
/// match a component via `ends_with` (generated / config / minified artifacts).
/// The walk LOGIC (and the leading-`.` syntax rule) stays in Rust; *which* names
/// it prunes is data. Read from the leaf `cfg::CONFIG` (depended on downward,
/// keeping the ECMAScript module graph acyclic — see `cfg.rs`).
struct SkipLists {
    dirs: Vec<String>,
    suffixes: Vec<String>,
}

static SKIP: LazyLock<SkipLists> = LazyLock::new(|| SkipLists {
    dirs: crate::config::string_list(&super::cfg::CONFIG, "skip_dirs"),
    suffixes: crate::config::string_list(&super::cfg::CONFIG, "skip_suffixes"),
});

/// Test-path convention DATA the `ecmascript_is_test_path` predicate keys on,
/// resolved once from `ecmascript/config.toml`. The predicate LOGIC (split '/',
/// `contains`, `ends_with`) stays in Rust; *which* names it matches is data.
/// `dirs` match a path component exactly; `infixes` match the filename via
/// `contains`; `stem_suffixes` match the filename stem via `ends_with`.
struct TestLists {
    dirs: Vec<String>,
    infixes: Vec<String>,
    stem_suffixes: Vec<String>,
}

static TEST: LazyLock<TestLists> = LazyLock::new(|| TestLists {
    dirs: crate::config::string_list(&super::cfg::CONFIG, "test_dirs"),
    infixes: crate::config::string_list(&super::cfg::CONFIG, "test_infixes"),
    stem_suffixes: crate::config::string_list(&super::cfg::CONFIG, "test_stem_suffixes"),
});

/// Source-root + module-path DATA, resolved once from `ecmascript/config.toml`.
/// The detection LOGIC stays in Rust; the names it keys on are data. `source_dirs`
/// are the workspace subfolders `find_source_root` prefers; `module_strip_exts`
/// are the extensions `file_to_mod_path` strips (order matters — first wins);
/// `index_file` is the implicit module stem collapsed into its parent dir.
struct ModuleLists {
    source_dirs: Vec<String>,
    strip_exts: Vec<String>,
    index_file: String,
    alias_prefix: String,
}

static MODULE: LazyLock<ModuleLists> = LazyLock::new(|| ModuleLists {
    source_dirs: crate::config::string_list(&super::cfg::CONFIG, "source_dirs"),
    strip_exts: crate::config::string_list(&super::cfg::CONFIG, "module_strip_exts"),
    index_file: super::cfg::CONFIG
        .get("index_file")
        .and_then(|v| v.as_str())
        .expect("ecmascript/config.toml `index_file`")
        .to_string(),
    alias_prefix: super::cfg::CONFIG
        .get("alias_prefix")
        .and_then(|v| v.as_str())
        .expect("ecmascript/config.toml `alias_prefix`")
        .to_string(),
});

/// The `uses` edge-kind identifier the structure builder tags `uses` edges with,
/// resolved via `config::edge_kind_id` against the merged `[edge_kinds]` (the
/// same table `ecmascript_level()` publishes), so the tagged `kind` and the level
/// descriptor can never drift. Mirrors `rust/collapse.rs`'s pattern: the `"uses"`
/// lookup key is the variant slot (an internal classification, not a bare output
/// literal); `edge_kind_id` validates it against the published vocabulary.
fn uses_edge_kind() -> &'static str {
    let key = "uses";
    crate::config::edge_kind_id(&super::cfg::CONFIG, key)
        .unwrap_or_else(|| panic!("ecmascript/config.toml [edge_kinds] is missing `{key}`"));
    key
}

/// A node-attribute key, validated against `[node_attributes]` (inherited from
/// `defaults.toml`) so an inserted attr can never use an undeclared key. Mirrors
/// `uses_edge_kind` / `rust/collapse.rs::attr_key`.
fn attr_key(key: &'static str) -> &'static str {
    crate::config::attr_key(&super::cfg::CONFIG, key)
        .unwrap_or_else(|| panic!("ecmascript [node_attributes] is missing `{key}`"));
    key
}

/// Walk `workspace`, parse every file whose extension is in `exts`, and build
/// an [`api::Graph`] of file + external nodes connected by `"uses"` edges.
///
/// `lang_for_ext` maps a file extension to a tree-sitter [`Language`]. Return
/// `None` to skip the file (the walker already filters by `exts`; returning
/// `None` here is an escape hatch for finer control).
///
/// `candidate_exts_order` controls the order in which candidate extensions are
/// tried when resolving an extensionless import specifier, e.g. `"./foo"`. The
/// first match wins. Pass `&["ts", "tsx", "js", "jsx"]` for TypeScript-first
/// resolution; `&["js", "jsx", "mjs", "cjs"]` for JS-only projects.
pub fn analyze_ecmascript(
    workspace: &Path,
    exts: &[&str],
    lang_for_ext: impl Fn(&str) -> Option<tree_sitter::Language>,
    candidate_exts_order: &[&str],
    ignore_tests: bool,
    ignore: &crate::config::IgnoreCfg,
) -> Result<Graph> {
    let source_root = find_source_root(workspace);
    let alias_root = source_root.clone();
    let files = collect_files(&source_root, exts, ignore_tests, ignore);
    let file_index = build_file_index(workspace, &files);

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    // Track external nodes we already emitted to avoid duplicates.
    let mut ext_seen: HashMap<String, ()> = HashMap::new();
    // Track file nodes we already emitted.
    let mut file_ids_seen: HashMap<String, ()> = HashMap::new();

    for abs_path in &files {
        let ext = abs_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let language = match lang_for_ext(ext) {
            Some(l) => l,
            None => continue,
        };

        let source = match std::fs::read(abs_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&language)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let tree = match ts_parser.parse(&source, None) {
            Some(t) => t,
            None => continue,
        };

        let loc = source.iter().filter(|&&b| b == b'\n').count() as i64 + 1;
        let file_id = abs_path.to_string_lossy().into_owned();

        if !file_ids_seen.contains_key(&file_id) {
            file_ids_seen.insert(file_id.clone(), ());
            let mut attrs = BTreeMap::new();
            attrs.insert(
                attr_key("visibility").to_string(),
                AttrValue::Str(super::cfg::VISIBILITY_PUBLIC.clone()),
            );
            attrs.insert(attr_key("loc").to_string(), AttrValue::Int(loc));
            nodes.push(Node {
                id: file_id.clone(),
                kind: code_ranker_plugin_api::node::FILE.to_string(),
                name: abs_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                parent: None,
                attrs,
            });
        }

        let specifiers = extract_import_specifiers(&tree.root_node(), &source);

        for (spec, line) in &specifiers {
            if let Some(target) = resolve_import(
                spec,
                abs_path,
                workspace,
                &alias_root,
                &file_index,
                candidate_exts_order,
            ) {
                let target_id = target.to_string_lossy().into_owned();
                if target_id != file_id {
                    edges.push(Edge {
                        source: file_id.clone(),
                        target: target_id,
                        kind: uses_edge_kind().to_string(),
                        line: Some(*line),
                        attrs: BTreeMap::new(),
                    });
                }
            } else if let Some(pkg) = external_package(spec) {
                let ext_id = format!("{}{pkg}", super::cfg::EXTERNAL_ID_PREFIX.as_str());
                if !ext_seen.contains_key(&ext_id) {
                    ext_seen.insert(ext_id.clone(), ());
                    let mut attrs = BTreeMap::new();
                    attrs.insert(attr_key("external").to_string(), AttrValue::Bool(true));
                    nodes.push(Node {
                        id: ext_id.clone(),
                        kind: code_ranker_plugin_api::node::EXTERNAL.to_string(),
                        name: pkg,
                        parent: None,
                        attrs,
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: ext_id,
                    kind: uses_edge_kind().to_string(),
                    line: Some(*line),
                    attrs: BTreeMap::new(),
                });
            }
        }
    }

    Ok(Graph { nodes, edges })
}

// ─────────────────────────────────────────────────────────────────────────────
// Source root detection
// ─────────────────────────────────────────────────────────────────────────────

fn find_source_root(workspace: &Path) -> PathBuf {
    // The preferred source subfolders are DATA (`source_dirs`); the LOGIC (first
    // that exists wins, else the workspace itself) stays in Rust.
    for dir in &MODULE.source_dirs {
        let candidate = workspace.join(dir);
        if candidate.is_dir() {
            return candidate;
        }
    }
    workspace.to_owned()
}

// ─────────────────────────────────────────────────────────────────────────────
// File discovery
// ─────────────────────────────────────────────────────────────────────────────

fn collect_files(
    root: &Path,
    exts: &[&str],
    ignore_tests: bool,
    ignore: &crate::config::IgnoreCfg,
) -> Vec<PathBuf> {
    crate::walk::collect(root, &SKIP.dirs, ignore, |p| {
        p.extension()
            .is_some_and(|x| exts.contains(&x.to_str().unwrap_or("")))
    })
    .into_iter()
    .filter(|p| !has_skip_suffix(p, root))
    .filter(|p| !(ignore_tests && is_test_file(p, root)))
    .collect()
}

/// ECMAScript test conventions, shared by the JS and TS plugins: `*.test.*` /
/// `*.spec.*` files and anything under `__tests__`, `__mocks__`, `tests` or
/// `test` directories.
pub fn ecmascript_is_test_path(rel_path: &str) -> bool {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = file.split('.').next().unwrap_or(file);
    rel_path
        .split('/')
        .any(|c| TEST.dirs.iter().any(|d| d == c))
        || TEST.infixes.iter().any(|i| file.contains(i.as_str()))
        || TEST
            .stem_suffixes
            .iter()
            .any(|s| stem.ends_with(s.as_str()))
}

/// Workspace-relative test check used during the walk.
fn is_test_file(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|rel| ecmascript_is_test_path(&rel.to_string_lossy().replace('\\', "/")))
        .unwrap_or(false)
}

/// True if any path component (relative to `root`) ends with a configured
/// skip-suffix (`.gen.ts`, `.config.js`, …) — DATA from `config.toml`. The
/// dotted-component / skip-dir pruning and `.gitignore`/`.ignore` handling are
/// done by the shared walker (`crate::walk`); only this language-specific
/// suffix rule stays here as a post-filter.
fn has_skip_suffix(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .map(|rel| {
            rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                SKIP.suffixes.iter().any(|suf| s.ends_with(suf.as_str()))
            })
        })
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// Module path helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `src/lib/utils.ts` → `src/lib/utils`
/// `src/lib/index.ts` → `src/lib`
fn file_to_mod_path(workspace: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    let last = parts.last_mut()?;
    for ext in &MODULE.strip_exts {
        if let Some(stem) = last.strip_suffix(ext.as_str()) {
            *last = stem.to_string();
            break;
        }
    }
    if parts
        .last()
        .map(|s| *s == MODULE.index_file)
        .unwrap_or(false)
    {
        parts.pop();
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Build a map: module_path → abs_path for all collected files.
fn build_file_index(workspace: &Path, files: &[PathBuf]) -> HashMap<String, PathBuf> {
    files
        .iter()
        .filter_map(|p| file_to_mod_path(workspace, p).map(|m| (m, p.clone())))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// External package name extraction
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the package name for a bare (non-relative, non-alias) import
/// specifier: `react` → `react`, `lodash/fp` → `lodash`,
/// `@scope/pkg/sub` → `@scope/pkg`.
/// Returns `None` for relative (`./`, `../`) and `@/` alias specifiers.
pub fn external_package(spec: &str) -> Option<String> {
    if spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with(MODULE.alias_prefix.as_str())
        || spec.is_empty()
    {
        return None;
    }
    let mut it = spec.split('/');
    let first = it.next().unwrap_or(spec);
    if first.starts_with('@') {
        match it.next() {
            Some(second) => Some(format!("{first}/{second}")),
            None => Some(first.to_string()),
        }
    } else {
        Some(first.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tree-sitter extraction (import / require specifiers)
// ─────────────────────────────────────────────────────────────────────────────

/// The import/module-graph tree-sitter NODE-KIND strings the walk keys on,
/// resolved once from `ecmascript/config.toml`'s `[structure]` table. The walk
/// LOGIC stays in Rust; *which* node kinds it matches is data.
struct StructureKinds {
    import_statement: String,
    export_statement: String,
    call_expression: String,
    string: String,
    require: String,
    /// tree-sitter field names for the `require(...)` call (`[fields]`).
    field_function: String,
    field_arguments: String,
}

impl StructureKinds {
    fn load() -> Self {
        let s = crate::config::string_table(&super::cfg::CONFIG, "structure");
        let get = |k: &str| s.get(k).cloned().expect("[structure] key");
        let f = crate::config::string_table(&super::cfg::CONFIG, "fields");
        let field = |k: &str| f.get(k).cloned().expect("[fields] key");
        StructureKinds {
            import_statement: get("import_statement"),
            export_statement: get("export_statement"),
            call_expression: get("call_expression"),
            string: get("string"),
            require: get("require"),
            field_function: field("function"),
            field_arguments: field("arguments"),
        }
    }
}

/// Each specifier paired with the 1-based line of its import/export/require.
fn extract_import_specifiers(root: &tree_sitter::Node, source: &[u8]) -> Vec<(String, u32)> {
    let kinds = StructureKinds::load();
    let mut specs = Vec::new();
    visit_imports(root, source, &kinds, &mut specs);
    specs
}

fn visit_imports<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    kinds: &StructureKinds,
    specs: &mut Vec<(String, u32)>,
) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();

    for child in &children {
        let line = child.start_position().row as u32 + 1;
        let kind = child.kind();
        if kind == kinds.import_statement {
            // import 'module' / import { x } from 'module'
            if let Some(src) = import_source(child, source, kinds) {
                specs.push((src, line));
            }
        } else if kind == kinds.export_statement {
            // export { x } from 'module'  /  export * from 'module'
            if let Some(src) = import_source(child, source, kinds) {
                specs.push((src, line));
            }
            visit_imports(child, source, kinds, specs);
        } else if kind == kinds.call_expression {
            if let Some(src) = require_source(child, source, kinds) {
                specs.push((src, line));
            } else {
                visit_imports(child, source, kinds, specs);
            }
        } else {
            visit_imports(child, source, kinds, specs);
        }
    }
}

/// Extract the module specifier string from an import or re-export statement.
fn import_source(
    node: &tree_sitter::Node,
    source: &[u8],
    kinds: &StructureKinds,
) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for child in children.iter().rev() {
        if child.kind() == kinds.string
            && let Ok(raw) = child.utf8_text(source)
        {
            let trimmed = raw.trim_matches(|c| c == '\'' || c == '"' || c == '`');
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Extract `require("./path")` specifier from a call_expression node.
fn require_source(
    node: &tree_sitter::Node,
    source: &[u8],
    kinds: &StructureKinds,
) -> Option<String> {
    let fn_node = node.child_by_field_name(&kinds.field_function)?;
    let fn_text = fn_node.utf8_text(source).ok()?;
    if fn_text != kinds.require {
        return None;
    }
    let args = node.child_by_field_name(&kinds.field_arguments)?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == kinds.string
            && let Ok(raw) = child.utf8_text(source)
        {
            let trimmed = raw.trim_matches(|c| c == '\'' || c == '"' || c == '`');
            return Some(trimmed.to_string());
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Import resolution
// ─────────────────────────────────────────────────────────────────────────────

fn resolve_import(
    specifier: &str,
    from_file: &Path,
    workspace: &Path,
    alias_root: &Path,
    file_index: &HashMap<String, PathBuf>,
    candidate_exts_order: &[&str],
) -> Option<PathBuf> {
    let base_path: PathBuf = if specifier.starts_with("./") || specifier.starts_with("../") {
        from_file.parent()?.join(specifier)
    } else if let Some(rest) = specifier.strip_prefix(MODULE.alias_prefix.as_str()) {
        alias_root.join(rest)
    } else {
        return None;
    };

    let normalized = normalize_path(&base_path);

    // Build candidate list: bare path with each extension, then index.* with each extension.
    let mut candidates: Vec<PathBuf> = Vec::new();
    for ext in candidate_exts_order {
        candidates.push(normalized.with_extension(ext));
    }
    // The implicit module stem (`index`) is DATA (`index_file` in the config),
    // already resolved in `MODULE`; reuse it rather than re-hardcoding "index".
    for ext in candidate_exts_order {
        candidates.push(normalized.join(format!("{}.{ext}", MODULE.index_file)));
    }

    for candidate in &candidates {
        if let Some(mod_path) = file_to_mod_path(workspace, candidate)
            && file_index.contains_key(&mod_path)
        {
            return file_index.get(&mod_path).cloned();
        }
    }
    None
}

/// Resolve `.` and `..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod structure_tests;
