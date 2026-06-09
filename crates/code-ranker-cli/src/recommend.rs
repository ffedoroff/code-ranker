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
fn num(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// A project source file (not a third-party library node).
fn is_internal(node: &Node) -> bool {
    node.kind != "external"
}

/// Is this file node in a dependency cycle? (the orchestrator writes a `cycle`
/// string attribute on every cycle member).
fn in_cycle(node: &Node) -> bool {
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
fn attr_short<'a>(level: &'a LevelGraph, metric: &'a str) -> &'a str {
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
fn top_cycle_groups(level: &LevelGraph, n_groups: usize) -> Vec<(&CycleGroup, Vec<&Node>)> {
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
fn tier_count(reco: &Reco, sev: Severity) -> usize {
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
fn file_count(level: &LevelGraph) -> usize {
    level.nodes.iter().filter(|n| is_internal(n)).count()
}

/// Format a metric value: abbreviate large numbers to K/M/G when the attribute
/// is flagged `abbreviate`, else a plain rounded integer.
fn fmt_val(level: &LevelGraph, metric: &str, v: f64) -> String {
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

/// Compose the AI prompt for one principle — the same Markdown the HTML viewer's
/// Prompt Generator produces: intent + summary + principle link + task checklist,
/// then the ranked offending modules, then the preset's connection lists.
pub fn compose_prompt(
    level: &LevelGraph,
    presets: &[Preset],
    preset_id: &str,
    sev: Severity,
    top: Option<usize>,
) -> Result<String> {
    let Some(preset) = presets.iter().find(|p| p.id == preset_id) else {
        let known: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        bail!(
            "unknown --preset '{preset_id}'. Known presets: {}",
            known.join(", ")
        );
    };

    let reco = reco_for(level, &preset.sort_metric);
    // For the cycle (ADP) preset the unit is a whole cycle group, not a node:
    // `--top` counts CYCLES (default 1 — the single biggest chain), and every
    // member of each selected cycle is listed. Other presets rank nodes, and
    // the default count = the active tier's size (≥ 1).
    let is_cycle = preset.sort_metric == "cycle";
    let cycle_groups = if is_cycle {
        top_cycle_groups(level, top.unwrap_or(1))
    } else {
        Vec::new()
    };
    let modules: Vec<&Node> = if is_cycle {
        cycle_groups
            .iter()
            .flat_map(|(_, members)| members.iter().copied())
            .collect()
    } else {
        let n = top.unwrap_or_else(|| tier_count(&reco, sev).max(1));
        reco.sorted.iter().take(n).copied().collect()
    };

    let mut parts: Vec<String> = Vec::new();

    // 1. Principle intent + summary + link + task protocol.
    let mut head = String::new();
    head.push_str(&format!("# {}\n\n", preset.title));
    head.push_str("I want to apply this to some modules in my system.\n\n");
    head.push_str("## Summary\n\n");
    head.push_str(&preset.prompt);
    head.push_str("\n\n");
    if let Some(url) = &preset.doc_url {
        head.push_str(&format!("**Full principle:** [{url}]({url})\n\n"));
        head.push_str(
            "Download and read the full principle to understand it in detail. \
             If you cannot download it, **stop the task immediately**.\n\n",
        );
    }
    head.push_str("## Task\n\n");
    head.push_str(
        "- Prepare a precise, detailed estimate and a report of where the modules below violate it.\n",
    );
    head.push_str(
        "- If you find more serious violations elsewhere during research, mention them in the report too.\n",
    );
    head.push_str("- Show a summary of the report in chat.\n");
    head.push_str(&format!(
        "- If any violation is found, suggest saving the report to a file as a plan for a detailed review, named `.code-ranker/<YYYYMMDD-HHMMSS>-{preset_id}.md`.\n\n",
    ));
    head.push_str("**Focus the research and report primarily on the modules below.**");
    parts.push(head);

    // 2. The offending modules, ordered by the preset's metric (or listed as a
    //    cycle for cycle-based principles), each annotated with its value.
    if !modules.is_empty() {
        if is_cycle {
            let mut s = String::new();
            if cycle_groups.len() == 1 {
                let (g, members) = &cycle_groups[0];
                s.push_str(&format!(
                    "## Modules in a dependency cycle ({}, {} modules)\n\n",
                    g.kind,
                    members.len()
                ));
                s.push_str(
                    "This is **one** dependency cycle; every module in it is listed below so the \
                     whole loop is visible. Fix one cycle at a time — `--top 2`+ lists several \
                     separate cycles at once and obscures how each one connects.\n\n",
                );
                for n in members {
                    s.push_str(&format!("- `{}`\n", clean_path(&n.id)));
                }
            } else {
                s.push_str(&format!(
                    "## {} dependency cycles (every member listed)\n\n",
                    cycle_groups.len()
                ));
                for (i, (g, members)) in cycle_groups.iter().enumerate() {
                    s.push_str(&format!(
                        "### Cycle {} — {}, {} modules\n\n",
                        i + 1,
                        g.kind,
                        members.len()
                    ));
                    for n in members {
                        s.push_str(&format!("- `{}`\n", clean_path(&n.id)));
                    }
                    s.push('\n');
                }
            }
            parts.push(s.trim_end().to_string());
        } else {
            let m = &preset.sort_metric;
            let label = attr_short(level, m);
            let mut s = format!("## Modules ordered by {label}\n\n");
            if let Some(spec) = level.node_attributes.get(m) {
                if let Some(d) = &spec.description {
                    s.push_str(d);
                    s.push_str("\n\n");
                }
                if let Some(f) = &spec.formula {
                    s.push_str(&format!("**Formula:** `{f}`\n\n"));
                }
            }
            for n in &modules {
                match num(n, m) {
                    Some(v) if v != 0.0 => s.push_str(&format!(
                        "- `{}` ({label}: {})\n",
                        clean_path(&n.id),
                        fmt_val(level, m, v)
                    )),
                    _ => s.push_str(&format!("- `{}`\n", clean_path(&n.id))),
                }
            }
            parts.push(s.trim_end().to_string());
        }
    }

    // 3. The preset's connection lists (only those with edges), endpoints as paths.
    let module_ids: std::collections::HashSet<&str> =
        modules.iter().map(|n| n.id.as_str()).collect();
    let internal: std::collections::HashSet<&str> = level
        .nodes
        .iter()
        .filter(|n| is_internal(n))
        .map(|n| n.id.as_str())
        .collect();
    let local_edges: Vec<&code_ranker_plugin_api::edge::Edge> = level
        .edges
        .iter()
        .filter(|e| internal.contains(e.source.as_str()) && internal.contains(e.target.as_str()))
        .collect();

    let edge_line = |e: &code_ranker_plugin_api::edge::Edge| {
        format!(
            "- `{}` → `{}` ({})",
            clean_path(&e.source),
            clean_path(&e.target),
            e.kind
        )
    };
    let push_conn =
        |parts: &mut Vec<String>, title: &str, edges: Vec<&code_ranker_plugin_api::edge::Edge>| {
            if edges.is_empty() {
                return;
            }
            let mut s = format!("## Connections — {title}\n\n");
            s.push_str(
                &edges
                    .iter()
                    .map(|e| edge_line(e))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            parts.push(s);
        };

    let wants = |c: &str| preset.connections.iter().any(|x| x == c);
    if wants("common") {
        let inner: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.source.as_str()) && module_ids.contains(e.target.as_str())
            })
            .collect();
        push_conn(&mut parts, "common", inner);
    }
    if wants("in") {
        let ins: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.target.as_str()) && !module_ids.contains(e.source.as_str())
            })
            .collect();
        push_conn(&mut parts, "in", ins);
    }
    if wants("out") {
        let outs: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.source.as_str()) && !module_ids.contains(e.target.as_str())
            })
            .collect();
        push_conn(&mut parts, "out", outs);
    }

    let mut out = parts.join("\n\n");
    out.push('\n');
    Ok(out)
}

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

    // Narrowing focuses the whole report on one principle.
    let shown_presets: Vec<&Preset> = match narrow {
        Some(id) => {
            let p = presets.iter().find(|p| p.id == id).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown --preset '{id}'. Known presets: {}",
                    presets
                        .iter()
                        .map(|p| p.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            vec![p]
        }
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

    if rows.is_empty() {
        out.push_str("No threshold breaches for the selected severity.\n");
        return Ok(out);
    }

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

    if narrow.is_some() {
        // Narrowed: the chosen principle's ranked modules.
        let preset = shown_presets[0];
        if preset.sort_metric == "cycle" {
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
            let reco = reco_for(level, &preset.sort_metric);
            for n in reco.sorted.iter().take(limit) {
                let head = match num(n, &preset.sort_metric) {
                    Some(v) if v != 0.0 => format!(
                        "{} {}",
                        attr_short(level, &preset.sort_metric),
                        fmt_val(level, &preset.sort_metric, v)
                    ),
                    _ => attr_short(level, &preset.sort_metric).to_string(),
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
    let hint_preset = narrow
        .map(str::to_string)
        .or_else(|| worst_preset(level, presets));
    if let Some(p) = hint_preset {
        out.push_str(&format!(
            "\n→ code-ranker report . --preset {p} --output.prompt.path=…\n"
        ));
    }

    Ok(out)
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

    #[test]
    fn parse_severity_rejects_garbage() {
        assert_eq!(parse_severity("warning").unwrap(), Severity::Warning);
        assert!(parse_severity("nope").is_err());
    }
}
