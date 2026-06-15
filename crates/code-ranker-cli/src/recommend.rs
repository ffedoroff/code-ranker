//! The recommendation engine behind the `prompt` and `scorecard` report formats.
//!
//! It is the console counterpart of the HTML viewer's Prompt Generator: the same
//! ranking (`reco_for` ≈ `recoFor` in `export-popup.js`) and the same Markdown
//! prompt (`compose_prompt` ≈ `composePrompt` + `buildContent`), plus a console
//! triage table (`render_scorecard`) that mirrors the viewer's per-preset badges.
//!
//! All of it is **advisory**, derived from the snapshot's language-calibrated
//! `node_attributes[*].thresholds` (the `info` / `warning` tiers) — never a gate.

use anyhow::{Result, bail};
use code_ranker_graph::level_graph::{CycleGroup, LevelGraph};
use code_ranker_plugin_api::{attrs::AttrValue, level::Thresholds, node::Node, plugin::Preset};
use std::collections::HashMap;

mod prompt;
mod scorecard;

pub use prompt::compose_prompt;
pub use scorecard::render_scorecard;

/// Which threshold tier drives an output. `Auto` resolves to `Warning` when any
/// module breaches it, else `Info` (the viewer's headline rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Auto,
}

/// Parse a `--severity` value (`info` / `warning` / `auto`). Invalid is fatal —
/// the tool never silently ignores an unknown rule knob.
pub fn parse_severity(s: &str) -> Result<Severity> {
    match s {
        "info" => Ok(Severity::Info),
        "warning" => Ok(Severity::Warning),
        "auto" => Ok(Severity::Auto),
        other => bail!("invalid --severity '{other}': expected info, warning, or auto"),
    }
}

/// A single ranking metric's recommendation: the candidate file nodes sorted
/// worst-first, plus how many cross the `warning` / `info` tiers. For the pseudo
/// metric `"cycle"` the candidates are the nodes in a dependency cycle (ranked by
/// HK) and both counts equal that set's size.
pub struct Reco<'a> {
    pub sorted: Vec<&'a Node>,
    pub warning_count: usize,
    pub info_count: usize,
}

/// Read a numeric node attribute (`Int`/`Float`) as `f64`, else `None`.
pub(super) fn num(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// A project source file (not a third-party library node).
pub(super) fn is_internal(node: &Node) -> bool {
    node.kind != "external"
}

/// Is this file node in a dependency cycle? (the orchestrator writes a `cycle`
/// string attribute on every cycle member).
pub(super) fn in_cycle(node: &Node) -> bool {
    matches!(node.attrs.get("cycle"), Some(AttrValue::Str(_)))
}

/// The two-tier thresholds for a metric: the metric's own, falling back to HK's,
/// then to a never-breached `{0, 0}` — mirroring the viewer's `recoFor`.
fn thresholds_for(level: &LevelGraph, metric: &str) -> Thresholds {
    level
        .node_attributes
        .get(metric)
        .and_then(|s| s.thresholds)
        .or_else(|| level.node_attributes.get("hk").and_then(|s| s.thresholds))
        .unwrap_or(Thresholds {
            info: 0.0,
            warning: 0.0,
        })
}

/// The short header label for a metric (falls back to its label, then the key).
pub(super) fn attr_short<'a>(level: &'a LevelGraph, metric: &'a str) -> &'a str {
    level
        .node_attributes
        .get(metric)
        .and_then(|s| s.short.as_deref().or(s.label.as_deref()))
        .unwrap_or(metric)
}

/// Strip a leading `{root}/` token from a relativized id, e.g.
/// `{target}/src/a.rs` → `src/a.rs`. A file node's id IS its path.
pub fn clean_path(id: &str) -> String {
    if let Some(rest) = id.strip_prefix('{')
        && let Some(idx) = rest.find("}/")
    {
        return rest[idx + 2..].to_string();
    }
    id.to_string()
}

/// Rank the file nodes for one metric, worst-first, and count tier breaches.
/// `"cycle"` is special-cased (cycle members ranked by HK).
pub fn reco_for<'a>(level: &'a LevelGraph, metric: &str) -> Reco<'a> {
    if metric == "cycle" {
        let mut sorted: Vec<&Node> = level
            .nodes
            .iter()
            .filter(|n| is_internal(n) && in_cycle(n))
            .collect();
        sorted.sort_by(|a, b| {
            num(b, "hk")
                .unwrap_or(0.0)
                .total_cmp(&num(a, "hk").unwrap_or(0.0))
        });
        let n = sorted.len();
        return Reco {
            sorted,
            warning_count: n,
            info_count: n,
        };
    }

    let th = thresholds_for(level, metric);
    let mut sorted: Vec<&Node> = level.nodes.iter().filter(|n| is_internal(n)).collect();
    // Worst-first by the metric, tie-broken by sloc then items (as in the viewer)
    // so equal scores still order deterministically.
    sorted.sort_by(|a, b| {
        let key = |n: &Node| {
            (
                num(n, metric).unwrap_or(0.0),
                num(n, "sloc").unwrap_or(0.0),
                num(n, "items").unwrap_or(0.0),
            )
        };
        let (am, as_, ai) = key(a);
        let (bm, bs, bi) = key(b);
        bm.total_cmp(&am)
            .then(bs.total_cmp(&as_))
            .then(bi.total_cmp(&ai))
    });
    let warning_count = sorted
        .iter()
        .filter(|n| num(n, metric).unwrap_or(0.0) > th.warning)
        .count();
    let info_count = sorted
        .iter()
        .filter(|n| num(n, metric).unwrap_or(0.0) > th.info)
        .count();
    Reco {
        sorted,
        warning_count,
        info_count,
    }
}

