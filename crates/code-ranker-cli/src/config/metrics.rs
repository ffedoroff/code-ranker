//! The per-file threshold metric vocabulary: every metric that can carry a
//! `[rules.thresholds.file]` limit, with its concern group and human label.
//!
//! This is **data-driven** — the metric set, labels and groups come from the
//! `code-ranker-graph` registry (`metric_specs` + `coupling_specs`) plus the
//! structural per-file attributes the plugins emit. No metric name is hardcoded
//! here, so a new registry metric is thresholdable automatically.

/// A per-file metric that can carry a threshold: its `key`, the concern `group`
/// (one of `CPX` / `CPL` / `SIZ`, matching the [`super::rules::RULES`] groups),
/// and the human `label` used in the breach message.
pub struct ThresholdMetric {
    pub key: String,
    pub group: &'static str,
    pub label: String,
}

/// Map an attribute-spec group to a threshold concern group.
pub(crate) fn concern_group(spec_group: Option<&str>) -> &'static str {
    match spec_group {
        Some("loc") => "SIZ",
        Some("coupling") => "CPL",
        // complexity / halstead / maintainability (and anything else) → CPX.
        _ => "CPX",
    }
}

/// The full per-file threshold vocabulary, built from the registry specs plus the
/// structural attributes (`loc` / `items` / `unsafe`) the plugins emit directly.
pub fn threshold_metrics() -> Vec<ThresholdMetric> {
    let (metrics, _) = code_ranker_graph::metric_specs();
    let (coupling, _) = code_ranker_graph::coupling_specs();
    let mut out: Vec<ThresholdMetric> = Vec::new();
    for (key, spec) in metrics.iter().chain(coupling.iter()) {
        // `cycle` is a string attribute (a cycle kind), not a numeric threshold.
        if key == "cycle" {
            continue;
        }
        let label = spec
            .name
            .clone()
            .or_else(|| spec.label.clone())
            .unwrap_or_else(|| key.clone());
        out.push(ThresholdMetric {
            key: key.clone(),
            group: concern_group(spec.group.as_deref()),
            label,
        });
    }
    // Structural per-file attributes (not in the metric registry) — emitted by a
    // specific plugin, thresholdable like any metric. `items`/`unsafe` are Rust;
    // the `headings`…`broken_links` group is Markdown's doc metrics.
    for (key, group, label) in [
        ("loc", "SIZ", "source loc"),
        ("items", "SIZ", "item count"),
        ("unsafe", "CPX", "unsafe blocks"),
        ("headings", "SIZ", "headings"),
        ("max_depth", "SIZ", "max heading depth"),
        ("code_lines", "SIZ", "embedded code lines"),
        ("links", "CPL", "links"),
        ("broken_links", "CPX", "broken links"),
    ] {
        out.push(ThresholdMetric {
            key: key.to_string(),
            group,
            label: label.to_string(),
        });
    }
    out
}

/// The threshold metadata for a metric key, if it is a known per-file metric.
pub fn threshold_metric(key: &str) -> Option<ThresholdMetric> {
    threshold_metrics().into_iter().find(|m| m.key == key)
}

/// Is `key` a metric that can carry a per-file threshold?
pub fn is_threshold_metric(key: &str) -> bool {
    threshold_metrics().iter().any(|m| m.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_metrics_cover_engine_specs() {
        // Every numeric per-file metric the engine emits must be thresholdable.
        // `cycle` is a string attribute, so it is excluded.
        let (metrics, _) = code_ranker_graph::metric_specs();
        let (coupling, _) = code_ranker_graph::coupling_specs();
        for key in metrics.keys().chain(coupling.keys()) {
            if key == "cycle" {
                continue;
            }
            assert!(
                is_threshold_metric(key),
                "engine metric {key:?} is not thresholdable"
            );
        }
    }
}
