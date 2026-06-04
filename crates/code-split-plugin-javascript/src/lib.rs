//! JavaScript language plugin for Code Split.
//!
//! Handles `.js`, `.jsx`, `.mjs`, `.cjs` files via tree-sitter-javascript.
//! Also exposes shared ECMAScript parsing helpers (`ecmascript_level`,
//! `analyze_ecmascript`, `detect_with_marker`) so the TypeScript plugin can
//! reuse the walker/resolver without any copy-paste.

use anyhow::Result;
use code_split_plugin_api::{attrs::{AttrValue, ValueType}, edge::Edge, graph::Graph, level::{AttributeSpec, EdgeKindSpec, Level}, node::Node, plugin::{LanguagePlugin, PluginInput}, default_cycle_kinds, default_node_kinds};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ─────────────────────────────────────────────────────────────────────────────
// Public shared helpers (used by the TypeScript plugin)
// ─────────────────────────────────────────────────────────────────────────────

/// Build the single "files" [`Level`] that both JS and TS plugins expose.
///
/// `name` is the level name (pass `"files"` — kept as a parameter so tests can
/// verify the returned value without hard-coding a string twice).
pub fn ecmascript_level(name: &str) -> Level {
    let mut edge_kinds = BTreeMap::new();
    edge_kinds.insert(
        "uses".to_string(),
        EdgeKindSpec {
            flow: true,
            label: Some("uses".to_string()),
            description: Some(
                "Import dependency \u{2014} this file imports from the other.".to_string(),
            ),
        },
    );

    let mut node_attributes = BTreeMap::new();
    node_attributes.insert(
        "path".to_string(),
        AttributeSpec::new(ValueType::Str, "Path"),
    );
    node_attributes.insert(
        "loc".to_string(),
        AttributeSpec::new(ValueType::Int, "Lines"),
    );
    node_attributes.insert(
        "visibility".to_string(),
        AttributeSpec::new(ValueType::Str, "Visibility"),
    );
    node_attributes.insert(
        "external".to_string(),
        AttributeSpec::new(ValueType::Bool, "External"),
    );

    Level {
        name: name.to_string(),
        edge_kinds,
        node_attributes,
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds: default_node_kinds(),
        cycle_kinds: default_cycle_kinds(),
        grouping: None,
    }
}

