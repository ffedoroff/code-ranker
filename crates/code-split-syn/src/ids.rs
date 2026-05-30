//! Shared id helpers used by both `crate_graph` and `module_graph`. Kept in a
//! leaf module so those submodules depend on it rather than "up" on the crate
//! root — which would otherwise close a `root → submodule → root` cycle.

/// The canonical graph node id for a crate, derived from its cargo `pkg_id` repr.
pub(crate) fn crate_node_id(pkg_id_repr: &str) -> String {
    format!("crate:{pkg_id_repr}")
}