/// Cycle groups ranked worst-first for the ADP (cycle) preset: `chain` cycles
/// before `mutual`, larger SCCs before smaller, so `--top 1` surfaces the single
/// biggest chain. Ties broken by the first node id for determinism.
fn ranked_cycle_groups(level: &LevelGraph) -> Vec<&CycleGroup> {
    let mut groups: Vec<&CycleGroup> = level.cycles.iter().collect();
    groups.sort_by(|a, b| {
        let chain = |g: &CycleGroup| u8::from(g.kind == "chain");
        chain(b)
            .cmp(&chain(a))
            .then(b.nodes.len().cmp(&a.nodes.len()))
            .then(a.nodes.first().cmp(&b.nodes.first()))
    });
    groups
}

/// The top-N cycle groups (see [`ranked_cycle_groups`]), each paired with its
/// member nodes ordered by HK (worst first). A node id with no matching node is
/// skipped. This is the unit the ADP preset recommends on: `--top` counts
/// **cycles**, and every member of each selected cycle is listed.
pub(super) fn top_cycle_groups(
    level: &LevelGraph,
    n_groups: usize,
) -> Vec<(&CycleGroup, Vec<&Node>)> {
    let by_id: HashMap<&str, &Node> = level.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    ranked_cycle_groups(level)
        .into_iter()
        .take(n_groups)
        .map(|g| {
            let mut members: Vec<&Node> = g
                .nodes
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            members.sort_by(|a, b| {
                num(b, "hk")
                    .unwrap_or(0.0)
                    .total_cmp(&num(a, "hk").unwrap_or(0.0))
            });
            (g, members)
        })
        .collect()
}

/// How many modules a tier selects for a metric's reco.
pub(super) fn tier_count(reco: &Reco, sev: Severity) -> usize {
    match sev {
        Severity::Warning => reco.warning_count,
        Severity::Info => reco.info_count,
        Severity::Auto => {
            if reco.warning_count > 0 {
                reco.warning_count
            } else {
                reco.info_count
            }
        }
    }
}

