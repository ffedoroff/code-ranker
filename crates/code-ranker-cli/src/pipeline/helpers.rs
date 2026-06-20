//! Small pure helpers split out of `pipeline.rs` so the parent module's
//! file-level aggregate cyclomatic stays under the project's own gate. These are
//! behaviour-preserving moves — verbatim from `pipeline.rs`, no logic changes.

use code_ranker_graph::level_graph::LevelGraph;
use std::collections::{BTreeMap, HashSet};

/// The set of edge kinds that carry information flow at this level (read from
/// `EdgeKindSpec.flow`). Cycles and coupling count only these.
pub(super) fn flow_kinds(level: Option<&code_ranker_plugin_api::level::Level>) -> HashSet<String> {
    match level {
        Some(l) => l
            .edge_kinds
            .iter()
            .filter(|(_, spec)| spec.flow)
            .map(|(k, _)| k.clone())
            .collect(),
        None => HashSet::new(),
    }
}

/// A node's numeric attributes as `f64` (the inputs an aggregate reduces over).
pub(super) fn numeric_attrs(node: &code_ranker_plugin_api::node::Node) -> BTreeMap<String, f64> {
    use code_ranker_plugin_api::attrs::AttrValue;
    node.attrs
        .iter()
        .filter_map(|(k, v)| match v {
            AttrValue::Int(i) => Some((k.clone(), *i as f64)),
            AttrValue::Float(f) => Some((k.clone(), *f)),
            _ => None,
        })
        .collect()
}

/// Remove named roots whose `{name}` token does not appear in any node id or
/// path after relativization. `target` is always kept (it names the analyzed
/// project even when every node sits directly under it). This keeps the
/// snapshot header free of roots that are irrelevant to the analyzed language
/// (e.g. the Rust toolchain roots in a JS/TS/Python snapshot).
pub(super) fn prune_unused_roots(level: &LevelGraph, roots: &mut BTreeMap<String, String>) {
    let mut used: HashSet<String> = HashSet::new();
    used.insert("target".to_string());
    for node in &level.nodes {
        let path_attr = match node.attrs.get("path") {
            Some(code_ranker_plugin_api::attrs::AttrValue::Str(p)) => p.as_str(),
            _ => "",
        };
        for name in roots.keys() {
            let token = format!("{{{name}}}");
            if node.id.contains(&token) || path_attr.contains(&token) {
                used.insert(name.clone());
            }
        }
    }
    roots.retain(|name, _| used.contains(name));
}
