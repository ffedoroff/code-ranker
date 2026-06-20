//! The console triage scorecard behind the `scorecard` report format — a
//! per-principle table mirroring the viewer's per-preset badges, plus the worst
//! modules overall.

use super::{
    Severity, attr_short, clean_path, file_count, fmt_val, in_cycle, is_internal, num, reco_for,
    top_cycle_groups,
};
use anyhow::Result;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::{Preset, node::Node};

/// One metric (or cycle) breach on a node, with its tier.
struct Breach {
    metric: String,
    warning: bool,
    /// `value / threshold` — how far over the line (for picking the worst metric).
    ratio: f64,
    value: f64,
}

/// Every selected-tier threshold a node breaches, plus cycle membership (treated
/// as a warning-tier signal — a cycle is always a real problem).
fn node_breaches(
    level: &LevelGraph,
    node: &Node,
    want_warning: bool,
    want_info: bool,
) -> Vec<Breach> {
    let mut out = Vec::new();
    for (metric, spec) in &level.node_attributes {
        let Some(th) = spec.thresholds else { continue };
        let Some(v) = num(node, metric) else { continue };
        if v > th.warning && want_warning {
            out.push(Breach {
                metric: metric.clone(),
                warning: true,
                ratio: if th.warning > 0.0 {
                    v / th.warning
                } else {
                    f64::INFINITY
                },
                value: v,
            });
        } else if v > th.info && want_info {
            out.push(Breach {
                metric: metric.clone(),
                warning: false,
                ratio: if th.info > 0.0 {
                    v / th.info
                } else {
                    f64::INFINITY
                },
                value: v,
            });
        }
    }
    if want_warning && in_cycle(node) {
        out.push(Breach {
            metric: "cycle".into(),
            warning: true,
            ratio: 1.0,
            value: 0.0,
        });
    }
    out
}

