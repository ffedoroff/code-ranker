//! Derived path fields for a node, shared by the metric engine ([`crate::registry`])
//! and the check engine ([`crate::checks`]) so both expose the same `path` /
//! `name` / `stem` / `ext` / `dir` variables to CEL.

use code_ranker_plugin_api::{attrs::AttrValue, node::Node};

/// The node's repo-relative path: its `path` string attribute when present, else
/// its id with the `{target}/` analysis prefix stripped.
pub(crate) fn node_path(node: &Node) -> String {
    if let Some(AttrValue::Str(p)) = node.attrs.get("path") {
        return p.clone();
    }
    node.id
        .strip_prefix("{target}/")
        .unwrap_or(&node.id)
        .to_string()
}

/// The basename, stem, extension and directory of a path.
pub(crate) struct PathParts {
    /// Final path segment (basename), e.g. `handler.rs`.
    pub name: String,
    /// Basename without its final extension, e.g. `handler` (`handler_tests` for
    /// `handler_tests.rs` — only the last `.ext` is removed).
    pub stem: String,
    /// Final extension without the dot, e.g. `rs` (empty if none).
    pub ext: String,
    /// Everything before the basename, e.g. `crates/a/src` (empty at the root).
    pub dir: String,
}

pub(crate) fn split_path(path: &str) -> PathParts {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let dir = match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    };
    let (stem, ext) = match name.rfind('.') {
        // A leading dot (dotfile) is not an extension separator.
        Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
        _ => (name.clone(), String::new()),
    };
    PathParts {
        name,
        stem,
        ext,
        dir,
    }
}

/// The five derived path variables (`path`/`name`/`stem`/`ext`/`dir`) for a node,
/// ready to bind into a CEL context.
pub(crate) fn path_fields(node: &Node) -> [(&'static str, String); 5] {
    let path = node_path(node);
    let parts = split_path(&path);
    [
        ("path", path),
        ("name", parts.name),
        ("stem", parts.stem),
        ("ext", parts.ext),
        ("dir", parts.dir),
    ]
}
