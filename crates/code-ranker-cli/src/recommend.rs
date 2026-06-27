//! The recommendation engine behind the `--prompt <ID>` output and the `scorecard`
//! report format.
//!
//! It is the console counterpart of the HTML viewer's Prompt Generator: the same
//! ranking (`reco_for` ≈ `recoFor` in `export-popup.js`) and the same Markdown
//! prompt (`compose_prompt` ≈ `composePrompt` + `buildContent`), plus a console
//! triage table (`render_scorecard`) that mirrors the viewer's per-principle badges.
//!
//! All of it is **advisory**, derived from the snapshot's gate-driven
//! `node_attributes[*].thresholds` (the `info` / `warning` tiers) — never a gate.
//!
//! Output can be focused on one axis by name ([`Focus`]): a **metric** (`hk`)
//! frames it by the metric itself, a **principle** id (`LSP`) by that design
//! principle. Resolution lives in [`resolve_focus`].

use anyhow::{Context, Result, bail};
use code_ranker_graph::level_graph::{CycleGroup, LevelGraph};
use code_ranker_graph::snapshot::{LanguageSnapshot, Snapshot};
pub use code_ranker_plugin_api::Principle;
use code_ranker_plugin_api::{
    attrs::{AttrValue, ValueType},
    level::Thresholds,
    node::Node,
};
use std::collections::HashMap;

/// Select the `LanguageSnapshot` to use for recommendations.
///
/// Resolution order:
/// 1. `--language` explicitly given → use that language or error.
/// 2. Single language → use it (no ambiguity).
/// 3. Multiple languages + `id` given → search all; if `id` matches exactly
///    one language, use it; if 2+ match → error listing them.
/// 4. Multiple languages + no `id` → use the first (BTreeMap order); this
///    path is taken only for scorecard/prompt without `--focus`.
pub fn resolve_language_snap<'a>(
    snap: &'a Snapshot,
    language: Option<&str>,
    id: Option<&str>,
) -> Result<&'a LanguageSnapshot> {
    // Explicit `--language` always wins.
    if let Some(lang) = language {
        return snap.languages.get(lang).with_context(|| {
            let available: Vec<&str> = snap.languages.keys().map(String::as_str).collect();
            format!(
                "language {lang:?} not found in snapshot; available: {}",
                available.join(", ")
            )
        });
    }

    // Single language: no ambiguity.
    if snap.languages.len() == 1 {
        return Ok(snap.languages.values().next().expect("len==1"));
    }

    // Multiple languages: try to resolve the id across all of them.
    if let Some(focus_id) = id {
        let matches: Vec<&str> = snap
            .languages
            .iter()
            .filter_map(|(lang, ls)| {
                // A match is: it is a principle id, or a metric key in the files level.
                let is_principle = ls.principles.iter().any(|p| p.id == focus_id);
                let is_metric = ls
                    .graphs
                    .get("files")
                    .is_some_and(|g| g.node_attributes.contains_key(focus_id));
                (is_principle || is_metric).then_some(lang.as_str())
            })
            .collect();

        match matches.as_slice() {
            [one] => return Ok(snap.languages.get(*one).expect("key from languages")),
            [] => {} // fall through to first-language default
            langs => anyhow::bail!(
                "{focus_id:?} found in languages: {}; specify --language <name> to disambiguate",
                langs.join(", ")
            ),
        }
    }

    // Fall back to the first language (BTreeMap order, deterministic).
    snap.languages
        .values()
        .next()
        .context("snapshot has no languages; regenerate the report with `code-ranker report`")
}

mod prompt;
mod scorecard;

pub use prompt::compose_prompt;
pub use scorecard::render_scorecard;

/// What `--focus <NAME>` resolves to: a SOLID-style design **principle** (a principle
/// id, e.g. `LSP`) or a **metric** (an attribute key, e.g. `hk`). The two live in
/// separate namespaces — principle ids are upper-case codes, metric keys are
/// lower-case — so a name maps unambiguously to one lens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    Principle(String),
    Metric(String),
}

/// Resolve a `--focus` name to a [`Focus`]. Accepts, in order: a principle id
/// (exact, e.g. `LSP`) → a principle; a metric — by its bare key or a full
/// threshold rule id, case-insensitive (`HK`, `hk`, `threshold.file.hk` all →
/// `hk`), matched against any attribute the level carries (by value, so it works
/// whether or not the metric has a configured threshold) or the `cycle`
/// pseudo-metric. Unknown names are fatal, listing both namespaces.
pub fn resolve_focus(level: &LevelGraph, principles: &[Principle], name: &str) -> Result<Focus> {
    if principles.iter().any(|p| p.id == name) {
        return Ok(Focus::Principle(name.to_string()));
    }
    // A metric can be named bare (`hk`) or as a full rule id (`threshold.file.hk`);
    // the metric key is the segment after the last dot. Match case-insensitively.
    let bare = name.rsplit('.').next().unwrap_or(name);
    for candidate in [name, bare] {
        let key = candidate.to_lowercase();
        if key == "cycle" || level.node_attributes.contains_key(&key) {
            return Ok(Focus::Metric(key));
        }
    }
    let mut metrics: Vec<&str> = level
        .node_attributes
        .iter()
        .filter(|(_, s)| matches!(s.value_type, ValueType::Int | ValueType::Float))
        .map(|(k, _)| k.as_str())
        .collect();
    metrics.push("cycle");
    metrics.sort_unstable();
    let principles: Vec<&str> = principles.iter().map(|p| p.id.as_str()).collect();
    bail!(
        "unknown --focus '{name}'. Metrics: {}. Principles: {}",
        metrics.join(", "),
        principles.join(", ")
    );
}

