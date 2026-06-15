//! Leaf module of the `module_graph` cluster: the structural items shared by
//! both `walk` (Phase A) and `resolve` (Phase B), plus their small helpers.
//! Extracted here so the children depend on a leaf (`super::shared`) instead of
//! the parent (`super`), keeping the cluster's module-dependency graph acyclic.
//! Pure code movement: no logic change. Depends only on `crate::internal`, std,
//! and `cargo_metadata` — never on `super`, `walk`, or `resolve`.

use crate::internal::{NodeId, Visibility};
use cargo_metadata::{Package, Target};
use std::collections::HashMap;

/// Human-readable owning-crate (compilation unit) label for a target. A package
/// can produce several crates — a library plus one or more binaries — so the
/// label is per-target: the library uses the package name, binaries get a
/// `(bin …)` suffix that keeps the package name as a prefix (globally unique
/// among workspace members, where package names are unique).
pub(super) fn crate_label(pkg: &Package, target: &Target) -> String {
    let pkg_name = pkg.name.to_string();
    if is_lib_target(target) {
        pkg_name
    } else if target.name == pkg_name {
        format!("{pkg_name} (bin)")
    } else {
        format!("{pkg_name} (bin {})", target.name)
    }
}

/// A target addressable by name from another crate (lib / proc-macro), as
/// opposed to a `bin` (which cannot be `use`d by name).
pub(super) fn is_lib_target(target: &Target) -> bool {
    target.kind.iter().any(|k| {
        matches!(
            k.as_str(),
            "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro"
        )
    })
}

#[derive(Debug)]
pub(super) struct PendingUse {
    pub(super) from_mod_id: NodeId,
    pub(super) current_path: Vec<String>,
    pub(super) use_path: Vec<String>,
    pub(super) visibility: Visibility,
    /// `true` for a crate-qualified path captured from an expression/type
    /// (`other_crate::item`) rather than a `use` statement.
    pub(super) bare: bool,
    /// `true` when this came from a glob `use` (`use path::*`).
    pub(super) glob: bool,
    /// 1-based line of the originating `use` statement; `None` for bare paths
    /// (an expression/type reference has no single statement to point at).
    pub(super) line: Option<u32>,
}

pub(super) fn is_reexport(v: &Visibility) -> bool {
    !matches!(v, Visibility::Private)
}

/// Per-module re-export table: module path → list of `(exported_symbol,
/// source_use_path)` captured from `pub use` statements. Lets resolution follow
/// `crate::Item` / `super::Item` to the file that *defines* `Item` instead of
/// anchoring on the (facade) module that merely re-exports it.
pub(super) type ReexportMap = HashMap<Vec<String>, Vec<(String, Vec<String>)>>;

/// Depth guard for following re-export chains (`pub use a::X` → `pub use b::X` …).
pub(super) const MAX_REEXPORT_DEPTH: usize = 8;

/// A foreign workspace crate's library, for submodule-precise cross-crate `use`
/// resolution: its module index plus its `pub use` re-export table, so
/// `other_crate::Symbol` resolves to the file that *defines* `Symbol` (following
/// the crate's `pub use` chain) rather than collapsing onto its crate root.
#[derive(Default)]
pub(super) struct ForeignLib {
    pub(super) index: HashMap<Vec<String>, NodeId>,
    pub(super) reexports: ReexportMap,
}

pub(super) fn build_reexports(pending: &[PendingUse]) -> ReexportMap {
    let mut map: ReexportMap = HashMap::new();
    for pu in pending {
        if pu.bare || !is_reexport(&pu.visibility) {
            continue;
        }
        if let Some(sym) = pu.use_path.last() {
            map.entry(pu.current_path.clone())
                .or_default()
                .push((sym.clone(), pu.use_path.clone()));
        }
    }
    map
}

pub(super) fn target_kind_label(target: &Target) -> &str {
    target
        .kind
        .iter()
        .map(String::as_str)
        .find(|k| {
            matches!(
                *k,
                "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro" | "bin"
            )
        })
        .unwrap_or("?")
}

/// A target's node-id namespace. A package can expose several targets that share
/// a name (e.g. a lib `bat` and a bin `bat`); keying module ids on the name alone
/// collapses their roots into one node, so `crate::X` in the lib mis-resolves to
/// the bin's `main.rs` (a library cannot depend on a binary). Disambiguate by the
/// target kind so each target gets its own module tree.
fn target_ns(pkg_id_repr: &str, target_kind: &str, target_name: &str) -> String {
    format!("mod:{pkg_id_repr}::{target_kind}:{target_name}")
}

pub(super) fn module_node_id(
    pkg_id_repr: &str,
    target_kind: &str,
    target_name: &str,
    path: &[String],
) -> String {
    let ns = target_ns(pkg_id_repr, target_kind, target_name);
    if path.is_empty() {
        ns
    } else {
        format!("{ns}::{}", path.join("::"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_named_lib_and_bin_get_distinct_ids() {
        // A package with a lib `bat` and a bin `bat` must not share a module-id
        // namespace, or `crate::X` in the lib resolves to the bin's `main.rs`.
        assert_ne!(
            module_node_id("bat 1.0", "lib", "bat", &[]),
            module_node_id("bat 1.0", "bin", "bat", &[]),
        );
        assert_ne!(
            module_node_id("bat 1.0", "lib", "bat", &["theme".into()]),
            module_node_id("bat 1.0", "bin", "bat", &["theme".into()]),
        );
    }
}
