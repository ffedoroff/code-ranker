//! The console triage scorecard behind the `scorecard` report format — a
//! per-principle table mirroring the viewer's per-principle badges, plus the worst
//! modules overall.

use super::{
    Severity, attr_short, clean_path, file_count, fmt_val, in_cycle, in_focus, is_internal, num,
    reco_for, top_cycle_groups,
};
use anyhow::Result;
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::{Principle, node::Node};

/// One metric (or cycle) breach on a node, with its tier.
struct Breach {
    metric: String,
    warning: bool,
    /// `value / threshold` — how far over the line (for picking the worst metric).
    ratio: f64,
    value: f64,
}

/// One row of the per-principle table.
struct Row {
    id: String,
    name: String,
    warn: usize,
    info: usize,
    top: String,
}

/// One row of the worst-modules list.
struct ModRow {
    is_warning: bool,
    path: String,
    head: String,
    rest: Vec<String>,
    n_warn: usize,
    n_info: usize,
    hk: f64,
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
/// pointing at `--prompt <ID>` for a specific principle/metric.
pub fn render_scorecard(
    plugin: &str,
    level: &LevelGraph,
    principles: &[Principle],
    severities: &[Severity],
    top: Option<usize>,
    focus: Option<&super::Focus>,
    focus_paths: &[String],
) -> Result<String> {
    let want_warning = severities
        .iter()
        .any(|s| matches!(s, Severity::Warning | Severity::Auto));
    let want_info = severities
        .iter()
        .any(|s| matches!(s, Severity::Info | Severity::Auto));

    // `--focus` picks the lens. A metric frames the scorecard by that metric alone
    // (no principle rows — the worst-modules list carries the ranking); a principle
    // shows just that principle's row; without it, the full per-principle triage. The
    // metric the worst-modules list ranks by is the focused metric, the focused
    // principle's `sort_metric`, or none (a breach-ranked list).
    let (shown_principles, narrow): (Vec<&Principle>, Option<&str>) = match focus {
        Some(super::Focus::Metric(m)) => (Vec::new(), Some(m.as_str())),
        Some(super::Focus::Principle(id)) => {
            let p: Vec<&Principle> = principles.iter().filter(|p| &p.id == id).collect();
            let m = p.first().map(|p| p.sort_metric.as_str());
            (p, m)
        }
        None => (principles.iter().collect(), None),
    };

    let mut out = String::new();
    out.push_str(&format!(
        "scorecard  ({plugin}, {} files)\n\n",
        file_count(level)
    ));
    // A metric lens names what it is focused on (there is no principle row to).
    if let Some(super::Focus::Metric(m)) = focus {
        out.push_str(&format!("focus: {}\n", metric_focus_label(level, m)));
    }

    // ── Per-principle table ──────────────────────────────────────────────────
    let mut rows = principle_rows(level, &shown_principles, narrow, want_warning, want_info);
    rows.sort_by(|a, b| b.warn.cmp(&a.warn).then(b.info.cmp(&a.info)));

    if rows.is_empty() && focus.is_none() {
        out.push_str("No threshold breaches for the selected severity.\n");
        return Ok(out);
    }

    // The per-principle table (skipped under a metric lens — the worst-modules list
    // below carries the ranking instead).
    if !rows.is_empty() {
        render_principle_table(&mut out, &rows, want_warning, want_info);
    }

    // ── Worst modules ────────────────────────────────────────────────────────
    out.push_str("\nWORST MODULES\n");
    let limit = top.unwrap_or(15);

    let mod_rows = match narrow {
        // Focused on a metric: that metric's ranked modules (may emit a heading).
        Some(m) => narrowed_mod_rows(&mut out, level, m, top, limit, focus_paths),
        // Otherwise: every internal node with a breach, ranked by severity.
        None => breach_mod_rows(level, want_warning, want_info, limit, focus_paths),
    };

    if mod_rows.is_empty() {
        out.push_str("  (none)\n");
    } else {
        render_mod_rows(&mut out, &mod_rows);
    }

    // ── Next-step hint ───────────────────────────────────────────────────────
    out.push_str(
        "\n→ code-ranker report . --prompt <PRINCIPLE|METRIC>   (AI fix-prompt to stdout)\n",
    );

    Ok(out)
}

/// The metric lens's header label: `HK — Henry–Kafura` (short/label + `name`),
/// or just the key when no richer names exist.
fn metric_focus_label(level: &LevelGraph, m: &str) -> String {
    if m == "cycle" {
        return "cycle — dependency cycles".to_string();
    }
    let spec = level.node_attributes.get(m);
    let label = spec
        .and_then(|s| s.short.as_deref().or(s.label.as_deref()))
        .unwrap_or(m);
    match spec.and_then(|s| s.name.as_deref()) {
        Some(n) if n != label => format!("{label} — {n}"),
        _ => label.to_string(),
    }
}

/// Build the per-principle table rows from the shown principles.
fn principle_rows(
    level: &LevelGraph,
    shown_principles: &[&Principle],
    narrow: Option<&str>,
    want_warning: bool,
    want_info: bool,
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for p in shown_principles {
        let reco = reco_for(level, &p.sort_metric);
        // Skip principles with nothing in the selected tiers (unless narrowed).
        let in_scope =
            (want_warning && reco.warning_count > 0) || (want_info && reco.info_count > 0);
        if narrow.is_none() && !in_scope {
            continue;
        }
        let top_module = principle_top_module(level, p, &reco);
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
    rows
}

/// The "top module" cell for a principle row: the worst-ranked module under the
/// principle's metric, annotated with the metric value (or `(cycle)` / a bare path).
fn principle_top_module(level: &LevelGraph, p: &Principle, reco: &super::Reco) -> String {
    match reco.sorted.first() {
        Some(n) if p.sort_metric == "cycle" => format!("{} (cycle)", clean_path(&n.id)),
        Some(n) => match num(n, &p.sort_metric) {
            Some(v) if v != 0.0 => format!(
                "{} ({} {})",
                clean_path(&n.id),
                attr_short(level, &p.sort_metric),
                fmt_val(v)
            ),
            _ => clean_path(&n.id),
        },
        None => "—".to_string(),
    }
}

/// Render the per-principle table (header + one line per row) into `out`.
fn render_principle_table(out: &mut String, rows: &[Row], want_warning: bool, want_info: bool) {
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
        header.push_str("  WARN");
    }
    if want_info {
        header.push_str("  INFO");
    }
    header.push_str("  TOP MODULE");
    out.push_str(&header);
    out.push('\n');
    for r in rows {
        let mut line = format!("{:<id_w$}  {:<name_w$}", r.id, clip(&r.name, name_w));
        if want_warning {
            line.push_str(&format!("  {:>4}", r.warn));
        }
        if want_info {
            line.push_str(&format!("  {:>4}", r.info));
        }
        line.push_str(&format!("  {}", r.top));
        out.push_str(&line);
        out.push('\n');
    }
}

/// Worst-modules rows when narrowed to one metric (or the `cycle` pseudo-metric).
/// May push an explanatory heading line into `out` (the cycle branch does).
fn narrowed_mod_rows(
    out: &mut String,
    level: &LevelGraph,
    m: &str,
    top: Option<usize>,
    limit: usize,
    focus_paths: &[String],
) -> Vec<ModRow> {
    if m == "cycle" {
        // A cycle is a global unit, so `--focus-path` does not narrow its members.
        cycle_mod_rows(out, level, top)
    } else {
        metric_mod_rows(level, m, limit, focus_paths)
    }
}

/// Cycle-narrowed worst-modules rows: list every member of each selected cycle
/// (so the whole loop is visible). Pushes the explanatory heading into `out`.
fn cycle_mod_rows(out: &mut String, level: &LevelGraph, top: Option<usize>) -> Vec<ModRow> {
    // ADP: `--top` counts CYCLES (default 1 — the biggest chain).
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
    let mut mod_rows: Vec<ModRow> = Vec::new();
    for (g, members) in &groups {
        for n in members {
            mod_rows.push(ModRow {
                is_warning: true,
                path: clean_path(&n.id),
                head: g.kind.clone(),
                rest: Vec::new(),
                n_warn: 0,
                n_info: 0,
                hk: num(n, "hk").unwrap_or(0.0),
            });
        }
    }
    mod_rows
}

/// Metric-narrowed worst-modules rows: the metric's ranked modules, capped,
/// restricted to `--focus-path` (empty = no restriction).
fn metric_mod_rows(
    level: &LevelGraph,
    m: &str,
    limit: usize,
    focus_paths: &[String],
) -> Vec<ModRow> {
    let reco = reco_for(level, m);
    reco.sorted
        .iter()
        .filter(|n| in_focus(n, focus_paths))
        .take(limit)
        .map(|n| {
            let head = match num(n, m) {
                Some(v) if v != 0.0 => {
                    format!("{} {}", attr_short(level, m), fmt_val(v))
                }
                _ => attr_short(level, m).to_string(),
            };
            ModRow {
                is_warning: true,
                path: clean_path(&n.id),
                head,
                rest: Vec::new(),
                n_warn: 0,
                n_info: 0,
                hk: num(n, "hk").unwrap_or(0.0),
            }
        })
        .collect()
}

/// Worst-modules rows for the unnarrowed view: every internal node with a breach
/// in the selected tiers, ranked by warning/info counts then `hk`, truncated.
fn breach_mod_rows(
    level: &LevelGraph,
    want_warning: bool,
    want_info: bool,
    limit: usize,
    focus_paths: &[String],
) -> Vec<ModRow> {
    let mut mod_rows: Vec<ModRow> = Vec::new();
    for n in level
        .nodes
        .iter()
        .filter(|n| is_internal(n) && in_focus(n, focus_paths))
    {
        let breaches = node_breaches(level, n, want_warning, want_info);
        if breaches.is_empty() {
            continue;
        }
        mod_rows.push(breach_row(level, n, &breaches));
    }
    mod_rows.sort_by(|a, b| {
        b.n_warn
            .cmp(&a.n_warn)
            .then(b.n_info.cmp(&a.n_info))
            .then(b.hk.total_cmp(&a.hk))
    });
    mod_rows.truncate(limit);
    mod_rows
}

/// Build the worst-modules row for one node from its (non-empty) breach list:
/// headline the worst metric (largest over-threshold ratio) and tag the rest.
fn breach_row(level: &LevelGraph, n: &Node, breaches: &[Breach]) -> ModRow {
    let n_warn = breaches.iter().filter(|b| b.warning).count();
    let n_info = breaches.iter().filter(|b| !b.warning).count();
    // Worst metric = the largest over-threshold ratio.
    let worst = breaches
        .iter()
        .max_by(|a, b| a.ratio.total_cmp(&b.ratio))
        .unwrap();
    let head = breach_label(level, &worst.metric, Some(worst.value));
    let rest: Vec<String> = breaches
        .iter()
        .filter(|b| b.metric != worst.metric)
        .map(|b| breach_label(level, &b.metric, None))
        .collect();
    ModRow {
        is_warning: n_warn > 0,
        path: clean_path(&n.id),
        head,
        rest,
        n_warn,
        n_info,
        hk: num(n, "hk").unwrap_or(0.0),
    }
}

/// Short label for one breached metric: `"cycle"` for the cycle pseudo-metric,
/// else the metric's short name, optionally suffixed with its formatted value.
fn breach_label(level: &LevelGraph, metric: &str, value: Option<f64>) -> String {
    if metric == "cycle" {
        return "cycle".to_string();
    }
    match value {
        Some(v) => format!("{} {}", attr_short(level, metric), fmt_val(v)),
        None => attr_short(level, metric).to_string(),
    }
}

/// Render the worst-modules list (one numbered line per row) into `out`.
fn render_mod_rows(out: &mut String, mod_rows: &[ModRow]) {
    let path_w = mod_rows.iter().map(|r| r.path.len()).max().unwrap_or(0);
    for (i, r) in mod_rows.iter().enumerate() {
        let tier = if r.is_warning { "warn" } else { "info" };
        let mut line = format!("{:>2} {:<4} {:<path_w$}  {}", i + 1, tier, r.path, r.head);
        if !r.rest.is_empty() {
            line.push_str(&format!("  +{}", r.rest.join(", ")));
        }
        out.push_str(&line);
        out.push('\n');
    }
}
