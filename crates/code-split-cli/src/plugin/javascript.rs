use anyhow::Result;
use code_split_core::{
    GraphBuilder, NodeKind, PluginGraphs, StageTime,
    graph::{Edge, EdgeKind, Node, Visibility},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::logger;
use crate::plugin::finalize::finalize_file_graph;

pub fn run(workspace: &Path) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings = Vec::new();
    let mut builder = GraphBuilder::new();

    let t = logger::Timer::start("js/ts: scan + parse + build file graph");

    let source_root = find_source_root(workspace);
    let alias_root = source_root.clone(); // @/* → source_root/*
    let js_files = collect_js_files(&source_root);
    let file_index = build_file_index(workspace, &source_root, &js_files);

    for abs_path in &js_files {
        let _ = parse_and_add(abs_path, workspace, &alias_root, &file_index, &mut builder);
    }

    let n = builder.node_count();
    let detail = format!("{n} nodes from {} files", js_files.len());
    let ms = t.finish_quiet();
    timings.push(StageTime {
        stage: "js-ts".into(),
        ms,
        detail,
    });

    {
        let t = logger::Timer::start("complexity: cyclomatic / cognitive / halstead / MI / LOC");
        let annotated = match code_split_complexity::analyze_js(&source_root, &mut builder) {
            Ok(n) => n,
            Err(e) => {
                logger::info(&format!("complexity skipped: {e:#}"));
                0
            }
        };
        let detail = format!("{annotated} nodes annotated");
        let ms = t.finish_quiet();
        timings.push(StageTime {
            stage: "complexity".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting file graph");
    let files = finalize_file_graph(builder.build());
    let detail = format!("files={} edges={}", files.nodes.len(), files.edges.len());
    let ms = t.finish_quiet();
    timings.push(StageTime {
        stage: "projection".into(),
        ms,
        detail,
    });

    Ok((PluginGraphs { files }, timings))
}

// ---------------------------------------------------------------------------
// Source root detection
// ---------------------------------------------------------------------------

/// Returns the `src/` directory if present, otherwise the workspace root.
fn find_source_root(workspace: &Path) -> PathBuf {
    let src = workspace.join("src");
    if src.is_dir() {
        src
    } else {
        workspace.to_owned()
    }
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

fn collect_js_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| matches!(x.to_str(), Some("ts" | "tsx" | "js" | "jsx")))
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
                    || s.ends_with(".cjs")
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Module path helpers
// ---------------------------------------------------------------------------

/// `src/lib/utils.ts` → `src/lib/utils`
/// `src/lib/index.ts` → `src/lib`
fn file_to_mod_path(workspace: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    let last = parts.last_mut()?;
    for ext in &[".tsx", ".ts", ".jsx", ".js"] {
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

/// Build a map: module_path → abs_path for all js/ts files.
fn build_file_index(
    workspace: &Path,
    _source_root: &Path,
    files: &[PathBuf],
) -> HashMap<String, PathBuf> {
    files
        .iter()
        .filter_map(|p| file_to_mod_path(workspace, p).map(|m| (m, p.clone())))
        .collect()
}

// ---------------------------------------------------------------------------
// Per-file parsing
// ---------------------------------------------------------------------------

fn parse_and_add(
    abs_path: &Path,
    workspace: &Path,
    alias_root: &Path,
    file_index: &HashMap<String, PathBuf>,
    builder: &mut GraphBuilder,
) -> Result<()> {
    let source = std::fs::read(abs_path)?;
    let ext = abs_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let language = match ext {
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "jsx" | "js" => tree_sitter_javascript::LANGUAGE.into(),
        _ => return Ok(()),
    };

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&language)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed: {}", abs_path.display()))?;

    let loc = source.iter().filter(|&&b| b == b'\n').count() as u32 + 1;
    let file_id = format!("file:{}", abs_path.to_string_lossy());

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
        version: None,
        visibility: Some(Visibility::Public),
        loc: Some(loc),
        line: None,
        item_count: None,
        method_count: None,
        complexity: None,
        cycle_kind: None,
    });

    let specifiers = extract_import_specifiers(&tree.root_node(), &source);

    for spec in &specifiers {
        if let Some(target) = resolve_import(spec, abs_path, workspace, alias_root, file_index) {
            let target_id = format!("file:{}", target.to_string_lossy());
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
        } else if let Some(pkg) = external_package(spec) {
            // Bare specifier that does not resolve to a project file → an
            // external (npm) dependency, recorded at depth 1.
            let ext_id = format!("ext:{pkg}");
            builder.add_node(Node {
                id: ext_id.clone(),
                kind: NodeKind::External,
                name: pkg,
                path: String::new(),
                parent: None,
                external: Some(true),
                version: None,
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
    }

    Ok(())
}

/// The package name for a bare (non-relative, non-alias) import specifier:
/// `react` → `react`, `lodash/fp` → `lodash`, `@scope/pkg/sub` → `@scope/pkg`.
/// Returns `None` for relative (`./`, `../`) and `@/` alias specifiers.
fn external_package(spec: &str) -> Option<String> {
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
        // scoped package: keep `@scope/name`
        match it.next() {
            Some(second) => Some(format!("{first}/{second}")),
            None => Some(first.to_string()),
        }
    } else {
        Some(first.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tree-sitter extraction (import / require specifiers only)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Import resolution
// ---------------------------------------------------------------------------

fn resolve_import(
    specifier: &str,
    from_file: &Path,
    workspace: &Path,
    alias_root: &Path,
    file_index: &HashMap<String, PathBuf>,
) -> Option<PathBuf> {
    let base_path: PathBuf = if specifier.starts_with("./") || specifier.starts_with("../") {
        from_file.parent()?.join(specifier)
    } else if let Some(rest) = specifier.strip_prefix("@/") {
        alias_root.join(rest)
    } else {
        // External package — not a local import.
        return None;
    };

    let normalized = normalize_path(&base_path);

    let candidates = [
        normalized.with_extension("ts"),
        normalized.with_extension("tsx"),
        normalized.with_extension("js"),
        normalized.with_extension("jsx"),
        normalized.join("index.ts"),
        normalized.join("index.tsx"),
        normalized.join("index.js"),
        normalized.join("index.jsx"),
    ];

    for candidate in &candidates {
        let mod_path = file_to_mod_path(workspace, candidate)?;
        if file_index.contains_key(&mod_path) {
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
mod tests {
    use super::*;
    use code_split_core::graph::Graph;
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

    // ── end-to-end: a tiny TypeScript project through run() ──────────────────

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
        write(
            root,
            "src/a.ts",
            "import { greet } from \"./b\";\n\
             import React from \"react\";\n\
             export function helper() { return greet(); }\n",
        );
        write(
            root,
            "src/b.ts",
            "export function greet() { return \"hi\"; }\n",
        );

        let (graphs, _timings) = run(root).expect("js/ts plugin runs");
        let g = &graphs.files;

        assert!(
            g.nodes
                .iter()
                .all(|n| matches!(n.kind, NodeKind::File | NodeKind::External)),
            "files graph holds only File/External nodes"
        );

        let a_id = format!("file:{}", root.join("src/a.ts").to_string_lossy());
        let b_id = format!("file:{}", root.join("src/b.ts").to_string_lossy());
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == a_id && e.to == b_id && e.kind == EdgeKind::Uses),
            "expected import edge a.ts → b.ts"
        );

        assert!(has_node(g, "ext:react"), "external node for react");
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == a_id && e.to == "ext:react" && e.external == Some(true)),
            "external edge a.ts → react flagged external"
        );
    }
}
