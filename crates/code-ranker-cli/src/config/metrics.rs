//! The per-file threshold metric vocabulary: every metric that can carry a
//! `[rules.thresholds.file]` limit, with its concern group and human label.
//!
//! This is a **leaf** module — it depends on nothing else in `config`, so both
//! the data model (`model`, which validates config keys against it) and the rule
//! catalog (`rules`, which resolves a metric's group through it) can use it
//! without forming a `model ↔ rules` dependency cycle.

/// A per-file metric that can carry a threshold: its key, the concern `group`
/// (one of `CPX` / `CPL` / `SIZ`, matching the [`super::rules::RULES`] groups),
/// and the human `label` used in the breach message. A threshold is a
/// `value > limit` gate, so this is the whole numeric per-file vocabulary — every
/// metric the engine emits is accepted, not a hand-picked subset.
/// `threshold_metrics_cover_engine_specs` (test) guards that this list stays in
/// sync with the engine's metric specs.
pub struct ThresholdMetric {
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
}

pub const THRESHOLD_METRICS: &[ThresholdMetric] = &[
    // CPX — control-flow complexity, maintainability, and Halstead effort.
    tm("cyclomatic", "CPX", "cyclomatic complexity"),
    tm("cognitive", "CPX", "cognitive complexity"),
    tm("exits", "CPX", "exit points"),
    tm("args", "CPX", "argument count"),
    tm("closures", "CPX", "closure count"),
    tm("mi", "CPX", "maintainability index"),
    tm("mi_sei", "CPX", "maintainability index (SEI)"),
    tm("length", "CPX", "Halstead length"),
    tm("vocabulary", "CPX", "Halstead vocabulary"),
    tm("volume", "CPX", "Halstead volume"),
    tm("effort", "CPX", "Halstead effort"),
    tm("time", "CPX", "Halstead time"),
    tm("bugs", "CPX", "Halstead bugs"),
    tm("unsafe", "CPX", "unsafe blocks"),
    // SIZ — size.
    tm("sloc", "SIZ", "source loc"),
    tm("loc", "SIZ", "source loc"),
    tm("lloc", "SIZ", "logical loc"),
    tm("cloc", "SIZ", "comment loc"),
    tm("blank", "SIZ", "blank lines"),
    tm("tloc", "SIZ", "test loc"),
    tm("items", "SIZ", "item count"),
    // CPL — coupling.
    tm("fan_in", "CPL", "fan-in"),
    tm("fan_out", "CPL", "fan-out"),
    tm("fan_out_external", "CPL", "external fan-out"),
    tm("hk", "CPL", "Henry-Kafura hk"),
];

const fn tm(key: &'static str, group: &'static str, label: &'static str) -> ThresholdMetric {
    ThresholdMetric { key, group, label }
}

/// The threshold metadata for a metric key, if it is a known per-file metric.
pub fn threshold_metric(key: &str) -> Option<&'static ThresholdMetric> {
    THRESHOLD_METRICS.iter().find(|m| m.key == key)
}

/// Is `key` a metric that can carry a per-file threshold?
pub fn is_threshold_metric(key: &str) -> bool {
    threshold_metric(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_metrics_cover_engine_specs() {
        // Every numeric per-file metric the engine emits must be thresholdable, so
        // the config accepts the full vocabulary. `cycle` is a string attribute
        // (a cycle kind), not a numeric threshold, so it is excluded.
        let (metrics, _) = code_ranker_graph::metric_specs();
        let (coupling, _) = code_ranker_graph::coupling_specs();
        for key in metrics.keys().chain(coupling.keys()) {
            if key == "cycle" {
                continue;
            }
            assert!(
                is_threshold_metric(key),
                "engine metric {key:?} is not in THRESHOLD_METRICS — add it (with a group)"
            );
        }
    }
}
