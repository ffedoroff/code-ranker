//! Level-graph assembly split out of `pipeline.rs` so the parent module's
//! file-level SLOC stays under the project's own gate. These are
//! behaviour-preserving moves — verbatim from `pipeline.rs`, no logic changes.
//! Both functions depend only on their arguments and types from other
//! modules/crates, so the move introduces no parent↔child dependency cycle.

use crate::plugin;
use code_ranker_graph::level_graph::{LevelGraph, LevelUi};
use std::collections::BTreeMap;

/// Assemble one [`LevelGraph`]: merge the plugin's structural attribute specs
/// with the centrally-produced complexity + coupling specs, prune them (and the
/// edge kinds / groups) to what is actually present, and attach the graph,
/// cycles and stats.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_level(
    level_spec: Option<code_ranker_plugin_api::level::Level>,
    graph: code_ranker_plugin_api::graph::Graph,
    cycles: Vec<code_ranker_graph::level_graph::CycleGroup>,
    stats: BTreeMap<String, code_ranker_plugin_api::attrs::AttrValue>,
    thresholds: BTreeMap<String, code_ranker_plugin_api::level::Thresholds>,
    custom_specs: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    plugin_name: &str,
    eff_cfg: &toml::Table,
    report_overrides: &[code_ranker_plugin_api::report::ReportOverride],
) -> LevelGraph {
    use std::collections::BTreeSet;

    let spec = level_spec.unwrap_or_else(|| code_ranker_plugin_api::level::Level {
        name: "files".into(),
        edge_kinds: BTreeMap::new(),
        node_attributes: BTreeMap::new(),
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds: BTreeMap::new(),
        cycle_kinds: BTreeMap::new(),
        grouping: None,
    });

    // Master node-attribute dictionary = structural (plugin) + computed.
    let mut node_attributes = spec.node_attributes;
    // Language-neutral default metric specs, refined by the active plugin (e.g.
    // Rust adds the `#[cfg(test)]` nuance to the LOC descriptions). Passes the
    // effective config so any user overrides reach the plugin's refinement.
    let (default_metric_specs, metric_groups) = code_ranker_graph::metric_specs();
    let metric_specs = plugin::metric_specs(plugin_name, eff_cfg, default_metric_specs);
    let (coupling_specs, coupling_groups) = code_ranker_graph::coupling_specs();
    node_attributes.extend(metric_specs);
    node_attributes.extend(coupling_specs);
    // User-defined declarative metrics render as first-class columns; built-ins
    // win a key collision (a user cannot shadow a core metric's spec).
    for (key, spec) in custom_specs {
        node_attributes
            .entry(key.clone())
            .or_insert_with(|| spec.clone());
    }
    let mut attribute_groups = spec.attribute_groups;
    attribute_groups.extend(metric_groups);
    attribute_groups.extend(coupling_groups);

    // Overlay the gate-derived advisory thresholds onto the matching specs
    // (`warning` = the `[rules.thresholds.file]` limit; see `gate_thresholds`).
    for (key, th) in thresholds {
        if let Some(s) = node_attributes.get_mut(&key) {
            s.thresholds = Some(th);
        }
    }

    // The node-attribute dictionary keeps every key that exists in the JSON —
    // present on any node, external included — so the viewer can still label it
    // (e.g. external-node `path`/`version` shown in the diagram detail panel).
    let present_node_keys: BTreeSet<&str> = graph
        .nodes
        .iter()
        .flat_map(|n| n.attrs.keys().map(String::as_str))
        .collect();
    node_attributes.retain(|k, _| present_node_keys.contains(k.as_str()));

    // The `ui` lists, however, are filtered to keys present on at least one
    // *internal* (non-external) node. Those lists drive rendering surfaces
    // (table, summary, sort) that never show external rows (see `isExternalNode`
    // in schema.js); a metric living only on external nodes would otherwise be
    // promised in a list but never rendered. A node is external when it carries
    // `external: true` or its kind spec is marked external.
    let is_external = |n: &code_ranker_plugin_api::node::Node| -> bool {
        matches!(
            n.attrs.get("external"),
            Some(code_ranker_plugin_api::attrs::AttrValue::Bool(true))
        ) || spec
            .node_kinds
            .get(&n.kind)
            .and_then(|k| k.external)
            .unwrap_or(false)
    };
    let present_internal_keys: BTreeSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| !is_external(n))
        .flat_map(|n| n.attrs.keys().map(String::as_str))
        .collect();

    // Prune edge attributes to keys present on at least one edge.
    let present_edge_keys: BTreeSet<&str> = graph
        .edges
        .iter()
        .flat_map(|e| e.attrs.keys().map(String::as_str))
        .collect();
    let mut edge_attributes = spec.edge_attributes;
    edge_attributes.retain(|k, _| present_edge_keys.contains(k.as_str()));

    // Prune edge kinds to kinds present on at least one edge.
    let present_edge_kinds: BTreeSet<&str> = graph.edges.iter().map(|e| e.kind.as_str()).collect();
    let mut edge_kinds = spec.edge_kinds;
    edge_kinds.retain(|k, _| present_edge_kinds.contains(k.as_str()));

    // Prune groups to those referenced by a surviving node attribute.
    let referenced_groups: BTreeSet<&str> = node_attributes
        .values()
        .filter_map(|s| s.group.as_deref())
        .collect();
    attribute_groups.retain(|k, _| referenced_groups.contains(k.as_str()));

    // Prune node kinds to kinds actually present on nodes.
    let present_node_kinds: BTreeSet<&str> = graph.nodes.iter().map(|n| n.kind.as_str()).collect();
    let mut node_kinds = spec.node_kinds;
    node_kinds.retain(|k, _| present_node_kinds.contains(k.as_str()));

    // Cycle-kind vocabulary (label / why / fix) is central + data-driven
    // (`builtin.toml` `[cycles.*]`): overlay it onto the kinds the plugin's level
    // declares, then prune to kinds actually present in the cycle groups.
    let present_cycle_kinds: BTreeSet<&str> = cycles.iter().map(|c| c.kind.as_str()).collect();
    let mut cycle_kinds = spec.cycle_kinds;
    cycle_kinds.extend(code_ranker_graph::cycle_specs());
    cycle_kinds.retain(|k, _| present_cycle_kinds.contains(k.as_str()));

    let ui = build_ui(
        &node_attributes,
        &present_internal_keys,
        spec.grouping,
        report_overrides,
    );

    LevelGraph {
        edge_kinds,
        node_attributes,
        edge_attributes,
        attribute_groups,
        node_kinds,
        cycle_kinds,
        nodes: graph.nodes,
        edges: graph.edges,
        cycles,
        stats,
        ui,
    }
}

