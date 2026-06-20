//! ECMAScript import extraction + specifier resolution.
//!
//! The tree-sitter walk that pulls `import` / re-export / `require(...)`
//! specifiers out of a parsed source, plus the resolver that maps a specifier
//! back to a concrete workspace file. Split out of [`super`] (the structure
//! builder) as a cohesive, behavior-identical submodule. It owns the source-root
//! / module-path data ([`MODULE`] + [`file_to_mod_path`], which the resolver
//! needs) and reads the `[structure]` / `[fields]` node-kind vocabulary from the
//! ECMAScript leaf config (`super::super::cfg::CONFIG`) — so it depends only on
//! `cfg` downward and never back up on `super` (keeping the module graph acyclic).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Source-root + module-path DATA, resolved once from `ecmascript/config.toml`.
/// The detection LOGIC stays in Rust; the names it keys on are data. `source_dirs`
/// are the workspace subfolders `find_source_root` prefers; `module_strip_exts`
/// are the extensions `file_to_mod_path` strips (order matters — first wins);
/// `index_file` is the implicit module stem collapsed into its parent dir.
pub(super) struct ModuleLists {
    // Read by `file_to_mod_path` below, the parent `structure` module, and its
    // tests — all within the ECMAScript module, so `pub(super)` throughout.
    pub(super) source_dirs: Vec<String>,
    pub(super) strip_exts: Vec<String>,
    pub(super) index_file: String,
    pub(super) alias_prefix: String,
}

pub(super) static MODULE: LazyLock<ModuleLists> = LazyLock::new(|| ModuleLists {
    source_dirs: crate::config::string_list(&super::super::cfg::CONFIG, "source_dirs"),
    strip_exts: crate::config::string_list(&super::super::cfg::CONFIG, "module_strip_exts"),
    index_file: super::super::cfg::CONFIG
        .get("index_file")
        .and_then(|v| v.as_str())
        .expect("ecmascript/config.toml `index_file`")
        .to_string(),
    alias_prefix: super::super::cfg::CONFIG
        .get("alias_prefix")
        .and_then(|v| v.as_str())
        .expect("ecmascript/config.toml `alias_prefix`")
        .to_string(),
});

/// Map a workspace file path to its module path: strip the workspace prefix and
/// the configured module extensions, and collapse a trailing `index` file into
/// its directory. Returns `None` for a path outside the workspace or one that
/// collapses to nothing.
pub(super) fn file_to_mod_path(workspace: &Path, path: &Path) -> Option<String> {
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

// ─────────────────────────────────────────────────────────────────────────────
// Tree-sitter extraction (import / require specifiers)
// ─────────────────────────────────────────────────────────────────────────────

/// The import/module-graph tree-sitter NODE-KIND strings the walk keys on,
/// resolved once from `ecmascript/config.toml`'s `[structure]` table. The walk
/// LOGIC stays in Rust; *which* node kinds it matches is data.
pub(super) struct StructureKinds {
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
        let s = crate::config::string_table(&super::super::cfg::CONFIG, "structure");
        let get = |k: &str| s.get(k).cloned().expect("[structure] key");
        let f = crate::config::string_table(&super::super::cfg::CONFIG, "fields");
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
pub(super) fn extract_import_specifiers(
    root: &tree_sitter::Node,
    source: &[u8],
) -> Vec<(String, u32)> {
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

pub(super) fn resolve_import(
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
pub(super) fn normalize_path(path: &Path) -> PathBuf {
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