/// Render the console triage scorecard: a per-principle table (warning/info
/// counts + the worst module) followed by the worst modules overall, then a hint
/// pointing at the prompt for the worst principle.
pub fn render_scorecard(
    plugin: &str,
    level: &LevelGraph,
    presets: &[Preset],
    severities: &[Severity],
    top: Option<usize>,
    narrow: Option<&str>,
) -> Result<String> {
    let want_warning = severities
        .iter()
        .any(|s| matches!(s, Severity::Warning | Severity::Auto));
    let want_info = severities
        .iter()
        .any(|s| matches!(s, Severity::Info | Severity::Auto));

    // Narrowing focuses the scorecard on one ranking axis (a metric, e.g. `hk`,
    // `cycle`, `sloc`). Validate it, then keep the presets that rank by it for the
    // table; the worst-modules list ranks by the metric directly.
    if let Some(m) = narrow {
        let known = m == "cycle" || level.node_attributes.contains_key(m);
        if !known {
            // List the ranking axes the presets actually use (plus `cycle`) — the
            // meaningful narrowing values, not every node attribute.
            let mut metrics: Vec<&str> = presets.iter().map(|p| p.sort_metric.as_str()).collect();
            metrics.push("cycle");
            metrics.sort_unstable();
            metrics.dedup();
            anyhow::bail!(
                "unknown --metric '{m}'. Known metrics: {}",
                metrics.join(", ")
            );
        }
    }
    let shown_presets: Vec<&Preset> = match narrow {
        Some(m) => presets.iter().filter(|p| p.sort_metric == m).collect(),
        None => presets.iter().collect(),
    };

    let mut out = String::new();
    out.push_str(&format!(
        "scorecard  ({plugin}, {} files)\n\n",
        file_count(level)
    ));

    // ── Per-principle table ──────────────────────────────────────────────────
    struct Row {
        id: String,
        name: String,
        warn: usize,
        info: usize,
        top: String,
    }
    let mut rows: Vec<Row> = Vec::new();
    for p in &shown_presets {
        let reco = reco_for(level, &p.sort_metric);
        // Skip presets with nothing in the selected tiers (unless narrowed).
        let in_scope =
            (want_warning && reco.warning_count > 0) || (want_info && reco.info_count > 0);
        if narrow.is_none() && !in_scope {
            continue;
        }
        let top_module = match reco.sorted.first() {
            Some(n) if p.sort_metric == "cycle" => format!("{} (cycle)", clean_path(&n.id)),
            Some(n) => match num(n, &p.sort_metric) {
                Some(v) if v != 0.0 => format!(
                    "{} ({} {})",
                    clean_path(&n.id),
                    attr_short(level, &p.sort_metric),
                    fmt_val(level, &p.sort_metric, v)
                ),
                _ => clean_path(&n.id),
            },
            None => "—".to_string(),
        };
        rows.push(Row {
            id: p.id.clone(),
            // Strip a leading "ID — " from the title to keep the column short.
            name: p
                .title
                .split_once(" — ")
                .map(|(_, rest)| rest)
                .unwrap_or(&p.title)
                .to_string(),
            warn: reco.warning_count,
            info: reco.info_count,
            top: top_module,
        });
    }
    rows.sort_by(|a, b| b.warn.cmp(&a.warn).then(b.info.cmp(&a.info)));

    if rows.is_empty() && narrow.is_none() {
        out.push_str("No threshold breaches for the selected severity.\n");
        return Ok(out);
    }

    // The per-principle table (skipped when narrowed to a metric no preset ranks
    // by — the worst-modules list below carries the ranking instead).
    if !rows.is_empty() {
        let id_w = rows.iter().map(|r| r.id.len()).max().unwrap_or(6).max(6);
        let name_w = rows
            .iter()
            .map(|r| r.name.len())
            .max()
            .unwrap_or(9)
            .clamp(9, 34);
        let clip = |s: &str, w: usize| -> String {
            if s.len() > w {
                format!("{}…", &s[..w.saturating_sub(1)])
            } else {
                s.to_string()
            }
        };
        let mut header = format!("{:<id_w$}  {:<name_w$}", "PRESET", "PRINCIPLE");
        if want_warning {
            header.push_str("  ⚠");
        }
        if want_info {
            header.push_str("  ⓘ");
        }
        header.push_str("  TOP MODULE");
        out.push_str(&header);
        out.push('\n');
        for r in &rows {
            let mut line = format!("{:<id_w$}  {:<name_w$}", r.id, clip(&r.name, name_w));
            if want_warning {
                line.push_str(&format!("  {:>1}", r.warn));
            }
            if want_info {
                line.push_str(&format!("  {:>1}", r.info));
            }
            line.push_str(&format!("  {}", r.top));
            out.push_str(&line);
            out.push('\n');
        }
    }

    // ── Worst modules ────────────────────────────────────────────────────────
    out.push_str("\nWORST MODULES\n");
    let limit = top.unwrap_or(15);

    struct ModRow {
        warning_icon: bool,
        path: String,
        head: String,
        rest: Vec<String>,
        n_warn: usize,
        n_info: usize,
        hk: f64,
    }
    let mut mod_rows: Vec<ModRow> = Vec::new();

    if let Some(m) = narrow {
        // Narrowed: the chosen metric's ranked modules.
        if m == "cycle" {
            // ADP: `--top` counts CYCLES (default 1 — the biggest chain). List
            // every member of each selected cycle so the whole loop is visible.
            let groups = top_cycle_groups(level, top.unwrap_or(1));
            match groups.as_slice() {
                [(g, members)] => out.push_str(&format!(
                    "  one cycle ({}, {} modules) — all members listed; fix one cycle at a \
                     time (avoid --top 2+):\n",
                    g.kind,
                    members.len()
                )),
                _ => out.push_str(&format!(
                    "  {} cycles — all members listed:\n",
                    groups.len()
                )),
            }
            for (g, members) in &groups {
                for n in members {
                    mod_rows.push(ModRow {
                        warning_icon: true,
                        path: clean_path(&n.id),
                        head: g.kind.clone(),
                        rest: Vec::new(),
                        n_warn: 0,
                        n_info: 0,
                        hk: num(n, "hk").unwrap_or(0.0),
                    });
                }
            }
        } else {
            let reco = reco_for(level, m);
            for n in reco.sorted.iter().take(limit) {
                let head = match num(n, m) {
                    Some(v) if v != 0.0 => {
                        format!("{} {}", attr_short(level, m), fmt_val(level, m, v))
                    }
                    _ => attr_short(level, m).to_string(),
                };
                mod_rows.push(ModRow {
                    warning_icon: true,
                    path: clean_path(&n.id),
                    head,
                    rest: Vec::new(),
                    n_warn: 0,
                    n_info: 0,
                    hk: num(n, "hk").unwrap_or(0.0),
                });
            }
        }
    } else {
        for n in level.nodes.iter().filter(|n| is_internal(n)) {
            let breaches = node_breaches(level, n, want_warning, want_info);
            if breaches.is_empty() {
                continue;
            }
            let n_warn = breaches.iter().filter(|b| b.warning).count();
            let n_info = breaches.iter().filter(|b| !b.warning).count();
            // Worst metric = the largest over-threshold ratio.
            let worst = breaches
                .iter()
                .max_by(|a, b| a.ratio.total_cmp(&b.ratio))
                .unwrap();
            let head = if worst.metric == "cycle" {
                "cycle".to_string()
            } else {
                format!(
                    "{} {}",
                    attr_short(level, &worst.metric),
                    fmt_val(level, &worst.metric, worst.value)
                )
            };
            let rest: Vec<String> = breaches
                .iter()
                .filter(|b| b.metric != worst.metric)
                .map(|b| {
                    if b.metric == "cycle" {
                        "cycle".to_string()
                    } else {
                        attr_short(level, &b.metric).to_string()
                    }
                })
                .collect();
            mod_rows.push(ModRow {
                warning_icon: n_warn > 0,
                path: clean_path(&n.id),
                head,
                rest,
                n_warn,
                n_info,
                hk: num(n, "hk").unwrap_or(0.0),
            });
        }
        mod_rows.sort_by(|a, b| {
            b.n_warn
                .cmp(&a.n_warn)
                .then(b.n_info.cmp(&a.n_info))
                .then(b.hk.total_cmp(&a.hk))
        });
        mod_rows.truncate(limit);
    }

    if mod_rows.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let path_w = mod_rows.iter().map(|r| r.path.len()).max().unwrap_or(0);
        for (i, r) in mod_rows.iter().enumerate() {
            let icon = if r.warning_icon { "⚠" } else { "ⓘ" };
            let mut line = format!("{:>2} {} {:<path_w$}  {}", i + 1, icon, r.path, r.head);
            if !r.rest.is_empty() {
                line.push_str(&format!("  +{}", r.rest.join(", ")));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    // ── Next-step hint ───────────────────────────────────────────────────────
    out.push_str("\n→ code-ranker report . --output.prompt.path=… --top 1\n");

    Ok(out)
}