/// Build the `ui` block from the data-driven `[report]` view section, dropping
/// anything not present on an internal node
/// (`present_internal_keys`) — external-only keys stay in the dictionary but
/// never reach a render list. `kind` is always a column.
fn build_ui(
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    present_internal_keys: &std::collections::BTreeSet<&str>,
    grouping: Option<code_ranker_plugin_api::level::Grouping>,
    report_overrides: &[code_ranker_plugin_api::report::ReportOverride],
) -> LevelUi {
    let v = code_ranker_graph::views();
    let has = |k: &str| k == "kind" || present_internal_keys.contains(k);
    let pick =
        |list: &[String]| -> Vec<String> { list.iter().filter(|k| has(k)).cloned().collect() };

    // Apply the report patches (language then project) over the catalog lists in
    // order, then prune to keys present on an internal node.
    let cols_base = report_overrides
        .iter()
        .fold(v.columns.clone(), |acc, ov| ov.columns.apply(&acc));
    let card_base = report_overrides
        .iter()
        .fold(v.card.clone(), |acc, ov| ov.card.apply(&acc));
    let size_base = report_overrides
        .iter()
        .fold(v.size.clone(), |acc, ov| ov.size.apply(&acc));
    let filter_base = report_overrides
        .iter()
        .fold(v.filter.clone(), |acc, ov| ov.filter.apply(&acc));
    let columns = pick(&cols_base);
    let card = pick(&card_base);
    // Map controls: prune to keys present on an internal node. `cycle` is a
    // string attribute (a cycle kind), valid as a filter but never a size mode.
    let size = pick(&size_base);
    let filter = pick(&filter_base);
    // Default sort: a signed-rank list (order = priority, leading `-` =
    // descending). Strip the sign and pick the first key present. Every column
    // stays sortable in the UI — this only sets the opening order.
    let default_sort = v
        .default_sort
        .iter()
        .map(|s| s.strip_prefix('-').unwrap_or(s))
        .find(|k| has(k))
        .map(|k| k.to_string());
    // Sortable = every column except the `kind` label.
    let sort: Vec<String> = columns.iter().filter(|k| *k != "kind").cloned().collect();
    // Summary rows = the numeric metric columns (exclude the `kind` label and the
    // categorical `cycle`).
    let summary: Vec<String> = columns
        .iter()
        .filter(|k| *k != "kind" && *k != "cycle")
        .cloned()
        .collect();

    // Keep the grouping only if it is usable: a `key` must reference an attribute
    // that survived pruning; a `function` is passed through. Otherwise drop it so
    // the viewer falls back to its default `dir` grouper.
    let grouping = grouping.filter(|g| match &g.key {
        Some(k) => node_attributes.contains_key(k),
        None => g.function.is_some(),
    });
    LevelUi {
        default_sort,
        sort,
        size,
        filter,
        card,
        columns,
        summary,
        grouping,
    }
}
