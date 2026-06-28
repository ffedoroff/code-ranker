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

/// Multi-language variant of [`prune_unused_roots`]: prunes by scanning all
/// nodes across every level of every language in the snapshot. `target` is
/// always kept.
pub(super) fn prune_unused_roots_multi(
    languages: &std::collections::BTreeMap<String, code_ranker_graph::snapshot::LanguageSnapshot>,
    roots: &mut BTreeMap<String, String>,
) {
    let mut used: HashSet<String> = HashSet::new();
    used.insert("target".to_string());
    for ls in languages.values() {
        for level in ls.graphs.values() {
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
        }
    }
    roots.retain(|name, _| used.contains(name));
}

/// The `omit_at` (no-signal floor) of every metric key, so an aggregate's `all`
/// population counts a missing value at the right floor (`0` for most, `1` for
/// `cyclomatic`). Built from the central + plugin-refined + coupling specs, then
/// the user's own metric defs.
pub(super) fn registry_omit_at(
    plugin_name: &str,
    eff_cfg: &toml::Table,
    custom: &BTreeMap<String, code_ranker_graph::MetricDef>,
) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    let (specs, _) = code_ranker_graph::metric_specs();
    for (k, s) in crate::plugin::metric_specs(plugin_name, eff_cfg, specs) {
        m.insert(k, s.omit_at);
    }
    let (coupling, _) = code_ranker_graph::coupling_specs();
    for (k, s) in coupling {
        m.insert(k, s.omit_at);
    }
    for (k, d) in custom {
        m.insert(k.clone(), d.omit_at);
    }
    m
}

/// Enforce the one-file-one-language invariant: the active languages' internal
/// (non-external) node sets must be disjoint. The extension-uniqueness check
/// covers extension-based plugins; this also catches any residual overlap (e.g.
/// Rust's cargo-metadata paths, which carry no `extensions`). A duplicate means a
/// file was analysed by two languages — double-counting it and breaking the
/// `--focus`/`--focus-path` path→language mapping.
pub(super) fn assert_disjoint_languages(
    languages: &std::collections::BTreeMap<String, code_ranker_graph::snapshot::LanguageSnapshot>,
) -> anyhow::Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for ls in languages.values() {
        for level in ls.graphs.values() {
            for node in &level.nodes {
                if node.kind == "external" {
                    continue;
                }
                if !seen.insert(node.id.as_str()) {
                    debug_assert!(false, "file {} claimed by >1 language", node.id);
                    // COVERAGE: release-only — under test/debug the `debug_assert!`
                    // above panics first, so this `bail!` (the production fallback)
                    // is unreachable when coverage is instrumented.
                    anyhow::bail!(
                        "internal error: file {:?} was analysed by more than one language; \
                         adjust `extensions` / `plugins` so each file maps to exactly one language",
                        node.id
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_graph::snapshot::LanguageSnapshot;
    use code_ranker_plugin_api::node::Node;

    /// A single-language snapshot whose `files` level holds one node.
    fn lang_with_node(id: &str, kind: &str) -> LanguageSnapshot {
        let level = LevelGraph {
            nodes: vec![Node {
                id: id.into(),
                kind: kind.into(),
                name: id.into(),
                parent: None,
                attrs: Default::default(),
            }],
            ..Default::default()
        };
        let mut graphs = BTreeMap::new();
        graphs.insert("files".to_string(), level);
        LanguageSnapshot {
            graphs,
            principles: vec![],
            prompt: Default::default(),
        }
    }

    /// Distinct internal files pass; a shared id that is `external` in one language
    /// is exempt (external nodes are third-party, not owned by a language).
    #[test]
    fn assert_disjoint_languages_accepts_distinct_and_external() {
        let mut langs = BTreeMap::new();
        langs.insert("rust".to_string(), lang_with_node("a.rs", "file"));
        // same id but external → ignored by the check
        langs.insert("python".to_string(), lang_with_node("a.rs", "external"));
        langs.insert("go".to_string(), lang_with_node("b.go", "file"));
        assert!(assert_disjoint_languages(&langs).is_ok());
    }

    /// Two languages claiming the same internal file trip the invariant. In a
    /// debug/test build the `debug_assert!` fires (the dev guard); the `bail!`
    /// fallback only runs in release.
    #[test]
    #[should_panic(expected = "claimed by >1 language")]
    fn assert_disjoint_languages_rejects_shared_internal_file() {
        let mut langs = BTreeMap::new();
        langs.insert("rust".to_string(), lang_with_node("dup.rs", "file"));
        langs.insert("ts".to_string(), lang_with_node("dup.rs", "file"));
        let _ = assert_disjoint_languages(&langs);
    }
}