/// Return `true` when `workspace` contains the given marker file.
///
/// Signature kept generic so both JS (`"package.json"`) and TS (`"tsconfig.json"`)
/// can reuse it.
pub fn detect_with_marker(workspace: &Path, marker: &str) -> bool {
    workspace.join(marker).exists()
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
) -> Result<Graph> {
    let source_root = find_source_root(workspace);
    let alias_root = source_root.clone();
    let files = collect_files(&source_root, exts);
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
                "visibility".to_string(),
                AttrValue::Str("public".to_string()),
            );
            attrs.insert("loc".to_string(), AttrValue::Int(loc));
            nodes.push(Node {
                id: file_id.clone(),
                kind: "file".to_string(),
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

        for spec in &specifiers {
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
                        kind: "uses".to_string(),
                        attrs: BTreeMap::new(),
                    });
                }
            } else if let Some(pkg) = external_package(spec) {
                let ext_id = format!("ext:{pkg}");
                if !ext_seen.contains_key(&ext_id) {
                    ext_seen.insert(ext_id.clone(), ());
                    let mut attrs = BTreeMap::new();
                    attrs.insert("external".to_string(), AttrValue::Bool(true));
                    nodes.push(Node {
                        id: ext_id.clone(),
                        kind: "external".to_string(),
                        name: pkg,
                        parent: None,
                        attrs,
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: ext_id,
                    kind: "uses".to_string(),
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
    let src = workspace.join("src");
    if src.is_dir() {
        src
    } else {
        workspace.to_owned()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// File discovery
// ─────────────────────────────────────────────────────────────────────────────

fn collect_files(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| exts.contains(&x.to_str().unwrap_or("")))
                && !is_skip_path(e.path(), root)
        })
        .map(|e| e.into_path())
        .collect()
}

fn is_skip_path(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .map(|rel| {
            rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s == "node_modules"
                    || s == "dist"
                    || s == "target"
                    || s == "build"
                    || s == "out"
                    || s == ".venv"
                    || s == "__pycache__"
                    || s.starts_with('.')
                    || s.ends_with(".gen.ts")
                    || s.ends_with(".config.ts")
                    || s.ends_with(".config.js")
                    || s.ends_with(".min.js")
                    || s.ends_with(".min.ts")
                    || s.ends_with(".umd.js")
                    || s.ends_with(".bundle.js")
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
    for ext in &[".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".mts", ".cts"] {
        if let Some(stem) = last.strip_suffix(ext) {
            *last = stem.to_string();
            break;
        }
    }
    if parts.last().map(|s| s == "index").unwrap_or(false) {
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
        || spec.starts_with("@/")
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

fn extract_import_specifiers(root: &tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut specs = Vec::new();
    visit_imports(root, source, &mut specs);
    specs
}

fn visit_imports<'t>(node: &tree_sitter::Node<'t>, source: &[u8], specs: &mut Vec<String>) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();

    for child in &children {
        match child.kind() {
            // import 'module' / import { x } from 'module'
            "import_statement" => {
                if let Some(src) = import_source(child, source) {
                    specs.push(src);
                }
            }
            // export { x } from 'module'  /  export * from 'module'
            "export_statement" => {
                if let Some(src) = import_source(child, source) {
                    specs.push(src);
                }
                visit_imports(child, source, specs);
            }
            "call_expression" => {
                if let Some(src) = require_source(child, source) {
                    specs.push(src);
                } else {
                    visit_imports(child, source, specs);
                }
            }
            _ => visit_imports(child, source, specs),
        }
    }
}

/// Extract the module specifier string from an import or re-export statement.
fn import_source(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for child in children.iter().rev() {
        if child.kind() == "string"
            && let Ok(raw) = child.utf8_text(source)
        {
            let trimmed = raw.trim_matches(|c| c == '\'' || c == '"' || c == '`');
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Extract `require("./path")` specifier from a call_expression node.
fn require_source(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let fn_node = node.child_by_field_name("function")?;
    let fn_text = fn_node.utf8_text(source).ok()?;
    if fn_text != "require" {
        return None;
    }
    let args = node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "string"
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
    } else if let Some(rest) = specifier.strip_prefix("@/") {
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
    for ext in candidate_exts_order {
        candidates.push(normalized.join(format!("index.{ext}")));
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

// ─────────────────────────────────────────────────────────────────────────────
// Plugin struct
// ─────────────────────────────────────────────────────────────────────────────

/// The JavaScript language plugin (handles .js / .jsx / .mjs / .cjs).
pub struct JavascriptPlugin;

const JS_EXTS: &[&str] = &["js", "jsx", "mjs", "cjs"];

impl LanguagePlugin for JavascriptPlugin {
    fn name(&self) -> &str {
        "javascript"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        detect_with_marker(workspace, "package.json")
    }

    fn levels(&self) -> Vec<Level> {
        vec![ecmascript_level("files")]
    }

    fn analyze(&self, workspace: &Path, _level: &str, _input: &PluginInput) -> Result<Graph> {
        analyze_ecmascript(
            workspace,
            JS_EXTS,
            |ext| match ext {
                "js" | "jsx" | "mjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
                _ => None,
            },
            &["js", "jsx", "mjs", "cjs"],
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn file_to_mod_path_strips_ext_and_collapses_index() {
        let ws = Path::new("/proj");
        assert_eq!(
            file_to_mod_path(ws, Path::new("/proj/src/lib/utils.ts")).as_deref(),
            Some("src/lib/utils")
        );
        assert_eq!(
            file_to_mod_path(ws, Path::new("/proj/src/lib/index.ts")).as_deref(),
            Some("src/lib")
        );
    }

    #[test]
    fn external_package_extracts_top_level_and_scope() {
        assert_eq!(external_package("react").as_deref(), Some("react"));
        assert_eq!(external_package("lodash/fp").as_deref(), Some("lodash"));
        assert_eq!(
            external_package("@scope/pkg/sub").as_deref(),
            Some("@scope/pkg")
        );
        assert_eq!(external_package("./local"), None);
        assert_eq!(external_package("@/aliased"), None);
    }

    #[test]
    fn resolve_import_external_package_is_skipped() {
        let got = resolve_import(
            "react",
            Path::new("/proj/src/a.ts"),
            Path::new("/proj"),
            Path::new("/proj/src"),
            &HashMap::new(),
            &["ts", "tsx", "js", "jsx"],
        );
        assert_eq!(got, None, "bare package specifiers are not local imports");
    }

    #[test]
    fn find_source_root_prefers_existing_src_dir() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(find_source_root(tmp.path()), tmp.path());
        fs::create_dir(tmp.path().join("src")).unwrap();
        assert_eq!(find_source_root(tmp.path()), tmp.path().join("src"));
    }

    fn write_file(dir: &Path, rel: &str, contents: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }

    #[test]
    fn analyze_builds_file_graph_with_imports_and_externals() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_file(
            root,
            "src/a.ts",
            "import { greet } from \"./b\";\n\
             import React from \"react\";\n\
             export function helper() { return greet(); }\n",
        );
        write_file(
            root,
            "src/b.ts",
            "export function greet() { return \"hi\"; }\n",
        );

        // Use TS extensions so the tree-sitter-javascript parser (used here
        // via the shared helper) can still parse the TS syntax subset.
        let graph = analyze_ecmascript(
            root,
            &["ts"],
            |ext| match ext {
                "ts" => Some(tree_sitter_javascript::LANGUAGE.into()),
                _ => None,
            },
            &["ts", "tsx", "js", "jsx"],
        )
        .expect("analyze_ecmascript should succeed");

        let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
        let b_id = root.join("src/b.ts").to_string_lossy().into_owned();

        assert!(
            graph.nodes.iter().any(|n| n.id == a_id && n.kind == "file"),
            "a.ts node present"
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses"),
            "expected import edge a.ts → b.ts"
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| n.id == "ext:react" && n.kind == "external"),
            "external node for react"
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.source == a_id && e.target == "ext:react"),
            "external edge a.ts → react"
        );
    }

    #[test]
    fn ecmascript_level_has_expected_structure() {
        let level = ecmascript_level("files");
        assert_eq!(level.name, "files");
        assert!(level.edge_kinds.contains_key("uses"));
        let uses = &level.edge_kinds["uses"];
        assert!(uses.flow);
        assert!(level.node_attributes.contains_key("loc"));
        assert!(level.node_attributes.contains_key("visibility"));
        assert!(level.node_attributes.contains_key("external"));
        assert!(level.edge_attributes.is_empty());
        assert!(level.attribute_groups.is_empty());
    }
}
