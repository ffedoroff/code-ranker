//! `--suggest-config` current-values dump: render the project's measured per-file
//! maxima and cycle counts as ready-to-paste `code-ranker.toml` blocks. Extracted
//! verbatim from `check.rs` to keep that file's aggregate complexity in budget.

use crate::config;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::attrs::{AttrValue, ValueType};
use code_ranker_plugin_api::level::Direction;
use code_ranker_plugin_api::node::Node;
use std::collections::{BTreeMap, BTreeSet};

/// Print the current measured values per scope as ready-to-paste `code-ranker.toml`
/// threshold blocks: the per-unit worst value (`single`) and the graph-wide
/// average (`avg`). Lets a user pin today's numbers as a baseline that passes.
pub(super) fn print_current_values(
    graphs: &BTreeMap<String, LevelGraph>,
    cycles: &config::CycleRules,
) {
    let Some(level) = graphs.get("files") else {
        return;
    };
    println!();
    println!("Current config — copy the blocks below into code-ranker.toml:");

    // Cycle budgets: today's count per kind (paste to forbid adding more).
    println!();
    println!(
        "# cycles: max allowed count per kind (today's count — raise only to allow more; false = off)"
    );
    println!("[rules.cycles]");
    for (key, kind, rule) in [
        ("mutual", "mutual", cycles.mutual),
        ("chain", "chain", cycles.chain),
    ] {
        if rule.is_off() {
            println!("{key:<12}= false");
        } else {
            let n = level.cycles.iter().filter(|c| c.kind == kind).count();
            println!("{key:<12}= {n}");
        }
    }

    // Thresholds: measured per-file maxima to pin as a baseline.
    println!();
    println!("# thresholds: the worst single file (max) per metric");
    print_scope_values("file", level);
}

/// Read a node's numeric attribute (`Int`/`Float` → `f64`, anything else → `0`).
fn attr(n: &Node, key: &str) -> f64 {
    match n.attrs.get(key) {
        Some(AttrValue::Int(i)) => *i as f64,
        Some(AttrValue::Float(f)) => *f,
        _ => 0.0,
    }
}

/// The metric keys to suggest a threshold for, in display order — data-driven, no
/// metric names hardcoded. A key qualifies when it is a known per-file threshold
/// metric (`config::metrics`) AND its snapshot spec is numeric and not
/// higher-is-better (a `>` cap is meaningful for lower-better and neutral metrics
/// like `sloc`/`fan_out`, but not for `mi`/`mi_sei` where bigger is healthier).
/// Ordered by the report column order, with any remaining candidates appended
/// alphabetically.
fn suggestable_metrics(level: &LevelGraph) -> Vec<&str> {
    let is_candidate = |key: &str| {
        config::metrics::is_threshold_metric(key)
            && level.node_attributes.get(key).is_some_and(|spec| {
                spec.direction != Direction::HigherBetter
                    && matches!(spec.value_type, ValueType::Int | ValueType::Float)
            })
    };
    let mut seen = BTreeSet::new();
    let mut out: Vec<&str> = Vec::new();
    for key in &level.ui.columns {
        if is_candidate(key) && seen.insert(key.as_str()) {
            out.push(key.as_str());
        }
    }
    let mut extra: Vec<&str> = level
        .node_attributes
        .keys()
        .map(String::as_str)
        .filter(|k| is_candidate(k) && !seen.contains(k))
        .collect();
    extra.sort_unstable();
    out.extend(extra);
    out
}

/// Emit a `[rules.thresholds.<scope>]` block with the per-file metric maxima,
/// read from the flat node `attrs`.
fn print_scope_values(scope: &str, level: &LevelGraph) {
    let keys = suggestable_metrics(level);
    let mut maxima: BTreeMap<&str, f64> = keys.iter().map(|k| (*k, 0f64)).collect();
    let mut any = false;
    for n in &level.nodes {
        if n.kind == "external" {
            continue;
        }
        any = true;
        for key in &keys {
            let slot = maxima.get_mut(key).expect("key seeded above");
            *slot = slot.max(attr(n, key));
        }
    }
    if !any {
        return;
    }
    // Preserve the display order from `keys` (BTreeMap would re-sort).
    let vals: Vec<(&str, f64)> = keys.iter().map(|k| (*k, maxima[*k])).collect();
    print_toml_block(&format!("[rules.thresholds.{scope}]"), &vals, false);
}

/// Print one TOML table, one `metric = value` line per non-zero metric. With
/// `round_up`, fractional values (averages) are ceiled so a strict `>` check
/// still passes at the printed limit.
fn print_toml_block(header: &str, vals: &[(&str, f64)], round_up: bool) {
    let rows: Vec<(&str, u64)> = vals
        .iter()
        .filter_map(|&(name, v)| {
            let n = if round_up { v.ceil() } else { v.round() } as u64;
            (n > 0).then_some((name, n))
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    println!();
    println!("{header}");
    for (name, v) in rows {
        println!("{name:<12}= {}", group_digits(v));
    }
}

/// Format an integer with `_` thousands separators (e.g. 512712 → "512_712"),
/// matching the human number syntax accepted by `--threshold` / the config.
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::CycleRule;
    use code_ranker_plugin_api::level::AttributeSpec;

    fn node(id: &str, kind: &str, attrs: &[(&str, AttrValue)]) -> Node {
        let mut a = BTreeMap::new();
        for (k, v) in attrs {
            a.insert((*k).to_string(), v.clone());
        }
        Node {
            id: id.into(),
            kind: kind.into(),
            name: id.into(),
            parent: None,
            attrs: a,
        }
    }

    #[test]
    fn group_digits_inserts_thousands_separators() {
        assert_eq!(group_digits(7), "7");
        assert_eq!(group_digits(512_712), "512_712");
        assert_eq!(group_digits(1_000_000), "1_000_000");
    }

    #[test]
    fn print_current_values_no_files_level_is_noop() {
        // No `files` level → early return, nothing printed, no panic.
        print_current_values(&BTreeMap::new(), &config::CycleRules::default());
    }

    #[test]
    fn print_current_values_handles_off_cycles_and_external_only_nodes() {
        // Both cycle rules off → the `= false` branch; only an external node →
        // `print_scope_values` finds no measurable unit and returns early.
        let mut level = LevelGraph {
            nodes: vec![node("ext", "external", &[])],
            ..Default::default()
        };
        level
            .node_attributes
            .insert("loc".into(), AttributeSpec::new(ValueType::Int, "LOC"));
        let mut graphs = BTreeMap::new();
        graphs.insert("files".to_string(), level);
        let cycles = config::CycleRules {
            mutual: CycleRule::Off,
            chain: CycleRule::Off,
        };
        print_current_values(&graphs, &cycles);
    }

    #[test]
    fn print_current_values_skips_all_zero_metrics() {
        // A real file node whose only suggestable metric is zero → the TOML block
        // has no rows and is omitted (`print_toml_block` early return).
        let mut level = LevelGraph {
            nodes: vec![node("a.rs", "file", &[("loc", AttrValue::Int(0))])],
            ..Default::default()
        };
        level
            .node_attributes
            .insert("loc".into(), AttributeSpec::new(ValueType::Int, "LOC"));
        level.ui.columns = vec!["loc".into()];
        let mut graphs = BTreeMap::new();
        graphs.insert("files".to_string(), level);
        print_current_values(&graphs, &config::CycleRules::default());
    }
}
