use anyhow::Result;
use code_split_core::{
    EdgeKind, GraphBuilder, NodeKind, PluginGraphs, StageTime,
    graph::{Edge, Node, Visibility},
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::logger;

pub fn run(
    workspace: &Path,
    _local_only: bool,
    _want_functions: bool,
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings = Vec::new();
    let mut builder = GraphBuilder::new();

    let t = logger::Timer::start("js/ts: scan + parse + build graph");

    let source_root = find_source_root(workspace);
    let alias_root = source_root.clone(); // @/* → source_root/*
    let js_files = collect_js_files(&source_root);
    let file_index = build_file_index(workspace, &source_root, &js_files);

    for abs_path in &js_files {
        add_dir_ancestors(abs_path, workspace, &source_root, &file_index, &mut builder);
        let _ = parse_and_add(abs_path, workspace, &alias_root, &file_index, &mut builder);
    }

    let n = builder.node_count();
    let detail = format!("{n} nodes from {} files", js_files.len());
    let ms = t.finish_with(&detail);
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
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "complexity".into(),
            ms,
            detail,
        });
    }

    {
        let t = logger::Timer::start("sema: heuristic call graph (tree-sitter)");
        let name_index = build_fn_name_index(&builder);
        let mut call_count = 0usize;
        for abs_path in &js_files {
            let ext = abs_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            match extract_calls_js(abs_path, workspace, ext, &name_index) {
                Ok(calls) => {
                    call_count += calls.len();
                    for (from, to) in calls {
                        builder.add_edge(Edge {
                            from,
                            to,
                            kind: EdgeKind::Calls,
                            unresolved: None,
                            external: None,
                            visibility: None,
                        });
                    }
                }
                Err(e) => logger::info(&format!("sema: skipped {}: {e:#}", abs_path.display())),
            }
        }
        let detail = format!("{call_count} call edges");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "sema".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting graphs (modules / files / functions)");
    let full = builder.build();

    let modules = full.project(&[NodeKind::Module], &[EdgeKind::Contains, EdgeKind::Uses]);
    let files = full.project(
        &[NodeKind::Module, NodeKind::File],
        &[EdgeKind::Contains, EdgeKind::Uses],
    );
    let functions = full.project(
        &[
            NodeKind::Module,
            NodeKind::File,
            NodeKind::Impl,
            NodeKind::Fn,
            NodeKind::Method,
        ],
        &[EdgeKind::Contains, EdgeKind::Uses, EdgeKind::Calls],
    );

    let detail = format!(
        "modules={} files={} functions={}",
        modules.nodes.len(),
        files.nodes.len(),
        functions.nodes.len(),
    );
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "projection".into(),
        ms,
        detail,
    });

    Ok((
        PluginGraphs {
            modules,
            files,
            functions,
        },
        timings,
    ))
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
    // Strip extension
    for ext in &[".tsx", ".ts", ".jsx", ".js"] {
        if let Some(stem) = last.strip_suffix(ext) {
            *last = stem.to_string();
            break;
        }
    }
    // index → collapse into directory
    if parts.last().map(|s| s == "index").unwrap_or(false) {
        parts.pop();
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn mod_id(mod_path: &str) -> String {
    format!("mod:{}", mod_path.replace('/', "::"))
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
// Directory / module ancestor nodes
// ---------------------------------------------------------------------------

/// Emit Module nodes for every ancestor directory of a file.
/// A directory is emitted as a Module node when it contains JS/TS files,
/// regardless of whether it has an index file (supports CommonJS projects).
fn add_dir_ancestors(
    file_path: &Path,
    workspace: &Path,
    _source_root: &Path,
    _file_index: &HashMap<String, PathBuf>,
    builder: &mut GraphBuilder,
) {
    let Some(mod_path) = file_to_mod_path(workspace, file_path) else {
        return;
    };
    let segments: Vec<&str> = mod_path.split('/').collect();

    // Emit a Module node for each ancestor directory (all prefixes that are real dirs).
    for i in 1..segments.len() {
        let prefix = segments[..i].join("/");
        let dir_path = workspace.join(prefix.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !dir_path.is_dir() {
            continue;
        }

        let id = mod_id(&prefix);
        let parent_id = (i > 1).then(|| mod_id(&segments[..i - 1].join("/")));

        builder.add_node(Node {
            id: id.clone(),
            kind: NodeKind::Module,
            name: segments[i - 1].to_string(),
            path: dir_path.to_string_lossy().into_owned(),
            parent: parent_id.clone(),
            external: Some(false),
            visibility: Some(Visibility::Public),
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });

        if let Some(pid) = parent_id {
            builder.add_edge(Edge {
                from: pid,
                to: id,
                kind: EdgeKind::Contains,
                unresolved: None,
                external: None,
                visibility: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Per-file parsing
// ---------------------------------------------------------------------------

struct ExtractedFn {
    name: String,
    class_name: Option<String>,
    line: u32,
    end_line: u32,
}

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

    let Some(mod_path) = file_to_mod_path(workspace, abs_path) else {
        return Ok(());
    };
    let segments: Vec<&str> = mod_path.split('/').collect();

    // Parent: the closest ancestor directory that is a module (has index file),
    // or just the immediate parent directory node.
    let parent_id = find_parent_mod(&segments, workspace);

    builder.add_node(Node {
        id: file_id.clone(),
        kind: NodeKind::File,
        name: abs_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: abs_path.to_string_lossy().into_owned(),
        parent: parent_id.clone(),
        external: Some(false),
        visibility: Some(Visibility::Public),
        loc: Some(loc),
        line: None,
        item_count: None,
        method_count: None,
        complexity: None,
        cycle_kind: None,
    });

    if let Some(pid) = &parent_id {
        builder.add_edge(Edge {
            from: pid.clone(),
            to: file_id.clone(),
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
        });
    }

    let root = tree.root_node();
    let (fns, imports) = extract_tree_info(&root, &source);

    // Import edges
    for imp in &imports {
        if let Some(target) = resolve_import(imp, abs_path, workspace, alias_root, file_index) {
            let target_mod = file_to_mod_path(workspace, &target).unwrap_or_default();
            let is_index = target.file_stem().is_some_and(|s| s == "index");
            let target_id = if is_index {
                mod_id(&target_mod)
            } else {
                format!("file:{}", target.to_string_lossy())
            };
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

    // Class nodes
    let mut seen_classes: HashSet<String> = HashSet::new();
    for f in &fns {
        if let Some(cls) = &f.class_name
            && seen_classes.insert(cls.clone())
        {
            let cls_id = format!("impl:{}::{}", mod_path.replace('/', "::"), cls);
            builder.add_node(Node {
                id: cls_id.clone(),
                kind: NodeKind::Impl,
                name: cls.clone(),
                path: abs_path.to_string_lossy().into_owned(),
                parent: Some(file_id.clone()),
                external: Some(false),
                visibility: Some(Visibility::Public),
                loc: None,
                line: None,
                item_count: None,
                method_count: None,
                complexity: None,
                cycle_kind: None,
            });
            builder.add_edge(Edge {
                from: file_id.clone(),
                to: cls_id,
                kind: EdgeKind::Contains,
                unresolved: None,
                external: None,
                visibility: None,
            });
        }
    }

    // Function / method nodes
    for f in &fns {
        let (fn_id, fn_kind, fn_parent) = if let Some(cls) = &f.class_name {
            let cls_id = format!("impl:{}::{}", mod_path.replace('/', "::"), cls);
            (
                format!(
                    "method:{}::{}::{}",
                    mod_path.replace('/', "::"),
                    cls,
                    f.name
                ),
                NodeKind::Method,
                cls_id,
            )
        } else {
            (
                format!("fn:{}::{}", mod_path.replace('/', "::"), f.name),
                NodeKind::Fn,
                file_id.clone(),
            )
        };

        builder.add_node(Node {
            id: fn_id.clone(),
            kind: fn_kind,
            name: f.name.clone(),
            path: abs_path.to_string_lossy().into_owned(),
            parent: Some(fn_parent.clone()),
            external: Some(false),
            visibility: Some(Visibility::Public),
            loc: Some(f.end_line.saturating_sub(f.line) + 1),
            line: Some(f.line),
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });

        builder.add_edge(Edge {
            from: fn_parent,
            to: fn_id,
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
        });
    }

    Ok(())
}

fn find_parent_mod(segments: &[&str], workspace: &Path) -> Option<String> {
    for i in (1..segments.len()).rev() {
        let prefix = segments[..i].join("/");
        let dir = workspace.join(prefix.replace('/', std::path::MAIN_SEPARATOR_STR));
        if dir.is_dir() {
            return Some(mod_id(&prefix));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tree-sitter extraction
// ---------------------------------------------------------------------------

fn extract_tree_info(root: &tree_sitter::Node, source: &[u8]) -> (Vec<ExtractedFn>, Vec<String>) {
    let mut fns = Vec::new();
    let mut imports = Vec::new();
    visit(root, source, None, false, &mut fns, &mut imports);
    (fns, imports)
}

fn visit<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    class_ctx: Option<&str>,
    inside_fn: bool,
    fns: &mut Vec<ExtractedFn>,
    imports: &mut Vec<String>,
) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();

    for child in &children {
        match child.kind() {
            // import 'module' / import { x } from 'module'
            "import_statement" => {
                if let Some(src) = import_source(child, source) {
                    imports.push(src);
                }
            }
            // export { x } from 'module'  /  export * from 'module'
            "export_statement" => {
                if let Some(src) = import_source(child, source) {
                    imports.push(src);
                }
                if !inside_fn {
                    visit(child, source, class_ctx, false, fns, imports);
                }
            }
            // function foo() {}
            "function_declaration" | "function" => {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    fns.push(ExtractedFn {
                        name: name.to_string(),
                        class_name: class_ctx.map(str::to_string),
                        line: child.start_position().row as u32 + 1,
                        end_line: child.end_position().row as u32 + 1,
                    });
                }
                // Don't recurse into function bodies for nested functions
            }
            // class Foo {}
            "class_declaration" | "class" => {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    let cls = name.to_string();
                    if let Some(body) = child.child_by_field_name("body") {
                        visit(&body, source, Some(&cls), false, fns, imports);
                    }
                }
            }
            // method_definition inside class body
            "method_definition" => {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    fns.push(ExtractedFn {
                        name: name.to_string(),
                        class_name: class_ctx.map(str::to_string),
                        line: child.start_position().row as u32 + 1,
                        end_line: child.end_position().row as u32 + 1,
                    });
                }
            }
            // const foo = () => {} / const foo = require('./x')
            "lexical_declaration" | "variable_declaration" => {
                if !inside_fn {
                    extract_arrow_fns(child, source, class_ctx, fns);
                    extract_requires(child, source, imports);
                }
            }
            // expression_statement at top level: foo = () => {}  or  require('./x')
            "expression_statement" => {
                if !inside_fn {
                    extract_arrow_fns(child, source, class_ctx, fns);
                    extract_requires(child, source, imports);
                }
            }
            _ => {
                if !inside_fn {
                    visit(child, source, class_ctx, false, fns, imports);
                }
            }
        }
    }
}

/// Extract named arrow functions and function expressions from variable declarations.
/// `const foo = () => {}` → Fn named "foo"
fn extract_arrow_fns<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    class_ctx: Option<&str>,
    fns: &mut Vec<ExtractedFn>,
) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();
    for child in &children {
        if child.kind() == "variable_declarator" {
            let name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(str::to_string);
            let value = child.child_by_field_name("value");
            if let (Some(name), Some(val)) = (name, value)
                && matches!(
                    val.kind(),
                    "arrow_function" | "function" | "function_expression"
                )
            {
                // Only capture if name looks like a component or function (starts uppercase or lowercase alpha)
                if name.chars().next().is_some_and(|c| c.is_alphabetic()) {
                    fns.push(ExtractedFn {
                        name,
                        class_name: class_ctx.map(str::to_string),
                        line: child.start_position().row as u32 + 1,
                        end_line: child.end_position().row as u32 + 1,
                    });
                }
            }
        } else {
            extract_arrow_fns(child, source, class_ctx, fns);
        }
    }
}

/// Extract the module specifier string from an import or re-export statement.
fn import_source(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    // The source/from string is usually the last string literal child
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

/// Recursively find all `require("./path")` calls in a subtree and push specifiers.
fn extract_requires<'t>(node: &tree_sitter::Node<'t>, source: &[u8], imports: &mut Vec<String>) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();
    for child in &children {
        if child.kind() == "call_expression"
            && let Some(src) = require_source(child, source)
        {
            imports.push(src);
            continue; // don't recurse into require's arguments
        }
        extract_requires(child, source, imports);
    }
}

/// Extract `require("./path")` specifier from a call_expression node.
fn require_source(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    // node is call_expression; check function is "require"
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
    // Classify
    let base_path: PathBuf = if specifier.starts_with("./") || specifier.starts_with("../") {
        // Relative
        from_file.parent()?.join(specifier)
    } else if let Some(rest) = specifier.strip_prefix("@/") {
        // Path alias @/ → source_root
        alias_root.join(rest)
    } else {
        // External package — skip
        return None;
    };

    // Normalize (canonicalize resolves .. etc. but fails on non-existent paths)
    let normalized = normalize_path(&base_path);

    // Try extensions in order
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

// ---------------------------------------------------------------------------
// Heuristic sema: call graph via tree-sitter
// ---------------------------------------------------------------------------

fn build_fn_name_index(builder: &GraphBuilder) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for node in builder.nodes() {
        if matches!(node.kind, NodeKind::Fn | NodeKind::Method) {
            index
                .entry(node.name.clone())
                .or_default()
                .push(node.id.clone());
        }
    }
    index
}

fn extract_calls_js(
    abs_path: &Path,
    workspace: &Path,
    ext: &str,
    name_index: &HashMap<String, Vec<String>>,
) -> Result<Vec<(String, String)>> {
    let source = std::fs::read(abs_path)?;
    let language: tree_sitter::Language = match ext {
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "jsx" | "js" => tree_sitter_javascript::LANGUAGE.into(),
        _ => return Ok(vec![]),
    };

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&language)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed"))?;

    let Some(mod_path) = file_to_mod_path(workspace, abs_path) else {
        return Ok(vec![]);
    };

    let mut calls: HashSet<(String, String)> = HashSet::new();
    visit_calls_js(
        &tree.root_node(),
        &source,
        &mod_path,
        None,
        None,
        name_index,
        &mut calls,
    );
    Ok(calls.into_iter().collect())
}

fn visit_calls_js<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    mod_path: &str,
    class_ctx: Option<&str>,
    current_fn_id: Option<&str>,
    name_index: &HashMap<String, Vec<String>>,
    calls: &mut HashSet<(String, String)>,
) {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
            {
                let fn_id = format!("fn:{}::{}", mod_path.replace('/', "::"), name);
                if let Some(body) = node.child_by_field_name("body") {
                    visit_calls_js(
                        &body,
                        source,
                        mod_path,
                        class_ctx,
                        Some(&fn_id),
                        name_index,
                        calls,
                    );
                }
            }
        }
        "method_definition" => {
            if let (Some(name), Some(cls)) = (
                node.child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok()),
                class_ctx,
            ) {
                let fn_id = format!("method:{}::{}::{}", mod_path.replace('/', "::"), cls, name);
                if let Some(body) = node.child_by_field_name("body") {
                    visit_calls_js(
                        &body,
                        source,
                        mod_path,
                        class_ctx,
                        Some(&fn_id),
                        name_index,
                        calls,
                    );
                }
            }
        }
        "class_declaration" | "class" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                && let Some(body) = node.child_by_field_name("body")
            {
                visit_calls_js(
                    &body,
                    source,
                    mod_path,
                    Some(name),
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
        "variable_declarator" => {
            let name_node = node.child_by_field_name("name");
            let val_node = node.child_by_field_name("value");
            if let (Some(name_n), Some(val)) = (name_node, val_node)
                && let Ok(name) = name_n.utf8_text(source)
                && matches!(
                    val.kind(),
                    "arrow_function" | "function" | "function_expression"
                )
            {
                let fn_id = format!("fn:{}::{}", mod_path.replace('/', "::"), name);
                // Recurse into the function value with the named context
                visit_calls_js(
                    &val,
                    source,
                    mod_path,
                    class_ctx,
                    Some(&fn_id),
                    name_index,
                    calls,
                );
                return;
            }
            // Non-function declarator: recurse with current context
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                visit_calls_js(
                    &child,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
        "arrow_function" | "function_expression" | "function" => {
            // Anonymous function passed as callback — keep parent fn context
            if let Some(body) = node.child_by_field_name("body") {
                visit_calls_js(
                    &body,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            } else {
                // No named body field; recurse into children
                let mut c = node.walk();
                for child in node.children(&mut c).collect::<Vec<_>>() {
                    visit_calls_js(
                        &child,
                        source,
                        mod_path,
                        class_ctx,
                        current_fn_id,
                        name_index,
                        calls,
                    );
                }
            }
        }
        "call_expression" => {
            if let Some(from_id) = current_fn_id
                && let Some(fn_node) = node.child_by_field_name("function")
            {
                let callee = match fn_node.kind() {
                    "identifier" => fn_node.utf8_text(source).ok().map(str::to_string),
                    "member_expression" => fn_node
                        .child_by_field_name("property")
                        .and_then(|p| p.utf8_text(source).ok())
                        .map(str::to_string),
                    _ => None,
                };
                if let Some(callee) = callee {
                    for to_id in name_index.get(&callee).into_iter().flatten() {
                        if to_id.as_str() != from_id {
                            calls.insert((from_id.to_string(), to_id.clone()));
                        }
                    }
                }
            }
            // Recurse into call arguments to catch nested calls
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                visit_calls_js(
                    &child,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                visit_calls_js(
                    &child,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
    }
}

/// Cheap path normalization without filesystem access: resolve `..` and `.` components.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
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

    // ── pure helpers ────────────────────────────────────────────────────────

    #[test]
    fn mod_id_slashes_become_double_colons() {
        assert_eq!(mod_id("a/b/c"), "mod:a::b::c");
        assert_eq!(mod_id("x"), "mod:x");
    }

    #[test]
    fn file_to_mod_path_strips_extension_and_collapses_index() {
        let ws = Path::new("/proj");
        let cases: Vec<(&str, Option<&str>)> = vec![
            ("/proj/src/lib/utils.ts", Some("src/lib/utils")),
            ("/proj/src/components/Button.tsx", Some("src/components/Button")),
            ("/proj/src/index.ts", Some("src")), // index → collapse into dir
            ("/proj/index.ts", None),            // root index → empty → None
        ];
        for (path, expected) in cases {
            assert_eq!(
                file_to_mod_path(ws, Path::new(path)).as_deref(),
                expected,
                "for {path}"
            );
        }
    }

    #[test]
    fn is_skip_path_skips_vendor_build_and_generated() {
        let ws = Path::new("/proj");
        for p in [
            "/proj/node_modules/x.js",
            "/proj/dist/x.js",
            "/proj/build/x.js",
            "/proj/.git/x.ts",
            "/proj/a.min.js",
            "/proj/vite.config.ts",
            "/proj/legacy.cjs",
        ] {
            assert!(is_skip_path(Path::new(p), ws), "should skip {p}");
        }
        assert!(
            !is_skip_path(Path::new("/proj/src/app.ts"), ws),
            "normal source is not skipped"
        );
        assert!(
            !is_skip_path(Path::new("/other/x.ts"), ws),
            "path outside the workspace is not skipped"
        );
    }

    #[test]
    fn normalize_path_resolves_parent_and_current() {
        assert_eq!(normalize_path(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize_path(Path::new("/a/./b")), PathBuf::from("/a/b"));
        assert_eq!(
            normalize_path(Path::new("/a/b/c/../../d")),
            PathBuf::from("/a/d")
        );
    }

    #[test]
    fn resolve_import_relative_resolves_to_indexed_file() {
        let index: HashMap<String, PathBuf> =
            HashMap::from([("src/b".to_string(), PathBuf::from("/proj/src/b.ts"))]);
        let got = resolve_import(
            "./b",
            Path::new("/proj/src/a.ts"),
            Path::new("/proj"),
            Path::new("/proj/src"),
            &index,
        );
        assert_eq!(got, Some(PathBuf::from("/proj/src/b.ts")));
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
        // No src/ yet → the workspace root is the source root.
        assert_eq!(find_source_root(tmp.path()), tmp.path());
        // Once src/ exists, it wins.
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

    fn has_edge(g: &Graph, from: &str, to: &str, kind: EdgeKind) -> bool {
        g.edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.kind == kind)
    }

    #[test]
    fn run_builds_graphs_for_a_typescript_project() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "src/a.ts",
            "import { greet } from \"./b\";\n\
             export class Foo {\n\
             \x20 bar() { return greet(); }\n\
             }\n\
             export function helper() { return 1; }\n",
        );
        write(
            root,
            "src/b.ts",
            "export function greet() { return \"hi\"; }\n",
        );

        let (graphs, _timings) = run(root, false, true).expect("js/ts plugin runs");

        // modules graph: the `src` directory becomes a module node.
        assert!(has_node(&graphs.modules, "mod:src"), "src module node");

        // functions graph: class, method, and functions are extracted.
        let f = &graphs.functions;
        assert!(has_node(f, "impl:src::a::Foo"), "class Foo");
        assert!(has_node(f, "method:src::a::Foo::bar"), "method Foo.bar");
        assert!(has_node(f, "fn:src::a::helper"), "function helper");
        assert!(has_node(f, "fn:src::b::greet"), "function greet");

        // heuristic call edge: Foo.bar() → greet().
        assert!(
            has_edge(
                f,
                "method:src::a::Foo::bar",
                "fn:src::b::greet",
                EdgeKind::Calls
            ),
            "expected a call edge bar→greet"
        );

        // import edge a.ts → b.ts — a `uses` edge between two file nodes.
        let uses_between_files = f.edges.iter().any(|e| {
            e.kind == EdgeKind::Uses && e.from.starts_with("file:") && e.to.starts_with("file:")
        });
        assert!(uses_between_files, "expected an import edge between files");
    }
}