/// Build a throwaway [`Principle`] that frames a **metric** as its own principle, so
/// the metric-lens prompt reuses [`compose_prompt`] verbatim — the title is the
/// metric (`HK — Henry–Kafura`), the summary its `description`, the `doc_url` its
/// base-corpus doc stem (resolved by key), and the ranking axis the metric
/// itself. No SOLID principle is involved. Coupling metrics also pull the in/out
/// connection lists (the HK fix workflow needs the crossroads); others omit them.
pub fn synth_metric_principle(
    level: &LevelGraph,
    principles: &[Principle],
    metric: &str,
) -> Principle {
    // The `cycle` pseudo-metric IS the ADP principle's ranking axis (ADP's
    // `sort_metric` is literally `cycle`). Frame its prompt like ADP — borrow that
    // principle's title, prompt body, connection set, and doc — so `--focus cycle`
    // reads almost exactly like `--focus ADP`; the scorecard still keeps the
    // metric lens (its header comes from `metric_focus_label`, not this principle, and
    // it drops the principle table). Falls through to generic framing if absent.
    if metric == "cycle"
        && let Some(adp) = principles.iter().find(|p| p.sort_metric == "cycle")
    {
        return Principle {
            id: metric.to_string(),
            label: metric.to_string(),
            title: adp.title.clone(),
            prompt: adp.prompt.clone(),
            doc_url: adp.doc_url.clone(),
            sort_metric: metric.to_string(),
            connections: adp.connections.clone(),
        };
    }
    let spec = level.node_attributes.get(metric);
    let label = spec
        .and_then(|s| s.short.as_deref().or(s.label.as_deref()))
        .unwrap_or(metric);
    let name = spec.and_then(|s| s.name.as_deref()).unwrap_or(label);
    let title = if name == label {
        label.to_string()
    } else {
        format!("{label} — {name}")
    };
    // The doc this metric's prompt points at — its base-corpus doc stem (`hk`→`HK`),
    // or `None` for a metric that ships no prose doc.
    let doc_url = crate::templates::metric_doc_stem(metric).map(str::to_string);
    let connections = if spec.and_then(|s| s.group.as_deref()) == Some("coupling") {
        vec!["in".to_string(), "out".to_string(), "common".to_string()]
    } else {
        Vec::new()
    };
    Principle {
        id: metric.to_string(),
        label: label.to_string(),
        title,
        prompt: spec.and_then(|s| s.description.clone()).unwrap_or_default(),
        doc_url,
        sort_metric: metric.to_string(),
        connections,
    }
}

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

/// The two-tier thresholds for a metric, or `None` when it has no configured
/// threshold. A metric is gated (and ranked by breach count) only by its OWN
/// threshold — there is no cross-metric fallback, and an unconfigured metric has
/// zero breaches rather than treating a `0` limit as "every node breaches".
fn thresholds_for(level: &LevelGraph, metric: &str) -> Option<Thresholds> {
    level.node_attributes.get(metric).and_then(|s| s.thresholds)
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

/// Whether `node` falls under one of the `--focus-path` entries (empty = no
/// restriction). Mirrors `check`'s path matching: an entry matches a file exactly
/// or, as a folder, anything beneath it; leading `./` and a trailing `/` are
/// normalized so `./crates/a/` and `crates/a` are equivalent.
pub(super) fn in_focus(node: &Node, focus_paths: &[String]) -> bool {
    if focus_paths.is_empty() {
        return true;
    }
    let rel = clean_path(&node.id);
    focus_paths.iter().any(|f| {
        let f = f.trim_start_matches("./").trim_end_matches('/');
        !f.is_empty() && (rel == f || rel.starts_with(&format!("{f}/")))
    })
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
    // No configured threshold → no breaches (the metric still ranks for display,
    // but never claims violations and so never contributes to the scorecard counts).
    let (warning_count, info_count) = match th {
        Some(th) => (
            sorted
                .iter()
                .filter(|n| num(n, metric).unwrap_or(0.0) > th.warning)
                .count(),
            sorted
                .iter()
                .filter(|n| num(n, metric).unwrap_or(0.0) > th.info)
                .count(),
        ),
        None => (0, 0),
    };
    Reco {
        sorted,
        warning_count,
        info_count,
    }
}

/// Cycle groups ranked worst-first for the ADP (cycle) principle: `chain` cycles
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
/// skipped. This is the unit the ADP principle recommends on: `--top` counts
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

/// Count of project source files in the level.
pub(super) fn file_count(level: &LevelGraph) -> usize {
    level.nodes.iter().filter(|n| is_internal(n)).count()
}

/// Format a metric value for CLI output: the exact rounded integer (never
/// abbreviated — the K/M/G `abbreviate` spec flag is a viewer-only concern, so the
/// scorecard and prompt always show the precise number, e.g. `295488` not `295.5K`).
pub(super) fn fmt_val(v: f64) -> String {
    format!("{}", v.round() as i64)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "recommend_test.rs"]
mod tests;
