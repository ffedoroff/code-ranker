//! The AI prompt builder behind the `prompt` report format — the console
//! counterpart of the HTML viewer's Prompt Generator (`composePrompt` +
//! `buildContent`).

use super::{
    Severity, attr_short, clean_path, fmt_val, is_internal, num, reco_for, tier_count,
    top_cycle_groups,
};
use anyhow::{Result, bail};
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::{node::Node, plugin::Preset};

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