/// The principle with the most violations: highest `warning` count, tie-broken by
/// `info` count, then by catalog order (the first preset wins on a tie). `None`
/// only if there are no presets.
pub fn worst_preset(level: &LevelGraph, presets: &[Preset]) -> Option<String> {
    let mut best: Option<(&Preset, usize, usize)> = None;
    for p in presets {
        let r = reco_for(level, &p.sort_metric);
        // Strictly-greater so the FIRST preset wins on a tie (catalog order).
        let better = match best {
            None => true,
            Some((_, bw, bi)) => (r.warning_count, r.info_count) > (bw, bi),
        };
        if better {
            best = Some((p, r.warning_count, r.info_count));
        }
    }
    best.map(|(p, _, _)| p.id.clone())
        .or_else(|| presets.first().map(|p| p.id.clone()))
}

/// Count of project source files in the level.
pub(super) fn file_count(level: &LevelGraph) -> usize {
    level.nodes.iter().filter(|n| is_internal(n)).count()
}

/// Format a metric value: abbreviate large numbers to K/M/G when the attribute
/// is flagged `abbreviate`, else a plain rounded integer.
pub(super) fn fmt_val(level: &LevelGraph, metric: &str, v: f64) -> String {
    let abbreviate = level
        .node_attributes
        .get(metric)
        .and_then(|s| s.abbreviate)
        .unwrap_or(false);
    if abbreviate && v.abs() >= 1000.0 {
        for (suf, div) in [("G", 1e9), ("M", 1e6), ("K", 1e3)] {
            if v.abs() >= div {
                let n = v / div;
                let s = format!("{n:.1}");
                let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
                return format!("{s}{suf}");
            }
        }
    }
    format!("{}", v.round() as i64)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_plugin_api::{attrs::ValueType, level::AttributeSpec};
    use std::collections::BTreeMap;

    fn node_kind(id: &str, kind: &str, attrs: &[(&str, AttrValue)]) -> Node {
        let mut a: BTreeMap<String, AttrValue> = BTreeMap::new();
        for (k, v) in attrs {
            a.insert((*k).to_string(), v.clone());
        }
        Node {
            id: id.to_string(),
            kind: kind.to_string(),
            name: id.rsplit('/').next().unwrap_or(id).to_string(),
            parent: None,
            attrs: a,
        }
    }
    fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
        node_kind(id, "file", attrs)
    }

    fn level_with(nodes: Vec<Node>) -> LevelGraph {
        let mut na: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        let mut hk = AttributeSpec::new(ValueType::Float, "HK");
        hk.short = Some("HK".into());
        hk.abbreviate = Some(true);
        hk.thresholds = Some(Thresholds {
            info: 100.0,
            warning: 1000.0,
        });
        na.insert("hk".into(), hk);
        let mut sloc = AttributeSpec::new(ValueType::Int, "SLOC");
        sloc.short = Some("SLOC".into());
        sloc.thresholds = Some(Thresholds {
            info: 50.0,
            warning: 200.0,
        });
        na.insert("sloc".into(), sloc);
        LevelGraph {
            node_attributes: na,
            nodes,
            ..Default::default()
        }
    }

    #[test]
    fn reco_for_sorts_worst_first_and_counts_tiers() {
        let level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(2000.0)),
                    ("sloc", AttrValue::Int(10)),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(150.0)),
                    ("sloc", AttrValue::Int(10)),
                ],
            ),
            file_node(
                "{target}/c.rs",
                &[("hk", AttrValue::Float(10.0)), ("sloc", AttrValue::Int(10))],
            ),
            node_kind("ext:x", "external", &[]),
        ]);
        let r = reco_for(&level, "hk");
        // External excluded; worst-first by hk.
        assert_eq!(
            r.sorted.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["{target}/a.rs", "{target}/b.rs", "{target}/c.rs"]
        );
        assert_eq!(r.warning_count, 1, "only a.rs > 1000");
        assert_eq!(r.info_count, 2, "a.rs and b.rs > 100");
    }

    #[test]
    fn reco_for_cycle_uses_cycle_members() {
        let level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(50.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(80.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
            file_node("{target}/c.rs", &[("hk", AttrValue::Float(900.0))]),
        ]);
        let r = reco_for(&level, "cycle");
        assert_eq!(r.warning_count, 2);
        assert_eq!(r.info_count, 2);
        // Ranked by hk: b (80) before a (50).
        assert_eq!(r.sorted[0].id, "{target}/b.rs");
    }

    #[test]
    fn worst_preset_picks_most_violations() {
        let level = level_with(vec![file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(2000.0)),
                ("sloc", AttrValue::Int(10)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        )]);
        let presets = vec![
            Preset {
                id: "SRP".into(),
                label: "SRP".into(),
                title: "SRP — x".into(),
                prompt: "p".into(),
                doc_url: None,
                sort_metric: "sloc".into(),
                connections: vec![],
            },
            Preset {
                id: "ADP".into(),
                label: "ADP".into(),
                title: "ADP — x".into(),
                prompt: "p".into(),
                doc_url: None,
                sort_metric: "cycle".into(),
                connections: vec!["common".into()],
            },
        ];
        // SRP: sloc 10 → 0 breaches; ADP: cycle → 1. ADP wins.
        assert_eq!(worst_preset(&level, &presets).as_deref(), Some("ADP"));
    }

    #[test]
    fn compose_prompt_cycle_lists_modules_and_connections() {
        let mut level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(50.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(80.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
        ]);
        // The cycle recommendation groups by the level's `cycles` (the SCC groups
        // the pipeline computes), not by per-node attrs.
        level.cycles.push(CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
        });
        level.edges.push(code_ranker_plugin_api::edge::Edge {
            source: "{target}/a.rs".into(),
            target: "{target}/b.rs".into(),
            kind: "uses".into(),
            line: None,
            attrs: Default::default(),
        });
        let presets = vec![Preset {
            id: "ADP".into(),
            label: "ADP".into(),
            title: "ADP — Acyclic".into(),
            prompt: "the DAG rule".into(),
            doc_url: Some("http://x/adp.md".into()),
            sort_metric: "cycle".into(),
            connections: vec!["common".into()],
        }];
        let md = compose_prompt(&level, &presets, "ADP", Severity::Auto, None).unwrap();
        assert!(md.contains("# ADP — Acyclic"), "title heading: {md}");
        assert!(md.contains("## Summary\n\nthe DAG rule"), "summary body");
        assert!(
            md.contains("**Full principle:** [http://x/adp.md]"),
            "doc link"
        );
        assert!(
            md.contains("## Modules in a dependency cycle"),
            "cycle modules section"
        );
        assert!(
            md.contains("- `a.rs`") && md.contains("- `b.rs`"),
            "module paths cleaned: {md}"
        );
        assert!(md.contains("## Connections — common"), "common connections");
        assert!(md.contains("`a.rs` → `b.rs` (uses)"), "edge line");
        assert!(
            md.contains("191019-ADP.md") || md.contains("-ADP.md"),
            "save-report name carries preset id"
        );
    }

    #[test]
    fn cycle_groups_rank_chain_first_then_size() {
        let mut level = level_with(vec![
            file_node("{target}/m1.rs", &[("hk", AttrValue::Float(9.0))]),
            file_node("{target}/m2.rs", &[("hk", AttrValue::Float(1.0))]),
            file_node("{target}/c1.rs", &[("hk", AttrValue::Float(1.0))]),
            file_node("{target}/c2.rs", &[("hk", AttrValue::Float(5.0))]),
            file_node("{target}/c3.rs", &[("hk", AttrValue::Float(2.0))]),
        ]);
        level.cycles = vec![
            CycleGroup {
                kind: "mutual".into(),
                nodes: vec!["{target}/m1.rs".into(), "{target}/m2.rs".into()],
            },
            CycleGroup {
                kind: "chain".into(),
                nodes: vec![
                    "{target}/c1.rs".into(),
                    "{target}/c2.rs".into(),
                    "{target}/c3.rs".into(),
                ],
            },
        ];
        // --top 1 picks the chain (chains rank before mutuals), and lists all of
        // its members ordered by HK (c2 → c3 → c1).
        let top = top_cycle_groups(&level, 1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0.kind, "chain");
        let ids: Vec<&str> = top[0].1.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["{target}/c2.rs", "{target}/c3.rs", "{target}/c1.rs"]);
    }

    #[test]
    fn compose_prompt_metric_orders_and_respects_top() {
        let level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(2000.0)),
                    ("sloc", AttrValue::Int(300)),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(50.0)),
                    ("sloc", AttrValue::Int(100)),
                ],
            ),
        ]);
        let presets = vec![Preset {
            id: "SRP".into(),
            label: "SRP".into(),
            title: "SRP — Single".into(),
            prompt: "one reason".into(),
            doc_url: None,
            sort_metric: "sloc".into(),
            connections: vec![],
        }];
        let md = compose_prompt(&level, &presets, "SRP", Severity::Warning, Some(1)).unwrap();
        assert!(
            md.contains("## Modules ordered by SLOC"),
            "ordered heading: {md}"
        );
        assert!(
            md.contains("- `a.rs` (SLOC: 300)"),
            "worst module with value: {md}"
        );
        assert!(
            !md.contains("- `b.rs`"),
            "--top 1 keeps only the worst: {md}"
        );
    }

    #[test]
    fn compose_prompt_unknown_preset_errors() {
        let level = level_with(vec![]);
        let presets = vec![Preset {
            id: "ADP".into(),
            label: "ADP".into(),
            title: "t".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "cycle".into(),
            connections: vec![],
        }];
        let err = compose_prompt(&level, &presets, "NOPE", Severity::Auto, None).unwrap_err();
        assert!(format!("{err}").contains("unknown --preset 'NOPE'"));
    }

    #[test]
    fn scorecard_shows_principle_and_worst_modules() {
        let level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(50.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(2000.0)),
                    ("sloc", AttrValue::Int(300)),
                ],
            ),
        ]);
        let presets = vec![
            Preset {
                id: "ADP".into(),
                label: "ADP".into(),
                title: "ADP — Acyclic Dependencies".into(),
                prompt: "p".into(),
                doc_url: None,
                sort_metric: "cycle".into(),
                connections: vec![],
            },
            Preset {
                id: "SRP".into(),
                label: "SRP".into(),
                title: "SRP — Single Responsibility".into(),
                prompt: "p".into(),
                doc_url: None,
                sort_metric: "sloc".into(),
                connections: vec![],
            },
        ];
        let sc = render_scorecard(
            "rust",
            &level,
            &presets,
            &[Severity::Warning, Severity::Info],
            None,
            None,
        )
        .unwrap();
        assert!(sc.contains("scorecard  (rust, 2 files)"), "header: {sc}");
        assert!(
            sc.contains("ADP") && sc.contains("Acyclic Dependencies"),
            "ADP row"
        );
        assert!(sc.contains("WORST MODULES"), "modules section");
        assert!(
            sc.contains("a.rs") && sc.contains("cycle"),
            "cycle node listed: {sc}"
        );
        assert!(
            sc.contains("b.rs") && sc.contains("HK"),
            "hk breach listed: {sc}"
        );
        assert!(
            sc.contains("→ code-ranker report . --preset"),
            "next-step hint"
        );
    }

    /// A cycle preset for the narrowed-scorecard tests.
    fn adp_preset() -> Preset {
        Preset {
            id: "ADP".into(),
            label: "ADP".into(),
            title: "ADP — Acyclic Dependencies".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "cycle".into(),
            connections: vec![],
        }
    }

    fn srp_preset() -> Preset {
        Preset {
            id: "SRP".into(),
            label: "SRP".into(),
            title: "SRP — Single Responsibility".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "sloc".into(),
            connections: vec![],
        }
    }

    /// Narrowing on a metric preset lists that metric's ranked modules under
    /// WORST MODULES (the `narrow.is_some()` non-cycle branch).
    #[test]
    fn scorecard_narrowed_metric_lists_ranked_modules() {
        let level = level_with(vec![
            file_node("{target}/big.rs", &[("sloc", AttrValue::Int(300))]),
            file_node("{target}/small.rs", &[("sloc", AttrValue::Int(10))]),
        ]);
        let sc = render_scorecard(
            "rust",
            &level,
            &[srp_preset()],
            &[Severity::Warning],
            Some(2),
            Some("SRP"),
        )
        .unwrap();
        assert!(sc.contains("WORST MODULES"), "modules section: {sc}");
        assert!(
            sc.contains("big.rs") && sc.contains("SLOC 300"),
            "ranked module with metric head: {sc}"
        );
        // Worst-first: big.rs before small.rs.
        assert!(
            sc.find("big.rs") < sc.find("small.rs"),
            "ranked worst-first: {sc}"
        );
    }

    /// Narrowing on the cycle (ADP) preset lists every member of the top cycle
    /// (the `narrow.is_some()` cycle branch), with the "one cycle" header.
    #[test]
    fn scorecard_narrowed_cycle_lists_all_members() {
        let mut level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[
                    ("hk", AttrValue::Float(80.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
            file_node(
                "{target}/b.rs",
                &[
                    ("hk", AttrValue::Float(50.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
        ]);
        level.cycles.push(CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
        });
        let sc = render_scorecard(
            "rust",
            &level,
            &[adp_preset()],
            &[Severity::Warning],
            None,
            Some("ADP"),
        )
        .unwrap();
        assert!(
            sc.contains("one cycle (mutual, 2 modules)"),
            "single-cycle header: {sc}"
        );
        assert!(
            sc.contains("a.rs") && sc.contains("b.rs"),
            "all cycle members listed: {sc}"
        );
    }

    /// An unknown `--preset` (narrow) is a hard error naming the known presets.
    #[test]
    fn scorecard_unknown_narrow_preset_errors() {
        let level = level_with(vec![file_node("{target}/a.rs", &[])]);
        let err = render_scorecard(
            "rust",
            &level,
            &[srp_preset()],
            &[Severity::Auto],
            None,
            Some("ZZZ"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("unknown --preset 'ZZZ'"),
            "names bad id: {err}"
        );
        assert!(err.contains("SRP"), "lists known presets: {err}");
    }

    /// Info-tier breaches: a node over the info line (but under warning) is shown
    /// with the ⓘ icon, and a worse metric pushes a co-occurring cycle into the
    /// `+rest` list (the non-cycle-worst path).
    #[test]
    fn scorecard_info_tier_and_cycle_in_rest() {
        let level = level_with(vec![
            // info-only: sloc 80 > info 50, < warning 200.
            file_node("{target}/info.rs", &[("sloc", AttrValue::Int(80))]),
            // warning hk (ratio 2.0) beats the cycle (ratio 1.0) → cycle in +rest.
            file_node(
                "{target}/hot.rs",
                &[
                    ("hk", AttrValue::Float(2000.0)),
                    ("cycle", AttrValue::Str("mutual".into())),
                ],
            ),
        ]);
        let sc = render_scorecard(
            "rust",
            &level,
            &[srp_preset()],
            &[Severity::Warning, Severity::Info],
            None,
            None,
        )
        .unwrap();
        assert!(
            sc.contains("info.rs") && sc.contains("ⓘ"),
            "info icon: {sc}"
        );
        assert!(
            sc.contains("hot.rs") && sc.contains("+cycle"),
            "cycle shown as a secondary breach: {sc}"
        );
    }

    /// With nothing over the selected tier, the scorecard says so and stops.
    #[test]
    fn scorecard_reports_no_breaches_when_clean() {
        let level = level_with(vec![file_node(
            "{target}/quiet.rs",
            &[("hk", AttrValue::Float(10.0)), ("sloc", AttrValue::Int(5))],
        )]);
        let sc = render_scorecard(
            "rust",
            &level,
            &[srp_preset()],
            &[Severity::Warning],
            None,
            None,
        )
        .unwrap();
        assert!(
            sc.contains("No threshold breaches for the selected severity."),
            "clean report: {sc}"
        );
    }

    /// A two-cycle level: builds nodes + two `CycleGroup`s, returned ready for the
    /// ADP (cycle) preset.
    fn two_cycle_level() -> LevelGraph {
        let mut level = level_with(vec![
            file_node(
                "{target}/a.rs",
                &[("cycle", AttrValue::Str("mutual".into()))],
            ),
            file_node(
                "{target}/b.rs",
                &[("cycle", AttrValue::Str("mutual".into()))],
            ),
            file_node(
                "{target}/x.rs",
                &[("cycle", AttrValue::Str("chain".into()))],
            ),
            file_node(
                "{target}/y.rs",
                &[("cycle", AttrValue::Str("chain".into()))],
            ),
            file_node(
                "{target}/z.rs",
                &[("cycle", AttrValue::Str("chain".into()))],
            ),
        ]);
        level.cycles = vec![
            CycleGroup {
                kind: "chain".into(),
                nodes: vec![
                    "{target}/x.rs".into(),
                    "{target}/y.rs".into(),
                    "{target}/z.rs".into(),
                ],
            },
            CycleGroup {
                kind: "mutual".into(),
                nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
            },
        ];
        level
    }

    /// `--top 2` on the ADP prompt lists each cycle under its own heading (the
    /// multi-cycle branch of `compose_prompt`).
    #[test]
    fn compose_prompt_lists_multiple_cycles() {
        let level = two_cycle_level();
        let md = compose_prompt(&level, &[adp_preset()], "ADP", Severity::Auto, Some(2)).unwrap();
        assert!(
            md.contains("## 2 dependency cycles"),
            "multi-cycle header: {md}"
        );
        assert!(
            md.contains("### Cycle 1 — chain, 3 modules")
                && md.contains("### Cycle 2 — mutual, 2 modules"),
            "per-cycle headings: {md}"
        );
    }

    /// Narrowed ADP scorecard with `--top 2` uses the plural "N cycles" header.
    #[test]
    fn scorecard_narrowed_cycle_top_n_header() {
        let level = two_cycle_level();
        let sc = render_scorecard(
            "rust",
            &level,
            &[adp_preset()],
            &[Severity::Warning],
            Some(2),
            Some("ADP"),
        )
        .unwrap();
        assert!(
            sc.contains("2 cycles — all members listed:"),
            "header: {sc}"
        );
    }

    /// Narrowed ADP scorecard when there are no cycles at all → "(none)".
    #[test]
    fn scorecard_narrowed_cycle_with_none_says_none() {
        let level = level_with(vec![file_node("{target}/a.rs", &[])]);
        let sc = render_scorecard(
            "rust",
            &level,
            &[adp_preset()],
            &[Severity::Warning],
            None,
            Some("ADP"),
        )
        .unwrap();
        assert!(sc.contains("(none)"), "empty modules list: {sc}");
    }

    /// A principle name longer than the column width is clipped with an ellipsis.
    #[test]
    fn scorecard_clips_long_principle_name() {
        let level = level_with(vec![file_node(
            "{target}/a.rs",
            &[("hk", AttrValue::Float(2000.0))],
        )]);
        let preset = Preset {
            id: "LONG".into(),
            label: "LONG".into(),
            title: "LONG — A Very Long Principle Name That Exceeds The Column".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "hk".into(),
            connections: vec![],
        };
        let sc =
            render_scorecard("rust", &level, &[preset], &[Severity::Warning], None, None).unwrap();
        assert!(sc.contains('…'), "long name clipped with ellipsis: {sc}");
    }

    #[test]
    fn parse_severity_rejects_garbage() {
        assert_eq!(parse_severity("warning").unwrap(), Severity::Warning);
        assert!(parse_severity("nope").is_err());
    }
}
